use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// One mission template: prose fired by the owning character, addressed (in
/// the prose itself) to `to`, whose bot replies with `success` when the
/// message reaches its station. There is no machine-readable metadata in the
/// message text — recognition works by (signed author, exact text) lookup
/// against these packs, which every bot loads in full.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mission {
    /// Character key of the intended recipient.
    pub to: String,
    /// The mission prose, sent verbatim.
    pub text: String,
    /// The recipient's in-character success reply, sent verbatim.
    pub success: String,
}

/// One character's scenario pack (`scenarios/<character>.toml`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pack {
    /// Display name for the character's chat profile (e.g. "Bombers").
    pub name: String,
    /// Sent once per group when the bot joins it.
    pub greeting: String,
    /// Reply to the first player message after a quiet spell, if configured.
    #[serde(default)]
    pub comeback: Option<Comeback>,
    #[serde(default)]
    pub missions: Vec<Mission>,
    /// The character's chat avatar as a `data:image/png;base64,…` URI (the
    /// only image form the app renders). Not toml: `load_dir` fills it from
    /// the sibling `scenarios/<character>.png`, if present.
    #[serde(skip)]
    pub avatar: Option<String>,
}

/// After `after_secs` without any player message in a group, the character
/// answers the next player message with `text` (sent verbatim, once per
/// quiet spell).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comeback {
    pub after_secs: u64,
    pub text: String,
}

/// All packs, keyed by character. Every bot loads all of them: recognizing a
/// mission addressed to me requires knowing the *other* characters' texts.
#[derive(Clone, Debug, Default)]
pub struct Scenarios {
    pub packs: BTreeMap<String, Pack>,
}

impl Scenarios {
    /// Load every `*.toml` in the directory; the file stem is the character key.
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut packs = BTreeMap::new();
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("reading scenarios dir {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let character = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("scenario file has a non-utf8 name")?
                .to_string();
            let raw = std::fs::read_to_string(&path)?;
            let mut pack: Pack = toml::from_str(&raw)
                .with_context(|| format!("parsing scenario pack {}", path.display()))?;
            let png = path.with_extension("png");
            if png.exists() {
                pack.avatar = Some(png_data_uri(&png)?);
            }
            packs.insert(character, pack);
        }
        let scenarios = Self { packs };
        scenarios.lint()?;
        Ok(scenarios)
    }

    /// The pack invariants recognition depends on:
    /// - every `to` names a known character (and not the pack's own),
    /// - mission texts are unique across ALL packs (a text identifies exactly
    ///   one mission),
    /// - success lines are unique across ALL packs and never collide with a
    ///   mission text.
    pub fn lint(&self) -> Result<()> {
        let mut texts: BTreeSet<&str> = BTreeSet::new();
        let mut successes: BTreeSet<&str> = BTreeSet::new();
        for (character, pack) in &self.packs {
            if pack.greeting.trim().is_empty() {
                bail!("pack {character}: empty greeting");
            }
            for (i, mission) in pack.missions.iter().enumerate() {
                if mission.to == *character {
                    bail!("pack {character} mission {i}: addressed to itself");
                }
                if !self.packs.contains_key(&mission.to) {
                    bail!(
                        "pack {character} mission {i}: unknown recipient {:?}",
                        mission.to
                    );
                }
                if mission.text.trim().is_empty() || mission.success.trim().is_empty() {
                    bail!("pack {character} mission {i}: empty text or success");
                }
                if !texts.insert(&mission.text) {
                    bail!("pack {character} mission {i}: duplicate mission text");
                }
                if !successes.insert(&mission.success) {
                    bail!("pack {character} mission {i}: duplicate success line");
                }
            }
        }
        if let Some(overlap) = texts.intersection(&successes).next() {
            bail!("a success line equals a mission text: {overlap:?}");
        }
        // Comeback lines are never looked up, but they must not collide with
        // texts that are: an identical mission text or success line would be
        // misrecognized by the other bots.
        for (character, pack) in &self.packs {
            if let Some(comeback) = &pack.comeback {
                if comeback.text.trim().is_empty() {
                    bail!("pack {character}: empty comeback text");
                }
                if texts.contains(comeback.text.as_str())
                    || successes.contains(comeback.text.as_str())
                {
                    bail!("pack {character}: comeback text collides with a mission");
                }
            }
        }
        Ok(())
    }

    /// Find the mission with this exact text, authored by this character.
    pub fn mission_by_text(&self, author: &str, text: &str) -> Option<&Mission> {
        self.packs
            .get(author)?
            .missions
            .iter()
            .find(|m| m.text == text)
    }

    pub fn pack(&self, character: &str) -> Option<&Pack> {
        self.packs.get(character)
    }
}

/// Encode a PNG file as the `data:image/png;base64,…` URI the app's avatar
/// component renders. The whole image travels inside the SetProfile op, so
/// keep the files small (the app itself exports ≤300px).
pub fn png_data_uri(path: &Path) -> Result<String> {
    use base64::Engine as _;
    let bytes =
        std::fs::read(path).with_context(|| format!("reading avatar {}", path.display()))?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenarios(packs: &[(&str, Pack)]) -> Scenarios {
        Scenarios {
            packs: packs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    fn pack(missions: Vec<Mission>) -> Pack {
        Pack {
            name: "Test".into(),
            greeting: "hello".into(),
            comeback: None,
            missions,
            avatar: None,
        }
    }

    fn mission(to: &str, text: &str, success: &str) -> Mission {
        Mission {
            to: to.into(),
            text: text.into(),
            success: success.into(),
        }
    }

    #[test]
    fn lint_accepts_valid_packs() {
        let s = scenarios(&[
            ("a", pack(vec![mission("b", "t1", "s1")])),
            ("b", pack(vec![mission("a", "t2", "s2")])),
        ]);
        s.lint().unwrap();
    }

    #[test]
    fn lint_rejects_unknown_recipient() {
        let s = scenarios(&[("a", pack(vec![mission("nobody", "t", "s")]))]);
        assert!(s.lint().is_err());
    }

    #[test]
    fn lint_rejects_self_addressed() {
        let s = scenarios(&[("a", pack(vec![mission("a", "t", "s")]))]);
        assert!(s.lint().is_err());
    }

    #[test]
    fn shipped_packs_lint() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios");
        let s = Scenarios::load_dir(dir).unwrap();
        for character in ["firefighters", "hospital", "journalist", "relative"] {
            let pack = s.pack(character).expect("missing pack");
            assert!(
                pack.avatar.as_deref().is_some_and(|a| a.starts_with("data:image/png;base64,")),
                "pack {character} has no avatar (scenarios/{character}.png missing?)"
            );
        }
        // Aunt Anna answers the first player message after a quiet spell.
        assert!(s.pack("relative").unwrap().comeback.is_some());
    }

    #[test]
    fn load_dir_picks_up_sibling_avatar_png() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.toml"),
            "name = \"A\"\ngreeting = \"hi\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.toml"),
            "name = \"B\"\ngreeting = \"hi\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("a.png"), [1u8, 2, 3]).unwrap();
        let s = Scenarios::load_dir(dir.path()).unwrap();
        assert_eq!(
            s.pack("a").unwrap().avatar.as_deref(),
            Some("data:image/png;base64,AQID")
        );
        assert_eq!(s.pack("b").unwrap().avatar, None);
    }

    #[test]
    fn lint_rejects_comeback_colliding_with_a_mission() {
        let mut p = pack(vec![mission("b", "t1", "s1")]);
        p.comeback = Some(Comeback { after_secs: 60, text: "s1".into() });
        let s = scenarios(&[("a", p), ("b", pack(vec![]))]);
        assert!(s.lint().is_err());
    }

    #[test]
    fn lint_rejects_duplicate_texts_across_packs() {
        let s = scenarios(&[
            ("a", pack(vec![mission("b", "same", "s1")])),
            ("b", pack(vec![mission("a", "same", "s2")])),
        ]);
        assert!(s.lint().is_err());
    }
}
