//! The character bot: one process per character, talking to each player in
//! their own **direct chat**.
//!
//! There is no group chat any more. A player scans the character's QR poster,
//! the bot accepts the contact request (which creates the direct chat), greets
//! them, and starts dropping missions into that chat. Each mission names
//! another character in its prose; the player copies the message and pastes it
//! into *that* character's chat, whose bot recognizes the text and answers with
//! the mission's success line. Pasting a mission at the wrong character earns a
//! "this message is not for me!" nudge.
//!
//! A landed delivery also earns the courier their next job on the spot: the
//! receiving character immediately fires a fresh mission of its own into the
//! chat, preferring one that doesn't send the player straight back to the
//! character whose message they just delivered.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
#[allow(deprecated)]
use dashchat_node::FakeAgentId;
use dashchat_node::{
    AsBody as _, ChatId, ChatPayload, DeviceId, InboxPayload, Node, NodeConfig, Payload, Profile,
    TopicId, stores::LocalStore,
};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::cast::ResolvedCast;
use crate::config::{BotConfig, Timing};
use crate::identity::IdentityBundle;
use crate::scenario::Scenarios;

/// Persistent bot state (`state.json` in the data dir). A cache like the rest
/// of the data dir: wiping it loses greeted/answered bookkeeping but never the
/// identity (which lives in the flashed bundle).
///
/// Every field is `#[serde(default)]` so a state file written by an older
/// build still loads as far as its fields still make sense.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BotState {
    /// Direct chats already greeted (hex chat ids).
    #[serde(default)]
    pub greeted: std::collections::BTreeSet<String>,
    /// Contact requests already accepted (the requester's device id, as
    /// printed by `DeviceId`). The direct-chat topic is derived from it.
    #[serde(default)]
    pub accepted_contacts: std::collections::BTreeSet<String>,
    /// Player messages I already answered — success replies and "not for me"
    /// notices alike (hex op hashes).
    #[serde(default)]
    pub answered: std::collections::BTreeSet<String>,
    /// Mission texts already fired, per direct chat (hex chat id → texts).
    /// Each template is used at most once per player.
    #[serde(default)]
    pub fired: BTreeMap<String, Vec<String>>,
    /// Direct chats already given the informant's contact (hex chat ids).
    /// At most one tip per player: after it, repeats are noise.
    #[serde(default)]
    pub tipped: std::collections::BTreeSet<String>,
    /// Direct chats already told the mayor has fallen (hex chat ids) — the
    /// eruption happens once per player.
    #[serde(default)]
    pub fallen_announced: std::collections::BTreeSet<String>,
}

impl BotState {
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

/// How long a human would plausibly take to type `text`: one second for
/// every four words. Awaited before every message a bot sends — an instant
/// reply reads as a machine, a beat of "typing" reads as a person. Shared
/// with the spec bots (the mayor, the informant).
pub(crate) async fn typing_pause(text: &str) {
    let words = text.split_whitespace().count() as u64;
    tokio::time::sleep(Duration::from_millis(words * 1000 / 4)).await;
}

/// Overwrite the freshly-migrated local store's identity with the flashed
/// bundle, and register the bundle's inbox topic as active. Idempotent; runs
/// before the Node ever reads its keys. `Node::init`'s startup path then
/// re-subscribes the inbox topic from the store, so no private API is needed.
pub async fn seed_identity(data_dir: &Path, bundle: &IdentityBundle) -> Result<()> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    // Same filename Node::new derives via its (private) Filesystem type.
    let store_path = data_dir.join("localdata.db");

    // Run the store's own migrations (also mints a throwaway identity on
    // first boot, which the UPDATE below replaces). The store now takes the
    // pool rather than a path, so the same pool carries the raw UPDATEs.
    let pool = dashchat_node::stores::create_sqlite_pool(&store_path)
        .await
        .context("opening local store for identity seeding")?;
    let store = LocalStore::new(pool.clone()).await?;

    let seed = hex::decode(&bundle.device_private_key)?;
    sqlx::query("UPDATE identity SET value = ? WHERE key = 'private_key'")
        .bind(seed)
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE identity SET value = ? WHERE key = 'agent_id'")
        .bind(bundle.agent_id_bytes()?.to_vec())
        .execute(&pool)
        .await?;
    // Schema: active_inboxes(topic_id BLOB PK, expires_at_nanos INTEGER,
    // role INTEGER DEFAULT 0, expected_ack_author BLOB NULL). `role` defaults
    // to 0 = Advertised, which is exactly the inbox the printed QR points at.
    let nanos = bundle
        .inbox_expires_at
        .timestamp_nanos_opt()
        .unwrap_or(i64::MAX);
    sqlx::query("INSERT OR REPLACE INTO active_inboxes (topic_id, expires_at_nanos) VALUES (?, ?)")
        .bind(bundle.inbox_topic_bytes()?.to_vec())
        .bind(nanos)
        .execute(&pool)
        .await?;

    // Sanity: the store must now report the flashed identity.
    let keys = store.node_keys().await?;
    let ok = keys.private_key.as_bytes() == bundle.signing_key()?.as_bytes()
        && keys.agent_id == bundle.agent_id()?;
    store.close().await;
    pool.close().await;
    anyhow::ensure!(ok, "identity seeding failed: store keys don't match the bundle");
    Ok(())
}

/// Seed the identity and start the node. The caller picks the `NodeConfig`
/// (production: [`bot_node_config`]; tests: `NodeConfig::testing()`-based)
/// and registers a mailbox afterwards.
pub async fn build_node(
    data_dir: &Path,
    bundle: &IdentityBundle,
    config: NodeConfig,
) -> Result<(Node, mpsc::Receiver<dashchat_node::Notification>)> {
    seed_identity(data_dir, bundle).await?;
    let (notification_tx, notification_rx) = mpsc::channel(1024);
    let node = Node::new(
        data_dir.to_path_buf(),
        config,
        Some(notification_tx),
        None,
    )
    .await?;
    Ok((node, notification_rx))
}

/// Node config for a bot: the app's defaults, with a far-out contact-code
/// expiry.
///
/// 0.19 added real networking knobs (`mdns_mode`, `use_relay`, `enable_p2p`,
/// `enable_blob_sync`) that 0.18.9 didn't have. They are deliberately left at
/// their defaults — the same ones the players' app runs with — rather than
/// tuned for the offline map. `use_relay` in particular is a no-op on an
/// isolated station: the relay is simply unreachable.
pub fn bot_node_config() -> NodeConfig {
    let mut config = NodeConfig::default();
    // Runtime QR minting isn't used (the QR comes from the bundle), but keep
    // any incidental inbox registration far-lived anyway.
    config.contact_code_expiry = chrono::Duration::days(365 * 5);
    config
}

/// Register the configured mailbox on the node.
///
/// Unlike v0.18.9 — where the MailboxId was a client-side string and
/// registration was offline — the id is now the server's own EndpointId, read
/// from its `/health` endpoint, and its dialing address has to go into the
/// p2panda address book before blobs can be fetched. That makes registration a
/// network operation, so it retries: on a station Pi this runs while the Pi's
/// own mailbox is still booting. The bot has nothing to do until it succeeds.
pub(crate) async fn register_mailbox(node: &Node, url: &str) {
    let health = loop {
        match dashchat_node::mailbox::fetch_mailbox_health(url).await {
            Ok(health) => break health,
            Err(err) => {
                warn!(%url, ?err, "mailbox /health unreachable, retrying");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    };

    if let Err(err) = node.insert_peer_addr(health.endpoint_addr.clone()).await {
        warn!(%url, ?err, "could not add the mailbox to the address book");
    }
    let client = mailbox_client::toy::ToyMailboxClient::<
        dashchat_node::mailbox::MailboxOperation,
    >::new(
        health.mailbox_id,
        url.to_string(),
        node.endpoint_id(),
        node.unfetched_blob_tracker(),
    )
    .with_blob_reader(node.blob_reader());
    node.mailboxes.register(client).await;

    // Best-effort: lets the mailbox dial us back when fetching blobs we
    // publish. Text-only bots don't strictly need it, but avatars might.
    if let Err(err) = node.register_with_mailbox(url).await {
        warn!(%url, ?err, "could not register our address with the mailbox");
    }
    info!(%url, "mailbox registered");
}

/// Every direct chat a bot has with a player.
///
/// Direct chats are not group chats — `Node::get_groups` never returns them —
/// so they are derived from contacts. Two sources, unioned, so neither kind of
/// state loss is fatal: the accepted-contacts set (what this bot answered
/// itself, hex device ids from its `state.json`) and the node's own contact
/// projection (which survives a `state.json` wipe). A candidate only counts
/// once its chat topic is actually subscribed — that is the node's own record
/// of `create_direct_chat_space` having run, and it filters out contact
/// requests that were never accepted.
///
/// Shared by the character bot and the spec bots (the informant, the mayor).
pub(crate) async fn direct_chats(
    node: &Node,
    me: DeviceId,
    accepted: &BTreeSet<String>,
) -> Result<Vec<(DeviceId, ChatId)>> {
    let mut devices: BTreeSet<DeviceId> = BTreeSet::new();
    for recorded in accepted {
        match DeviceId::from_str(recorded) {
            Ok(device) => {
                devices.insert(device);
            }
            Err(err) => warn!(%recorded, ?err, "unparseable device id in state.json"),
        }
    }
    for agent in node.projection.all_contact_agent_ids().await? {
        for device in node.projection.lookup_devices_by_agent_id(agent).await? {
            devices.insert(device);
        }
    }
    devices.remove(&me);

    let subscribed = node.subscribed_topics().await?;
    Ok(devices
        .into_iter()
        .map(|device| {
            #[allow(deprecated)] // FakeAgentId is what direct_chat_topic takes today
            let chat = node.direct_chat_topic(FakeAgentId::from(device));
            (device, chat)
        })
        .filter(|(_, chat)| subscribed.contains(&TopicId::from(*chat)))
        .collect())
}

/// Every chat message in a direct chat, oldest first, as
/// `(author device, op hash, text)`.
///
/// dashchat-node has `OpStore::get_interleaved_logs`, but 0.19 gates it behind
/// the `testing` feature ("only used for testing and should stay that way"),
/// so the same walk over the public log API is done here. Ops with an
/// undecodable body are skipped rather than failing the whole scan. The op
/// hash is what both callers dedup their replies on.
pub(crate) async fn chat_messages(
    node: &Node,
    chat: ChatId,
) -> Result<Vec<(DeviceId, String, String)>> {
    let log_id = chat.into();
    let mut ops = Vec::new();
    for author in node.op_store.get_authors(log_id).await? {
        for op in node.op_store.get_log(&author, &log_id, None).await? {
            let Some(body) = op.body else { continue };
            let Ok(payload) = Payload::try_from_body(&body) else {
                warn!(hash = ?op.header.hash(), "undecodable op payload, skipping");
                continue;
            };
            ops.push((op.header, payload));
        }
    }
    ops.sort_by_key(|(header, _)| header.timestamp);
    // The offline stations' NTP substitute (see crate::clock): the phones
    // know the time and stamp every op with it, so the newest op in the chat
    // pulls a lagging station clock forward. Must at least be considered on
    // every scan — the clock matters most for stamping the very replies this
    // scan is about to produce.
    if let Some((header, _)) = ops.last() {
        crate::clock::step_clock_forward(u64::from(header.timestamp));
    }
    Ok(ops
        .into_iter()
        .filter_map(|(header, payload)| {
            let Payload::Chat(ChatPayload::Message(content)) = payload else {
                return None;
            };
            Some((
                DeviceId::from(header.verifying_key),
                hex::encode(header.hash().as_bytes()),
                content.message().to_string(),
            ))
        })
        .collect())
}

/// How a character hands out the informant's contact, once a delivery lands.
///
/// The informant has no QR poster: this is the only way a player meets him.
/// His identity is flashed onto the tipping character's card alone (Mira's —
/// characters.just), where it does double duty: it arms the informant
/// service, and this bot reads its public half to build the add-contact deep
/// link. Tapping the link only gets an answer inside that station's wifi,
/// which is why her tip says to do it there.
#[derive(Clone, Debug)]
pub struct InformantTip {
    /// `https://dashchat.org/add-contact/<code>`.
    pub link: String,
}

impl InformantTip {
    /// Build the tip from the flashed informant bundle. `None` (with a log
    /// line) when the card carries none.
    pub fn from_identity_file(path: &Path) -> Option<Self> {
        match IdentityBundle::load(path).and_then(|b| b.contact_code()) {
            Ok(code) => Some(Self {
                link: crate::qr::contact_deep_link(&code),
            }),
            Err(err) => {
                warn!(path = %path.display(), ?err, "no informant identity, tips disabled");
                None
            }
        }
    }
}

pub struct Bot {
    node: Node,
    bundle: IdentityBundle,
    cast: ResolvedCast,
    scenarios: Scenarios,
    timing: Timing,
    /// `None` on a card without the informant, or for a pack with no tip line.
    informant: Option<InformantTip>,
    /// The flag the mayor's spec bot touches when his trigger fires
    /// (`triggered` in his data dir). Polled each tick; only ever exists on
    /// the base station, where his bot and this one share the Pi.
    mayor_fallen_flag: Option<PathBuf>,
    state: BotState,
    state_path: PathBuf,
    /// Per-chat next mission fire time (in-memory; reseeded on restart).
    next_fire: BTreeMap<String, Instant>,
    /// Player-message op hashes already seen, per chat (in-memory; only
    /// tracked when the pack has a comeback line). The first scan of a chat
    /// baselines its history, so restarts never trigger the comeback.
    seen_player_ops: BTreeMap<String, BTreeSet<String>>,
    /// When each chat last produced a NEW player message (or was baselined).
    last_player_msg: BTreeMap<String, Instant>,
}

/// Run the bot daemon: seed identity, start the node, register the mailbox,
/// then loop forever (notification handling + direct-chat polling + scheduling).
pub async fn run(config: BotConfig) -> Result<()> {
    let bundle = IdentityBundle::load(&config.identity)?;
    let cast = crate::cast::Cast::load(&config.cast)?.resolve()?;
    let scenarios = Scenarios::load_dir(&config.scenarios_dir)?;

    let (node, notification_rx) =
        build_node(&config.data_dir, &bundle, bot_node_config()).await?;
    info!(
        character = %bundle.character,
        device_id = %hex::encode(bundle.device_id()?.as_bytes()),
        "node up"
    );

    register_mailbox(&node, &config.mailbox_url).await;

    let informant = config
        .anonymous_identity
        .as_deref()
        .and_then(InformantTip::from_identity_file);

    let state_path = config.data_dir.join("state.json");
    Bot::new(
        node,
        bundle,
        cast,
        scenarios,
        config.timing,
        informant,
        config.mayor_fallen_flag.clone(),
        state_path,
    )?
    .run_loop(notification_rx)
    .await
}

impl Bot {
    pub fn new(
        node: Node,
        bundle: IdentityBundle,
        cast: ResolvedCast,
        scenarios: Scenarios,
        timing: Timing,
        informant: Option<InformantTip>,
        mayor_fallen_flag: Option<PathBuf>,
        state_path: PathBuf,
    ) -> Result<Self> {
        anyhow::ensure!(
            scenarios.pack(&bundle.character).is_some(),
            "no scenario pack for character {:?}",
            bundle.character
        );
        // The printed QR posters embed the bundle's name; the profile the app
        // shows comes from the pack. They must agree, or the poster greets the
        // player with a different name than the chat that follows.
        let pack_name = &scenarios.pack(&bundle.character).unwrap().name;
        anyhow::ensure!(
            bundle.qr_profile_name() == *pack_name,
            "identity bundle name {:?} != scenario profile name {:?} for {:?} — \
             set profile_name in secrets/{}-identity.toml and reprint the QR poster",
            bundle.qr_profile_name(),
            pack_name,
            bundle.character,
            bundle.character,
        );
        Ok(Self {
            node,
            bundle,
            cast,
            scenarios,
            timing,
            informant,
            mayor_fallen_flag,
            state: BotState::load(&state_path),
            state_path,
            next_fire: BTreeMap::new(),
            seen_player_ops: BTreeMap::new(),
            last_player_msg: BTreeMap::new(),
        })
    }

    /// Announce the character's profile on every boot. Must happen before
    /// accepting contacts: `add_contact`'s reply requires a profile.
    ///
    /// Re-authored unconditionally, not just when unset: the mailbox server's
    /// cleanup deletes blobs older than 7 days but keeps its watermarks, so a
    /// SetProfile op published once is eventually deleted yet never reported
    /// missing again — new accounts would sync an empty announcements topic
    /// and see no profile. A fresh op every boot lands above the watermark
    /// and gets pushed anew.
    async fn ensure_profile(&self) -> Result<()> {
        let pack = self
            .scenarios
            .pack(&self.bundle.character)
            .expect("checked in new()");
        self.node
            .set_profile(Profile {
                name: pack.name.clone(),
                surname: None,
                avatar: pack.avatar.clone(),
                about: None,
            })
            .await?;
        Ok(())
    }

    pub async fn run_loop(mut self, mut notifications: mpsc::Receiver<dashchat_node::Notification>) -> Result<()> {
        self.ensure_profile().await?;
        let poll = Duration::from_secs(self.timing.poll_interval_secs.max(1));
        let mut tick = tokio::time::interval(poll);
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

    /// Auto-accept incoming contact requests (the acceptance half of the
    /// QR-poster onboarding flow).
    ///
    /// The request no longer carries the scanner's QR code: it carries their
    /// agent id (already mapped to the op author by the node) plus the private
    /// reply topic to acknowledge on. `accept_contact` does the rest — network
    /// establishment, the profile reply, and the direct-chat space the whole
    /// game now runs in.
    ///
    /// What gets recorded is the requester's **device** id (the op author),
    /// not their agent id: that is what [`Node::direct_chat_topic`] derives the
    /// chat from.
    async fn handle_notification(&mut self, n: dashchat_node::Notification) -> Result<()> {
        let Some(op) = n.op() else { return Ok(()) };
        // The very first thing a player ever sends is the contact request
        // their QR scan fires, and it arrives before the bot says a word —
        // stepping the clock HERE (see crate::clock) means even the greeting,
        // the first message the player reads, is stamped with real time.
        crate::clock::step_clock_forward(u64::from(op.header.timestamp));
        let Some(Payload::Inbox(InboxPayload::ContactRequest {
            agent_id, profile, ..
        })) = &op.payload
        else {
            return Ok(());
        };
        let device = DeviceId::from(op.header.verifying_key);
        let requester = device.to_string();
        if *agent_id == self.bundle.agent_id()?
            || self.state.accepted_contacts.contains(&requester)
        {
            return Ok(());
        }
        info!(name = %profile.name, "accepting contact request");
        self.node
            .accept_contact(*agent_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        self.state.accepted_contacts.insert(requester);
        self.state.save(&self.state_path)?;
        Ok(())
    }

    async fn tick(&mut self) -> Result<()> {
        // Has the mayor come apart, and at whose hands? Checked once per
        // tick: his spec bot shares this Pi on the base station and writes
        // the flag the moment a trigger fires — one triggering player's
        // device id per line. Everywhere else the file simply never appears.
        // An empty file names nobody and announces to nobody: a stray
        // `touch` (or a pre-id-format leftover) must not congratulate the
        // whole town, which is exactly what one did on 2026-09-02.
        let mayor_felled_by: Option<BTreeSet<String>> = self
            .mayor_fallen_flag
            .as_deref()
            .and_then(|flag| std::fs::read_to_string(flag).ok())
            .map(|raw| raw.split_whitespace().map(str::to_string).collect());

        let me = self.bundle.device_id()?;
        for (device, chat) in direct_chats(&self.node, me, &self.state.accepted_contacts).await? {
            let key = chat.to_string();

            // Greet a player the first time we see their chat.
            if !self.state.greeted.contains(&key) {
                let greeting = self
                    .scenarios
                    .pack(&self.bundle.character)
                    .expect("checked at startup")
                    .greeting
                    .clone();
                info!(player = %device, "greeting a new player");
                typing_pause(&greeting).await;
                self.node.send_message(chat, greeting, None, None).await?;
                self.state.greeted.insert(key.clone());
                self.state.save(&self.state_path)?;
                // The player's first mission follows the welcome closely.
                self.next_fire.insert(
                    key.clone(),
                    Instant::now() + Duration::from_secs(self.timing.first_mission_delay_secs),
                );
            }

            if let Some(felled_by) = &mayor_felled_by {
                // The eruption goes to the player who felled him — the payoff
                // for delivering the sentence — not to every open chat.
                if felled_by.contains(&device.to_string()) {
                    self.maybe_announce_fallen(chat, &key).await?;
                }
            }
            self.process_chat_messages(chat, &key).await?;
            self.maybe_fire_mission(chat, &key).await?;
        }
        Ok(())
    }

    /// Erupt: the mayor has fallen and this character saw it happen. Sent
    /// unprompted, once per chat, the tick the flag appears — but only into
    /// the chats of the players the flag names as having felled him (the
    /// caller checks): the collapse is the courier's payoff, not broadcast
    /// news. Silent for packs with no `mayor_fallen` line (everyone but
    /// Nadia, who shares his Pi).
    async fn maybe_announce_fallen(&mut self, chat: ChatId, key: &str) -> Result<()> {
        if self.state.fallen_announced.contains(key) {
            return Ok(());
        }
        let Some(line) = self
            .scenarios
            .pack(&self.bundle.character)
            .expect("checked at startup")
            .mayor_fallen
            .clone()
        else {
            return Ok(());
        };
        info!(chat = %key, "the mayor has fallen — announcing");
        typing_pause(&line).await;
        self.node.send_message(chat, line, None, None).await?;
        self.state.fallen_announced.insert(key.to_string());
        self.state.save(&self.state_path)?;
        Ok(())
    }

    /// Scan a direct chat's messages and react to what the player pasted in:
    /// a mission addressed to this character gets its success line, a mission
    /// addressed to somebody else gets a "not for me" nudge.
    ///
    /// Everything a player pastes is authored by *them*, so recognition is
    /// text-based (forgiving — see [`Scenarios::mission_in_pasted_text`])
    /// rather than author-based. Dedup is by operation hash, so re-scans and
    /// restarts are harmless.
    async fn process_chat_messages(
        &mut self,
        chat: ChatId,
        key: &str,
    ) -> Result<()> {
        let my_device = self.bundle.device_id()?;
        let messages = chat_messages(&self.node, chat).await?;

        let comeback = self
            .scenarios
            .pack(&self.bundle.character)
            .expect("checked at startup")
            .comeback
            .clone();
        let baseline_scan = comeback.is_some() && !self.seen_player_ops.contains_key(key);
        if comeback.is_some() {
            self.seen_player_ops.entry(key.to_string()).or_default();
            self.last_player_msg
                .entry(key.to_string())
                .or_insert_with(Instant::now);
        }
        let mut comeback_due = false;

        // What to say, and which player op each line answers (the comeback
        // answers no single op). That op is two things at once: the dedup key,
        // marked only once the reply is actually out so a failed send is
        // retried next tick, and the message the answer is THREADED onto — an
        // ack quotes the delivery it acks, the way a person answering three
        // pasted messages would.
        let mut replies: Vec<(Option<String>, String)> = Vec::new();
        // Characters whose missions were delivered here in this scan. Any
        // entry means a delivery was acked — the moment the informant tip may
        // follow, and the trigger for the follow-up mission (which prefers
        // destinations outside this set).
        let mut delivered_from: BTreeSet<String> = BTreeSet::new();
        for (author, op_hash, text) in messages {
            if author == my_device {
                continue;
            }
            // A direct chat only ever holds this bot and one player, but the
            // cast is still consulted so that a message from another character
            // — however it got here — is never mistaken for a delivery.
            if self.cast.character_of_device(&author).is_some() {
                continue;
            }

            // The player wrote something. If this character has a comeback
            // line, their first message after a quiet spell gets it.
            if let Some(cb) = &comeback {
                let seen = self.seen_player_ops.entry(key.to_string()).or_default();
                if seen.insert(op_hash.clone()) {
                    let quiet = self
                        .last_player_msg
                        .get(key)
                        .is_some_and(|t| t.elapsed() >= Duration::from_secs(cb.after_secs));
                    if quiet && !baseline_scan {
                        comeback_due = true;
                    }
                    self.last_player_msg.insert(key.to_string(), Instant::now());
                }
            }

            // Did they paste a mission in? Answer it exactly once.
            let Some((owner, mission)) = self.scenarios.mission_in_pasted_text(&text) else {
                continue;
            };
            if self.state.answered.contains(&op_hash) {
                continue;
            }
            let reply = if mission.to == self.bundle.character {
                info!(from = %owner, "delivery received, acking");
                delivered_from.insert(owner.to_string());
                Some(mission.success.clone())
            } else {
                // Wrong station. Turn them away without naming the right one:
                // the message they are carrying already says who it is for.
                let to = mission.to.clone();
                match self.scenarios.misdelivery_notice(&self.bundle.character) {
                    Some(notice) => {
                        info!(%to, "message for another character, turning it away");
                        Some(notice.to_string())
                    }
                    None => {
                        info!(%to, "message for another character, but no misdelivered line");
                        None
                    }
                }
            };
            if let Some(reply) = reply {
                replies.push((Some(op_hash), reply));
            }
        }
        if comeback_due {
            let cb = comeback.expect("comeback_due implies a comeback line");
            info!(chat = %key, "player message after a quiet spell, greeting");
            replies.insert(0, (None, cb.text));
        }
        let mut dirty = false;
        for (answers, reply) in replies {
            // `Node::send_message` validates the reply target against the
            // chat's ops and refuses the send if it doesn't like it. An ack
            // that never goes out would strand the delivery, so a rejected
            // thread falls back to a plain message. (The hex op hash parses
            // straight into the p2panda `Hash` the node wants.)
            let target = answers.as_ref().and_then(|hash| hash.parse().ok());
            typing_pause(&reply).await;
            let sent = if target.is_some() {
                match self
                    .node
                    .send_message(chat, reply.clone(), None, target)
                    .await
                {
                    Ok(_) => true,
                    Err(err) => {
                        warn!(?err, "could not thread the answer as a reply, sending plain");
                        false
                    }
                }
            } else {
                false
            };
            if !sent {
                self.node.send_message(chat, reply, None, None).await?;
            }
            if let Some(op_hash) = answers {
                self.state.answered.insert(op_hash);
                dirty = true;
            }
        }
        let tipped_now = !delivered_from.is_empty() && self.maybe_tip_informant(chat, key).await?;
        if tipped_now {
            dirty = true;
        }
        if dirty {
            self.state.save(&self.state_path)?;
        }
        // When the tip just went out, it IS the follow-up: handing the player
        // the informant's contact opens the side plot, and stacking a regular
        // mission on top would bury it. Only Mira ever tips, once per player —
        // her later deliveries earn ordinary follow-ups again.
        if !delivered_from.is_empty() && !tipped_now {
            self.fire_followup_mission(chat, key, &delivered_from).await?;
        }
        Ok(())
    }

    /// A delivery just landed: hand the courier their next job on the spot.
    /// The draw prefers missions not addressed to the characters whose
    /// messages they just delivered (steering them somewhere new rather than
    /// straight back), falling back to any unused template when those are all
    /// that's left. Resets the chat's timer so the background drip doesn't
    /// pile a second mission right on top. Silent once the pack is exhausted.
    async fn fire_followup_mission(
        &mut self,
        chat: ChatId,
        key: &str,
        avoid: &BTreeSet<String>,
    ) -> Result<()> {
        let Some(mission) = self.pick_mission(key, avoid) else {
            return Ok(());
        };
        info!(to = %mission.to, chat = %key, "delivery landed, firing a follow-up mission");
        typing_pause(&mission.text).await;
        self.node
            .send_message(chat, mission.text.clone(), None, None)
            .await?;
        self.state
            .fired
            .entry(key.to_string())
            .or_default()
            .push(mission.text);
        self.state.save(&self.state_path)?;
        let next = self.draw_next_fire();
        self.next_fire.insert(key.to_string(), next);
        Ok(())
    }

    /// Hand the player the informant's contact.
    ///
    /// Called right after a delivery is acked, and deterministic: carrying a
    /// message to a character who has a tip line always earns it. The line
    /// goes out with the informant's add-contact deep link substituted in,
    /// and the chat is marked so it never happens twice. Returns whether the
    /// state changed.
    ///
    /// Silent when the card has no informant identity or the pack has no tip
    /// line — as shipped, every character but Mira is in the second case.
    async fn maybe_tip_informant(&mut self, chat: ChatId, key: &str) -> Result<bool> {
        if self.state.tipped.contains(key) {
            return Ok(false);
        }
        let Some(informant) = self.informant.clone() else {
            return Ok(false);
        };
        let Some(tip) = self
            .scenarios
            .pack(&self.bundle.character)
            .expect("checked at startup")
            .informant_tip_message(&informant.link)
        else {
            return Ok(false);
        };
        info!(chat = %key, "passing the informant's contact to a player");
        typing_pause(&tip).await;
        self.node.send_message(chat, tip, None, None).await?;
        self.state.tipped.insert(key.to_string());
        Ok(true)
    }

    /// Drop the next mission into a player's chat when its timer comes due —
    /// the background drip behind [`Bot::fire_followup_mission`]'s
    /// delivery-triggered fires.
    ///
    /// Nothing gates this on delivery: the courier walks one way, so this
    /// character never learns whether its last message reached its
    /// destination. What bounds the flow instead is the pack — each template
    /// is used at most once per player, and the character falls silent (bar
    /// acks and comebacks) once it has handed out everything it has.
    async fn maybe_fire_mission(
        &mut self,
        chat: ChatId,
        key: &str,
    ) -> Result<()> {
        let fired_before = self.state.fired.get(key).is_some_and(|v| !v.is_empty());
        let due = self
            .next_fire
            .entry(key.to_string())
            .or_insert_with(|| {
                // Restart: a player who never got a mission keeps the short
                // post-welcome delay; otherwise draw a fresh interval rather
                // than firing instantly.
                if fired_before {
                    Instant::now() + rand_interval(&self.timing)
                } else {
                    Instant::now() + Duration::from_secs(self.timing.first_mission_delay_secs)
                }
            });
        if Instant::now() < *due {
            return Ok(());
        }
        let Some(mission) = self.pick_mission(key, &BTreeSet::new()) else {
            // Pack exhausted for this player: re-check on the next interval
            // instead of every tick.
            let next = self.draw_next_fire();
            self.next_fire.insert(key.to_string(), next);
            return Ok(());
        };
        info!(to = %mission.to, chat = %key, "firing mission");
        typing_pause(&mission.text).await;
        self.node
            .send_message(chat, mission.text.clone(), None, None)
            .await?;
        self.state
            .fired
            .entry(key.to_string())
            .or_default()
            .push(mission.text);
        self.state.save(&self.state_path)?;
        let next = self.draw_next_fire();
        self.next_fire.insert(key.to_string(), next);
        Ok(())
    }

    /// A template this player has not been given yet, preferring missions not
    /// addressed to anyone in `avoid` — see
    /// [`crate::scenario::Pack::pick_unused_mission`] for the draw's rules.
    fn pick_mission(
        &self,
        chat: &str,
        avoid: &BTreeSet<String>,
    ) -> Option<crate::scenario::Mission> {
        let pack = self.scenarios.pack(&self.bundle.character)?;
        let fired_texts: Vec<&str> = self
            .state
            .fired
            .get(chat)
            .map(|v| v.iter().map(|t| t.as_str()).collect())
            .unwrap_or_default();
        pack.pick_unused_mission(&fired_texts, avoid).cloned()
    }

    fn draw_next_fire(&self) -> Instant {
        Instant::now() + rand_interval(&self.timing)
    }
}

fn rand_interval(timing: &Timing) -> Duration {
    let secs =
        rand::thread_rng().gen_range(timing.min_interval_secs..=timing.max_interval_secs.max(timing.min_interval_secs));
    Duration::from_secs(secs)
}
