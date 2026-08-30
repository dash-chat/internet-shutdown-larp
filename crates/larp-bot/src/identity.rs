use std::path::Path;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use dashchat_node::{AgentId, DeviceId, SigningKey};
use serde::{Deserialize, Serialize};

/// Maximum UTF-8 length of the name carried in a contact QR, mirroring
/// dashchat-node's `PROFILE_NAME_MAX_BYTES`. Longer names are truncated by the
/// *decoder*, so anything over this would fail our poster round-trip check.
const QR_PROFILE_NAME_MAX_BYTES: usize = 16;

/// The flashable identity bundle (`larp-identity.toml`): everything that must
/// survive a data-dir wipe or image re-flash so the printed QR posters stay
/// valid. Generated offline by `larp-bot keygen`, read from the FAT boot
/// partition by `larp-bot run`.
///
/// The device key, agent id and inbox nonce are all required — none is
/// derivable from the others: the agent id is minted from a throwaway key, and
/// the inbox topic the printed QR routes contact requests to is
/// `blake3(device_pubkey ‖ inbox_nonce)` (requests for unregistered inbox
/// topics are silently dropped).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityBundle {
    /// Character key, e.g. "firefighters". Selects the scenario pack.
    pub character: String,
    /// Hex ed25519 signing-key seed (the device key).
    pub device_private_key: String,
    /// Hex 32-byte agent id.
    pub agent_id: String,
    /// Hex 8-byte inbox nonce. The inbox topic is derived from it and the
    /// device pubkey; the nonce is what the QR actually carries.
    pub inbox_nonce: String,
    /// Expiry of the registered inbox topic. Posters are printed: keep it
    /// years out. Not carried in the QR (0.19 codes have no expiry field).
    pub inbox_expires_at: DateTime<Utc>,
    /// Name shown next to the QR scan before the real profile syncs. Defaults
    /// to the character key; must fit [`QR_PROFILE_NAME_MAX_BYTES`].
    #[serde(default)]
    pub profile_name: Option<String>,
}

impl IdentityBundle {
    pub fn generate(character: &str) -> Self {
        let device_key = SigningKey::generate();
        // Upstream mints the agent id from a throwaway key's public half
        // (stores/local_store.rs); mirror that.
        let agent_id = AgentId::from(dashchat_node::ActorId::from(
            SigningKey::generate().verifying_key(),
        ));
        Self {
            character: character.to_string(),
            device_private_key: hex::encode(device_key.as_bytes()),
            agent_id: hex::encode(agent_id.as_bytes()),
            inbox_nonce: hex::encode(rand::random::<[u8; 8]>()),
            inbox_expires_at: Utc::now() + chrono::Duration::days(365 * 5),
            profile_name: None,
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading identity bundle {}", path.as_ref().display()))?;
        let bundle: Self = toml::from_str(&raw).with_context(|| {
            format!(
                "parsing identity bundle {} (pre-0.19 bundles carry `inbox_topic` \
                 instead of `inbox_nonce`; regenerate them with \
                 `just characters::generate`)",
                path.as_ref().display()
            )
        })?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        std::fs::write(path.as_ref(), toml::to_string_pretty(self)?)
            .with_context(|| format!("writing identity bundle {}", path.as_ref().display()))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.signing_key()?;
        self.agent_id()?;
        self.inbox_nonce_bytes()?;
        ensure!(!self.character.is_empty(), "character must not be empty");
        let name = self.qr_profile_name();
        ensure!(
            name.len() <= QR_PROFILE_NAME_MAX_BYTES,
            "profile_name {name:?} is {} bytes; the contact QR truncates at {QR_PROFILE_NAME_MAX_BYTES}",
            name.len()
        );
        Ok(())
    }

    pub fn signing_key(&self) -> Result<SigningKey> {
        let bytes: [u8; 32] = hex::decode(&self.device_private_key)
            .context("device_private_key is not hex")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("device_private_key is not 32 bytes"))?;
        Ok(SigningKey::from_bytes(&bytes))
    }

    pub fn device_id(&self) -> Result<DeviceId> {
        Ok(DeviceId::from(self.signing_key()?.verifying_key()))
    }

    pub fn agent_id(&self) -> Result<AgentId> {
        let bytes: [u8; 32] = hex::decode(&self.agent_id)
            .context("agent_id is not hex")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("agent_id is not 32 bytes"))?;
        AgentId::from_bytes(&bytes)
    }

    pub fn agent_id_bytes(&self) -> Result<[u8; 32]> {
        Ok(*self.agent_id()?.as_bytes())
    }

    pub fn inbox_nonce_bytes(&self) -> Result<[u8; 8]> {
        hex::decode(&self.inbox_nonce)
            .context("inbox_nonce is not hex")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("inbox_nonce is not 8 bytes"))
    }

    /// The advertised inbox topic id, derived exactly as dashchat-node's
    /// (private) `derive_inbox_topic` does: `blake3(device_pubkey ‖ nonce)`.
    /// Covered by [`crate::qr`]'s round-trip test against the upstream parser.
    pub fn inbox_topic_bytes(&self) -> Result<[u8; 32]> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.device_id()?.as_bytes());
        hasher.update(&self.inbox_nonce_bytes()?);
        Ok(*hasher.finalize().as_bytes())
    }

    /// Name embedded in the printed QR. The scanner shows it on the pending
    /// chat until the bot accepts and its real profile syncs.
    pub fn qr_profile_name(&self) -> String {
        self.profile_name
            .clone()
            .unwrap_or_else(|| self.character.clone())
    }

    /// The contact QR string for this identity, exactly as the app would mint
    /// it. See [`crate::qr::encode_contact_code`].
    pub fn contact_code(&self) -> Result<String> {
        crate::qr::encode_contact_code(
            &self.device_id()?,
            &self.inbox_nonce_bytes()?,
            &self.qr_profile_name(),
        )
    }

    /// The public half, as a `cast.toml` entry.
    pub fn cast_entry(&self) -> Result<crate::cast::CastEntry> {
        Ok(crate::cast::CastEntry {
            agent_id: self.agent_id.clone(),
            device_id: hex::encode(self.device_id()?.as_bytes()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_toml_roundtrip() {
        let bundle = IdentityBundle::generate("hospital");
        let toml_str = toml::to_string_pretty(&bundle).unwrap();
        let back: IdentityBundle = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.device_private_key, bundle.device_private_key);
        assert_eq!(back.agent_id, bundle.agent_id);
        assert_eq!(back.inbox_nonce, bundle.inbox_nonce);
    }

    #[test]
    fn inbox_topic_is_derived_from_key_and_nonce() {
        let bundle = IdentityBundle::generate("firefighters");
        // Same inputs → same topic; a different nonce → a different topic.
        assert_eq!(
            bundle.inbox_topic_bytes().unwrap(),
            bundle.inbox_topic_bytes().unwrap()
        );
        let mut other = bundle.clone();
        other.inbox_nonce = hex::encode([0u8; 8]);
        assert_ne!(
            bundle.inbox_topic_bytes().unwrap(),
            other.inbox_topic_bytes().unwrap()
        );
    }

    #[test]
    fn overlong_profile_name_is_rejected() {
        let mut bundle = IdentityBundle::generate("journalist");
        bundle.profile_name = Some("Marta the journalist".into());
        assert!(bundle.validate().is_err());
        bundle.profile_name = Some("Marta".into());
        bundle.validate().unwrap();
    }
}
