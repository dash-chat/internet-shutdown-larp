//! The anonymous informant: the hidden character with no scenario pack and no
//! cast entry (no other character knows he exists). Players find his QR poster
//! somewhere in town; when their contact request reaches a station that runs
//! him, he accepts and whispers the truth about the mayor into the direct
//! chat — the whole secret, five-tap trick and password both.
//!
//! The same identity bundle runs on every station at once. That is safe
//! enough because p2panda logs are per (device, topic): the instances only
//! ever collide on the announcements topic (all branches carry the same
//! "Anonymous" profile — whichever arrives first wins, the other is dropped
//! per-op) and on a direct chat when several stations accept the *same*
//! player (the player keeps the first station's chat; every station tells
//! the full story, so it doesn't matter which one they keep).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use dashchat_node::{AgentId, InboxPayload, Node, Payload, Profile};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::identity::IdentityBundle;

fn default_poll_interval_secs() -> u64 {
    crate::config::Timing::default().poll_interval_secs
}

/// The informant's script (`anonymous.toml`, baked into the image): what to
/// whisper, in order, to every player who scans the hidden poster.
#[derive(Clone, Debug, Deserialize)]
pub struct AnonymousSpec {
    /// Display name for the chat profile.
    pub name: String,
    /// The messages every station sends, in order — the whole secret.
    pub reveal: Vec<String>,
}

impl AnonymousSpec {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading anonymous spec {}", path.as_ref().display()))?;
        let spec: Self = toml::from_str(&raw).context("parsing anonymous spec")?;
        spec.lint()?;
        Ok(spec)
    }

    pub fn lint(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("anonymous spec: empty name");
        }
        if self.reveal.is_empty() || self.reveal.iter().any(|m| m.trim().is_empty()) {
            bail!("anonymous spec: reveal must be a non-empty list of non-empty messages");
        }
        Ok(())
    }
}

/// `larp-bot anonymous` configuration: the informant runs as a second daemon
/// next to a station's character bot, with its own identity and data dir.
#[derive(Clone, Debug, Deserialize)]
pub struct AnonymousConfig {
    /// Mailbox the bot syncs through (the station's own, like the character bot).
    pub mailbox_url: String,
    /// The flashed anonymous identity bundle (see characters.just).
    pub identity: PathBuf,
    /// The script file (`anonymous.toml`, baked into the image).
    pub spec: PathBuf,
    /// Optional chat avatar PNG. Explicit (unlike the scenario packs' sibling
    /// convention) because the spec is deployed as a lone store file.
    #[serde(default)]
    pub avatar: Option<PathBuf>,
    /// Node data dir. A cache: safe to wipe, identity comes from the bundle.
    pub data_dir: PathBuf,
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
}

impl AnonymousConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading config {}", path.as_ref().display()))?;
        toml::from_str(&raw).context("parsing anonymous config")
    }
}

/// Persistent informant state (`state.json` in the data dir). A cache like
/// the data dir itself: wiping it re-tells players at worst.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AnonymousState {
    /// Contact requests already accepted (hex agent ids).
    pub accepted: BTreeSet<String>,
    /// Contacts the full script was sent to (hex agent ids).
    pub told: BTreeSet<String>,
}

impl AnonymousState {
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

pub struct AnonymousBot {
    node: Node,
    profile_name: String,
    profile_avatar: Option<String>,
    script: Vec<String>,
    poll: Duration,
    state: AnonymousState,
    state_path: PathBuf,
}

/// Run the informant daemon: seed identity, start the node, register the
/// mailbox, then loop forever (accept contact requests, whisper the script).
pub async fn run(config: AnonymousConfig) -> Result<()> {
    let bundle = IdentityBundle::load(&config.identity)?;
    let spec = AnonymousSpec::load(&config.spec)?;
    let script = spec.reveal.clone();

    let (node, notification_rx) =
        crate::bot::build_node(&config.data_dir, &bundle, crate::bot::bot_node_config()).await?;
    info!(
        character = %bundle.character,
        device_id = %hex::encode(bundle.device_id()?.as_bytes()),
        "anonymous node up"
    );

    crate::bot::register_mailbox(&node, &config.mailbox_url).await;

    let avatar = config
        .avatar
        .as_deref()
        .map(crate::scenario::png_data_uri)
        .transpose()?;
    let state_path = config.data_dir.join("state.json");
    AnonymousBot::new(
        node,
        spec.name,
        avatar,
        script,
        Duration::from_secs(config.poll_interval_secs.max(1)),
        state_path,
    )
    .run_loop(notification_rx)
    .await
}

impl AnonymousBot {
    pub fn new(
        node: Node,
        profile_name: String,
        profile_avatar: Option<String>,
        script: Vec<String>,
        poll: Duration,
        state_path: PathBuf,
    ) -> Self {
        Self {
            node,
            profile_name,
            profile_avatar,
            script,
            poll,
            state: AnonymousState::load(&state_path),
            state_path,
        }
    }

    /// Re-authored every boot, same as `Bot::ensure_profile`: the mailbox's
    /// blob cleanup outlives its watermarks, so a once-published profile
    /// becomes unfetchable for new accounts after 7 days.
    async fn ensure_profile(&self) -> Result<()> {
        self.node
            .set_profile(Profile {
                name: self.profile_name.clone(),
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

    /// Accept incoming contact requests. `add_contact` on the accepting side
    /// also creates the direct chat space, so the whisper in [`tick`] has a
    /// chat to land in.
    async fn handle_notification(&mut self, n: dashchat_node::Notification) -> Result<()> {
        let Some(Payload::Inbox(InboxPayload::ContactRequest { code, profile })) = n.payload
        else {
            return Ok(());
        };
        let requester = hex::encode(code.agent_id.as_bytes());
        if code.agent_id == self.node.agent_id() || self.state.accepted.contains(&requester) {
            return Ok(());
        }
        info!(name = %profile.name, "accepting contact request");
        self.node
            .add_contact(code)
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        self.state.accepted.insert(requester);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    /// Whisper the script to every accepted-but-untold contact. Separate from
    /// acceptance so a failed send is retried next tick (and after restarts).
    async fn tick(&mut self) -> Result<()> {
        let pending: Vec<String> = self
            .state
            .accepted
            .difference(&self.state.told)
            .cloned()
            .collect();
        for requester in pending {
            let bytes: [u8; 32] = hex::decode(&requester)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("state agent id is not 32 bytes"))?;
            let agent = AgentId::from_bytes(&bytes)?;
            let chat = self.node.direct_chat_topic(agent);
            info!(to = %requester, "whispering the script");
            for message in &self.script {
                self.node
                    .send_message(
                        chat,
                        dashchat_node::ChatMessageContent::text_only(message.clone()),
                    )
                    .await?;
            }
            self.state.told.insert(requester);
            self.state.save(&self.state_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> AnonymousSpec {
        toml::from_str(
            r#"
            name = "Anonymous"
            reveal = ["the mayor lies", "tap the head", "the code is x"]
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
        s.reveal.clear();
        assert!(s.lint().is_err());
        let mut s = spec();
        s.reveal.push("  ".into());
        assert!(s.lint().is_err());
    }

    #[test]
    fn shipped_spec_tells_the_whole_secret() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../anonymous.toml");
        let s = AnonymousSpec::load(path).unwrap();
        // Every station tells everything: the five-tap trick AND the
        // password the hidden prompt expects.
        assert!(s.reveal.concat().contains("FIVE"));
        assert!(s.reveal.concat().contains("ahawegotyou"));
    }
}
