//! Spec bots: scripted characters that have **no scenario pack and no cast
//! entry**, driven entirely by a small TOML script (`name` + `greeting` +
//! optional `triggers`). One character runs this way (docs/design.md):
//!
//! **the mayor**, whose QR poster hangs at the base station. His greeting is
//! the official notice — fires, networks down, be careful — and his trigger
//! is the endgame: paste the line Nadia saw in his own written order
//! (`secret_tip` in her pack) into his chat and he comes apart.
//!
//! He does not appear in `larp-cast.toml`, and he runs in exactly one place:
//! the base-station Pi (his identity is only flashed there — see
//! characters.just). One identity, one card, one op log; there is no
//! multi-instance identity anywhere any more. Nadia's bot shares that Pi,
//! which is how her eruption sees his collapse.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::time::Duration;

use anyhow::{Context, Result, bail};
#[allow(deprecated)]
use dashchat_node::FakeAgentId;
use dashchat_node::{ChatId, DeviceId, InboxPayload, Node, Payload, Profile};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::identity::IdentityBundle;
use crate::scenario::normalize;

fn default_poll_interval_secs() -> u64 {
    crate::config::Timing::default().poll_interval_secs
}

/// A phrase the bot listens for in player messages, and what it answers.
///
/// Matching is the same forgiving containment the delivery recognizer uses
/// (whitespace collapsed, lowercased, extra prose around it tolerated): a
/// player pasting the password with a quote header still lands it.
#[derive(Clone, Debug, Deserialize)]
pub struct Trigger {
    /// What a player's message must contain.
    pub phrase: String,
    /// The answer, sent in order — once per player message that matches.
    pub reply: Vec<String>,
}

/// A spec bot's script (`mayor.toml`, baked into the
/// image).
#[derive(Clone, Debug, Deserialize)]
pub struct Spec {
    /// Display name for the chat profile.
    pub name: String,
    /// Sent in order, once, when a player's contact request is accepted.
    pub greeting: Vec<String>,
    /// Phrases to listen for afterwards. Empty means the bot only ever greets.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
}

impl Spec {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading spec {}", path.as_ref().display()))?;
        let spec: Self = toml::from_str(&raw)
            .with_context(|| format!("parsing spec {}", path.as_ref().display()))?;
        spec.lint()?;
        Ok(spec)
    }

    pub fn lint(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("spec: empty name");
        }
        if self.greeting.is_empty() || self.greeting.iter().any(|m| m.trim().is_empty()) {
            bail!("spec {:?}: greeting must be a non-empty list of non-empty messages", self.name);
        }
        let mut phrases: BTreeSet<String> = BTreeSet::new();
        for trigger in &self.triggers {
            if trigger.phrase.trim().is_empty() {
                bail!("spec {:?}: empty trigger phrase", self.name);
            }
            if trigger.reply.is_empty() || trigger.reply.iter().any(|m| m.trim().is_empty()) {
                bail!(
                    "spec {:?}: trigger {:?} must reply with a non-empty list of non-empty messages",
                    self.name,
                    trigger.phrase
                );
            }
            if !phrases.insert(normalize(&trigger.phrase)) {
                bail!("spec {:?}: duplicate trigger phrase {:?}", self.name, trigger.phrase);
            }
        }
        // A phrase hiding inside another would make a paste ambiguous, the
        // same way nested mission texts would (see Scenarios::lint).
        for outer in &phrases {
            for inner in &phrases {
                if outer != inner && outer.contains(inner.as_str()) {
                    bail!(
                        "spec {:?}: trigger phrase {inner:?} is contained in {outer:?}",
                        self.name
                    );
                }
            }
        }
        Ok(())
    }

    /// The trigger a player's message fires, if any. Longest phrase wins, so
    /// the match stays deterministic even if a future spec nests phrases.
    pub fn triggered_by(&self, text: &str) -> Option<&Trigger> {
        let haystack = normalize(text);
        if haystack.is_empty() {
            return None;
        }
        self.triggers
            .iter()
            .filter(|t| haystack.contains(&normalize(&t.phrase)))
            .max_by_key(|t| t.phrase.len())
    }
}

/// `larp-bot spec` configuration: a spec bot runs as its own daemon next to a
/// station's character bot, with its own identity and data dir.
#[derive(Clone, Debug, Deserialize)]
pub struct SpecConfig {
    /// Mailbox the bot syncs through (the station's own, like the character bot).
    pub mailbox_url: String,
    /// The flashed identity bundle (see characters.just).
    pub identity: PathBuf,
    /// The script file (`mayor.toml`, baked into the image).
    pub spec: PathBuf,
    /// Optional chat avatar PNG. Explicit (unlike the scenario packs' sibling
    /// convention) because the spec is deployed as a lone store file.
    #[serde(default)]
    pub avatar: Option<PathBuf>,
    /// Node data dir. A cache: safe to wipe, identity comes from the bundle.
    pub data_dir: PathBuf,
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    /// Where to touch the flag when a trigger fires. Defaults to `triggered`
    /// in the data dir — but the mayor's deployment points it somewhere the
    /// neighbour's bot can actually see: both services run as DynamicUser,
    /// and a DynamicUser StateDirectory really lives under /var/lib/private
    /// (0700, root-only), so a flag inside it is invisible across services
    /// (nix/larp-bot.nix puts it in /var/lib/larp instead).
    #[serde(default)]
    pub triggered_flag: Option<PathBuf>,
}

impl SpecConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading config {}", path.as_ref().display()))?;
        toml::from_str(&raw).context("parsing spec bot config")
    }
}

/// Persistent spec-bot state (`state.json` in the data dir). A cache like the
/// data dir itself: wiping it re-tells players at worst.
///
/// Keyed by the requester's hex *device* id, not their agent id: that is what
/// the direct-chat topic is derived from ([`Node::direct_chat_topic`] takes a
/// `FakeAgentId`, which is a device id). Pre-0.19 state files hold agent ids,
/// so they are effectively ignored — worst case a player is told twice.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SpecState {
    /// Contact requests already accepted (hex device ids).
    #[serde(default)]
    pub accepted: BTreeSet<String>,
    /// Contacts the greeting was sent to (hex device ids).
    #[serde(default)]
    pub told: BTreeSet<String>,
    /// Player messages a trigger already answered (hex op hashes), so a
    /// re-sync or a restart never replays the endgame.
    #[serde(default)]
    pub answered: BTreeSet<String>,
}

impl SpecState {
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|err| {
                warn!(?err, "state.json unreadable, starting fresh");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

pub struct SpecBot {
    node: Node,
    me: DeviceId,
    spec: Spec,
    profile_avatar: Option<String>,
    poll: Duration,
    state: SpecState,
    state_path: PathBuf,
    /// Touched when a trigger fires (see [`SpecConfig::triggered_flag`]).
    triggered_flag: PathBuf,
    /// True when this process started with no state file at all — a wipe (or
    /// first boot). While true, the first scan of each chat BASELINES it:
    /// every message already there is marked answered without a reply. The
    /// mailbox and the players' phones keep the full history through a wipe,
    /// so without this the mayor would replay his collapse (and re-write the
    /// flag) from old messages — which happened on 2026-09-02. Never true on
    /// a normal restart, so a live player's fast paste is never swallowed.
    fresh_start: bool,
    /// Chats already baselined this run (in-memory; only used while
    /// `fresh_start`).
    baselined_chats: BTreeSet<String>,
}

/// Run a spec bot daemon: seed identity, start the node, register the
/// mailbox, then loop forever (accept contact requests, send the greeting,
/// answer triggers).
pub async fn run(config: SpecConfig) -> Result<()> {
    let bundle = IdentityBundle::load(&config.identity)?;
    let spec = Spec::load(&config.spec)?;
    // Same agreement the character bot enforces: the name a QR poster
    // embeds (bundle) must match the profile the bot publishes (spec).
    anyhow::ensure!(
        bundle.qr_profile_name() == spec.name,
        "identity bundle name {:?} != spec profile name {:?} for {:?} — \
         set profile_name in secrets/{}-identity.toml",
        bundle.qr_profile_name(),
        spec.name,
        bundle.character,
        bundle.character,
    );

    let (node, notification_rx) =
        crate::bot::build_node(&config.data_dir, &bundle, crate::bot::bot_node_config()).await?;
    info!(
        character = %bundle.character,
        device_id = %hex::encode(bundle.device_id()?.as_bytes()),
        triggers = spec.triggers.len(),
        "spec bot node up"
    );

    crate::bot::register_mailbox(&node, &config.mailbox_url).await;

    let avatar = config
        .avatar
        .as_deref()
        .map(crate::scenario::png_data_uri)
        .transpose()?;
    let state_path = config.data_dir.join("state.json");
    SpecBot::new(
        node,
        bundle.device_id()?,
        spec,
        avatar,
        Duration::from_secs(config.poll_interval_secs.max(1)),
        state_path,
        config.triggered_flag,
    )
    .run_loop(notification_rx)
    .await
}

impl SpecBot {
    pub fn new(
        node: Node,
        me: DeviceId,
        spec: Spec,
        profile_avatar: Option<String>,
        poll: Duration,
        state_path: PathBuf,
        triggered_flag: Option<PathBuf>,
    ) -> Self {
        let triggered_flag =
            triggered_flag.unwrap_or_else(|| state_path.with_file_name("triggered"));
        let fresh_start = !state_path.exists();
        Self {
            node,
            me,
            spec,
            profile_avatar,
            poll,
            state: SpecState::load(&state_path),
            state_path,
            triggered_flag,
            fresh_start,
            baselined_chats: BTreeSet::new(),
        }
    }

    /// Re-authored every boot, same as `Bot::ensure_profile`: the mailbox's
    /// blob cleanup outlives its watermarks, so a once-published profile
    /// becomes unfetchable for new accounts after 7 days.
    async fn ensure_profile(&self) -> Result<()> {
        self.node
            .set_profile(Profile {
                name: self.spec.name.clone(),
                surname: None,
                avatar: self.profile_avatar.clone(),
                about: None,
            })
            .await?;
        Ok(())
    }

    pub async fn run_loop(
        mut self,
        mut notifications: mpsc::Receiver<dashchat_node::Notification>,
    ) -> Result<()> {
        self.ensure_profile().await?;
        let mut tick = tokio::time::interval(self.poll);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                maybe = notifications.recv() => {
                    match maybe {
                        Some(notification) => {
                            if let Err(err) = self.handle_notification(notification).await {
                                warn!(?err, "notification handling failed");
                            }
                        }
                        None => anyhow::bail!("node notification channel closed"),
                    }
                }
                _ = tick.tick() => {
                    if let Err(err) = self.tick().await {
                        warn!(?err, "tick failed");
                    }
                }
            }
        }
    }

    /// Accept incoming contact requests. `accept_contact` on the accepting
    /// side also creates the direct chat space, so the greeting in [`tick`]
    /// has a chat to land in.
    ///
    /// The requester's device id is the op author — the request payload itself
    /// only carries their agent id — and the device id is what [`tick`] needs
    /// to derive the direct chat, so that is what gets recorded.
    async fn handle_notification(&mut self, n: dashchat_node::Notification) -> Result<()> {
        let Some(op) = n.op() else { return Ok(()) };
        // Same as Bot::handle_notification: the player's contact request
        // precedes the greeting, so stepping the clock here (crate::clock)
        // stamps even the first bot message with the phone's real time.
        crate::clock::step_clock_forward(u64::from(op.header.timestamp));
        let Some(Payload::Inbox(InboxPayload::ContactRequest {
            agent_id, profile, ..
        })) = &op.payload
        else {
            return Ok(());
        };
        let device = DeviceId::from(op.header.verifying_key);
        let requester = device.to_string();
        if *agent_id == self.node.agent_id() || self.state.accepted.contains(&requester) {
            return Ok(());
        }
        info!(name = %profile.name, "accepting contact request");
        self.node
            .accept_contact(*agent_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        self.state.accepted.insert(requester);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    async fn tick(&mut self) -> Result<()> {
        self.greet_pending().await?;
        if self.spec.triggers.is_empty() {
            return Ok(());
        }
        for (_, chat) in
            crate::bot::direct_chats(&self.node, self.me, &self.state.accepted).await?
        {
            self.answer_triggers(chat).await?;
        }
        Ok(())
    }

    /// Send the greeting to every accepted-but-untold contact. Separate from
    /// acceptance so a failed send is retried next tick (and after restarts).
    async fn greet_pending(&mut self) -> Result<()> {
        let pending: Vec<String> = self
            .state
            .accepted
            .difference(&self.state.told)
            .cloned()
            .collect();
        for requester in pending {
            let device = DeviceId::from_str(&requester)
                .with_context(|| format!("state device id {requester:?} is not a public key"))?;
            #[allow(deprecated)] // FakeAgentId is what direct_chat_topic takes today
            let chat = self.node.direct_chat_topic(FakeAgentId::from(device));
            info!(to = %requester, "sending the greeting");
            for message in &self.spec.greeting {
                crate::bot::typing_pause(message).await;
                self.node
                    .send_message(chat, message.clone(), None, None)
                    .await?;
            }
            self.state.told.insert(requester);
            self.state.save(&self.state_path)?;
        }
        Ok(())
    }

    /// Answer any player message carrying a trigger phrase — the mayor's
    /// downfall. Once per message: the op hash is persisted before the reply
    /// is considered done, so a re-sync can't replay the collapse.
    ///
    /// Only *player*-authored messages count. This bot's own lines are
    /// skipped, which is what lets a spec quote its own phrases safely.
    async fn answer_triggers(&mut self, chat: ChatId) -> Result<()> {
        // A wiped bot re-syncs history it already answered once (the dedup
        // hashes died with the state): the first scan of each chat after a
        // fresh start swallows what's already there instead of replying.
        let baseline =
            self.fresh_start && !self.baselined_chats.contains(&chat.to_string());
        let mut replies: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut swallowed = 0usize;
        for (author, op_hash, content) in crate::bot::chat_messages(&self.node, chat).await? {
            let text = content.message();
            if author == self.me || self.state.answered.contains(&op_hash) {
                continue;
            }
            if baseline {
                self.state.answered.insert(op_hash);
                swallowed += 1;
                continue;
            }
            if let Some(trigger) = self.spec.triggered_by(text) {
                info!(phrase = %trigger.phrase, "trigger phrase received");
                replies.push((op_hash, author.to_string(), trigger.reply.clone()));
            }
        }
        if baseline {
            self.baselined_chats.insert(chat.to_string());
            if swallowed > 0 {
                info!(chat = %chat, swallowed, "fresh start: baselined pre-existing messages");
                self.state.save(&self.state_path)?;
            }
            return Ok(());
        }
        for (op_hash, felled_by, reply) in replies {
            // The first line is threaded onto the message that triggered it —
            // the mayor answering the evidence the player just put in front of
            // him — and the rest of his unravelling follows as plain messages.
            // A target the node refuses falls back to a plain send: a collapse
            // that never arrives would strand the endgame.
            let mut target = op_hash.parse().ok();
            for message in reply {
                crate::bot::typing_pause(&message).await;
                if target.is_some() {
                    match self
                        .node
                        .send_message(chat, message.clone(), None, target.take())
                        .await
                    {
                        Ok(_) => continue,
                        Err(err) => {
                            warn!(?err, "could not thread the answer as a reply, sending plain");
                        }
                    }
                }
                self.node.send_message(chat, message, None, None).await?;
            }
            self.state.answered.insert(op_hash);
            self.state.save(&self.state_path)?;
            // Signal the fall to the outside: append the triggering player's
            // device id to the flag file. Nadia's character bot shares this
            // Pi and polls the path (BotConfig::mayor_fallen_flag) to erupt —
            // only in the chats of the players named in the flag, since the
            // fall is THEIR payoff, not the whole town's news feed. Appended,
            // not overwritten: several players can each fell him. An empty
            // flag names nobody (a bare `touch` is deliberately inert).
            let flag = self.triggered_flag_path();
            let written = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&flag)
                .and_then(|mut f| {
                    use std::io::Write as _;
                    f.write_all(format!("{felled_by}\n").as_bytes())
                });
            if let Err(err) = written {
                warn!(path = %flag.display(), ?err, "could not write the triggered flag");
            }
        }
        Ok(())
    }

    /// Where this bot records that one of its triggers has fired.
    pub fn triggered_flag_path(&self) -> PathBuf {
        self.triggered_flag.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAYOR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mayor.toml");
    const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios");

    fn spec() -> Spec {
        toml::from_str(
            r#"
            name = "Fixture"
            greeting = ["the mayor lies", "the code is x"]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn lint_accepts_the_fixture() {
        spec().lint().unwrap();
    }

    #[test]
    fn lint_rejects_empty_pieces() {
        let mut s = spec();
        s.greeting.clear();
        assert!(s.lint().is_err());
        let mut s = spec();
        s.greeting.push("  ".into());
        assert!(s.lint().is_err());
    }

    #[test]
    fn lint_rejects_a_trigger_with_no_reply() {
        let s: Spec = toml::from_str(
            r#"
            name = "Mayor"
            greeting = ["citizens"]
            [[triggers]]
            phrase = "open up"
            reply = []
            "#,
        )
        .unwrap();
        assert!(s.lint().is_err());
    }

    #[test]
    fn lint_rejects_a_phrase_nested_in_another() {
        let s: Spec = toml::from_str(
            r#"
            name = "Mayor"
            greeting = ["citizens"]
            [[triggers]]
            phrase = "open"
            reply = ["a"]
            [[triggers]]
            phrase = "open up"
            reply = ["b"]
            "#,
        )
        .unwrap();
        assert!(s.lint().is_err());
    }

    #[test]
    fn triggers_match_through_case_whitespace_and_extra_prose() {
        let s: Spec = toml::from_str(
            r#"
            name = "Mayor"
            greeting = ["citizens"]
            [[triggers]]
            phrase = "let the north side burn"
            reply = ["caught"]
            "#,
        )
        .unwrap();
        s.lint().unwrap();
        // What a player actually pastes: Nadia's whole secret message, case
        // mangled, wrapped in their own words.
        assert!(
            s.triggered_by("look what they sent me:\n  LET THE NORTH   side burn \n— his own words")
                .is_some()
        );
        assert!(s.triggered_by("let the north side").is_none());
        assert!(s.triggered_by("").is_none());
    }

    #[test]
    fn shipped_specs_lint() {
        Spec::load(MAYOR).unwrap();
    }

    /// The mayor never encourages the communications he secretly cut: his
    /// greeting must not name Nadia or teach the courier job — the printed
    /// sign and Nadia's own greeting carry the onboarding instead.
    #[test]
    fn the_mayor_keeps_out_of_the_comms() {
        let mayor = Spec::load(MAYOR).unwrap();
        for line in &mayor.greeting {
            assert!(
                !line.contains("Nadia") && !normalize(line).contains("copy"),
                "the mayor's greeting promotes the comms: {line:?}"
            );
        }
    }

    /// The whole side plot in one assertion: what Nadia's secret hands out
    /// has to be what the mayor listens for, or the endgame is unreachable.
    #[test]
    fn the_secret_tip_hands_out_what_the_mayor_listens_for() {
        let mayor = Spec::load(MAYOR).unwrap();
        assert!(!mayor.triggers.is_empty(), "the mayor has no endgame trigger");
        let scenarios = crate::scenario::Scenarios::load_dir(Path::new(SCENARIOS)).unwrap();
        let told = scenarios
            .packs
            .values()
            .filter_map(|p| p.secret_tip.as_deref())
            .collect::<Vec<_>>()
            .concat();
        for trigger in &mayor.triggers {
            assert!(
                normalize(&told).contains(&normalize(&trigger.phrase)),
                "no secret_tip gives out {:?} — players cannot end the game",
                trigger.phrase
            );
        }
    }
}
