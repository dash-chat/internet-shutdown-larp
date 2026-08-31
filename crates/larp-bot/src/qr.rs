use std::str::FromStr as _;

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64;
use dashchat_node::{AddContactQrCode, DeviceId};

/// Encode a contact QR string exactly as the app does.
///
/// dashchat-node's `Display for AddContactQrCode` (crates/dashchat-node/src/
/// contact.rs) is `base64url_nopad( cbor((device_pubkey, inbox_nonce, profile_name)) )`
/// — a 3-element array of two byte strings and a text string. `InboxNonce` and
/// the code's constructor live in a private module, so the tuple is built here
/// by hand rather than through `AddContactQrCode::new`; [`decode_contact_code`]
/// runs the real upstream parser over the result, and `larp-bot qr` re-encodes
/// what it parsed and compares, so a format drift can never reach paper.
pub fn encode_contact_code(
    device_pubkey: &DeviceId,
    inbox_nonce: &[u8; 8],
    profile_name: &str,
) -> Result<String> {
    let value = ciborium::Value::Array(vec![
        ciborium::Value::Bytes(device_pubkey.as_bytes().to_vec()),
        ciborium::Value::Bytes(inbox_nonce.to_vec()),
        ciborium::Value::Text(profile_name.to_string()),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&value, &mut buf).context("CBOR-encoding contact code")?;
    Ok(BASE64.encode(&buf))
}

/// Decode a contact QR string with dashchat-node's own parser — the inverse of
/// [`encode_contact_code`]; used by tests and by `larp-bot qr`.
pub fn decode_contact_code(code: &str) -> Result<AddContactQrCode> {
    AddContactQrCode::from_str(code).context("decoding contact code")
}

/// Render the QR string to a PNG (for the printed wall posters).
///
/// The background (light modules and the surrounding quiet zone) is left fully
/// transparent so the poster's own background shows through; only the dark
/// modules are painted opaque black. Uses grayscale+alpha (`LumaA`) to keep the
/// file small.
pub fn render_png(code: &str, path: &std::path::Path, module_px: u32) -> Result<()> {
    let qr = qrcode::QrCode::new(code.as_bytes()).context("building QR code")?;
    let image = qr
        .render::<image::LumaA<u8>>()
        .min_dimensions(qr.width() as u32 * module_px, qr.width() as u32 * module_px)
        .dark_color(image::LumaA([0, 255])) // opaque black
        .light_color(image::LumaA([0, 0])) // transparent
        .build();
    image
        .save(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityBundle;

    /// Our encoder must agree with upstream's parser *and* its encoder: decode
    /// with `AddContactQrCode::from_str`, then re-encode via its `Display` and
    /// compare byte-for-byte.
    #[test]
    fn contact_code_roundtrips_through_the_upstream_parser() {
        let bundle = IdentityBundle::generate("sister");
        let code = bundle.contact_code().unwrap();
        let decoded = decode_contact_code(&code).unwrap();
        assert_eq!(decoded.device_pubkey, bundle.device_id().unwrap());
        assert_eq!(decoded.profile_name, bundle.qr_profile_name());
        assert_eq!(decoded.to_string(), code);
    }

    /// The nonce in the code must reconstruct the inbox topic the bot
    /// registers, or contact requests land on a topic nobody listens to.
    #[test]
    fn decoded_nonce_matches_the_registered_inbox_topic() {
        let bundle = IdentityBundle::generate("grandpa");
        let decoded = decode_contact_code(&bundle.contact_code().unwrap()).unwrap();
        let mut hasher = blake3::Hasher::new();
        hasher.update(decoded.device_pubkey.as_bytes());
        hasher.update(&decoded.inbox_nonce.as_bytes());
        assert_eq!(
            *hasher.finalize().as_bytes(),
            bundle.inbox_topic_bytes().unwrap()
        );
    }
}
