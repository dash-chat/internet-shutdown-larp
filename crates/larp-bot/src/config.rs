use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// `larp-bot run` configuration (`config.toml`). The character itself comes
/// from the identity bundle — this file only wires up paths and timing.
#[derive(Clone, Debug, Deserialize)]
pub struct BotConfig {
    /// The mailbox this bot syncs through: `http://127.0.0.1:<port>` on the
    /// Pis, the cloud mailbox URL on the sister's droplet.
    pub mailbox_url: String,
    /// The flashed identity bundle (survives wipes; see identity.rs).
    pub identity: PathBuf,
    /// The public cast file (all characters' agent/device ids). Deliveries
    /// are recognized by their text, so this only serves to tell character
    /// bots apart from players.
    pub cast: PathBuf,
    /// Directory of scenario packs (`<character>.toml`, all characters).
    pub scenarios_dir: PathBuf,
    /// Node data dir. A cache: safe to wipe, identity comes from the bundle.
    pub data_dir: PathBuf,
    /// The flashed anonymous informant bundle (`larp-anonymous.toml`) — the
    /// same file the informant service runs on, flashed only onto the tipping
    /// character's card (Mira's). Only its public half is used: the contact
    /// code that goes into the informant tip (see `Pack::informant_tip`).
    /// Absent or unreadable simply means no tips, which is every other card.
    #[serde(default)]
    pub anonymous_identity: Option<PathBuf>,
    /// Path the mayor's spec bot touches when his trigger fires (`triggered`
    /// in his data dir). Only meaningful where both bots share a machine —
    /// the base station, where Nadia's bot polls it to erupt with her
    /// `mayor_fallen` line the moment he comes apart. Elsewhere the file
    /// simply never appears.
    #[serde(default)]
    pub mayor_fallen_flag: Option<PathBuf>,
    #[serde(default)]
    pub timing: Timing,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Timing {
    /// Mission firing interval bounds, per player chat (uniform random draw).
    pub min_interval_secs: u64,
    pub max_interval_secs: u64,
    /// Delay between a player's welcome message and their first mission.
    pub first_mission_delay_secs: u64,
    /// How often the bot polls its direct chats for new messages.
    pub poll_interval_secs: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            min_interval_secs: 180,
            max_interval_secs: 480,
            first_mission_delay_secs: 5,
            poll_interval_secs: 3,
        }
    }
}

impl BotConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading config {}", path.as_ref().display()))?;
        let config: Self = toml::from_str(&raw).context("parsing config")?;
        anyhow::ensure!(
            config.timing.min_interval_secs <= config.timing.max_interval_secs,
            "timing: min_interval_secs > max_interval_secs"
        );
        Ok(config)
    }
}
