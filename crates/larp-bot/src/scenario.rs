use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// One mission template: prose fired by the owning character into its direct
/// chat with a player, addressed (in the prose itself) to `to`. The player
/// copies it and pastes it into their chat with `to`, whose bot replies with
/// `success`. There is no machine-readable metadata in the message text —
/// recognition works by looking the pasted text up against these packs, which
/// every bot loads in full.
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
    /// Sent once per direct chat, when the character first sees a player.
    pub greeting: String,
    /// Reply to the first player message after a quiet spell, if configured.
    #[serde(default)]
    pub comeback: Option<Comeback>,
    /// Reply when a player pastes a mission that belongs to *another*
    /// character ("This message is not for me!"). Deliberately does **not**
    /// name the right recipient — the message itself says who it is for, and
    /// working that out is the game. Without this line the character stays
    /// silent on misdeliveries.
    #[serde(default)]
    pub misdelivered: Option<String>,
    /// The informant tip: every time a delivery lands here, this character
    /// passes the player the anonymous informant's contact (there is no
    /// informant poster to find — this is the only way to meet him). Not a
    /// chance: carrying something to her earns it, once per player.
    ///
    /// Exactly one character carries this line: Mira, at the shelter desk the
    /// insider wrote to. The side plot has one door, and it is behind an
    /// actual delivery.
    ///
    /// Must contain the literal `{link}`, which is replaced with the
    /// informant's add-contact deep link (see [`Pack::informant_tip_message`]).
    /// No line, or no informant identity on the card — no tip.
    #[serde(default)]
    pub informant_tip: Option<String>,
    #[serde(default)]
    pub missions: Vec<Mission>,
    /// The character's chat avatar as a `data:image/png;base64,…` URI (the
    /// only image form the app renders). Not toml: `load_dir` fills it from
    /// the sibling `scenarios/<character>.png`, if present.
    #[serde(skip)]
    pub avatar: Option<String>,
}

/// The placeholder a pack's `informant_tip` must carry, replaced with the
/// informant's add-contact deep link.
pub const INFORMANT_LINK_PLACEHOLDER: &str = "{link}";

impl Pack {
    /// The tip as it goes into the chat: the pack's line with the placeholder
    /// replaced by `link`. `None` when the pack has no tip.
    pub fn informant_tip_message(&self, link: &str) -> Option<String> {
        Some(
            self.informant_tip
                .as_ref()?
                .replace(INFORMANT_LINK_PLACEHOLDER, link),
        )
    }
}

/// After `after_secs` without any player message in a direct chat, the
/// character answers the next player message with `text` (sent verbatim, once
/// per quiet spell).
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
    ///   mission text,
    /// - a character with an `informant_tip` is the recipient of at least one
    ///   mission (the tip only fires on a delivery *to* them, so otherwise the
    ///   side plot is sealed off),
    /// - no mission text is *contained* in any other line a player might
    ///   paste (another mission, a success line, a comeback, a misdelivery
    ///   notice, an informant tip) — matching is containment-based (see
    ///   [`Scenarios::mission_in_pasted_text`]), so a nested text would make
    ///   the paste ambiguous.
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
            if let Some(misdelivered) = &pack.misdelivered {
                if misdelivered.trim().is_empty() {
                    bail!("pack {character}: empty misdelivered text");
                }
            }
            if let Some(tip) = &pack.informant_tip {
                if tip.trim().is_empty() {
                    bail!("pack {character}: empty informant_tip text");
                }
                // Without the placeholder the tip names no way to reach the
                // informant — and there is no poster to fall back on.
                if !tip.contains(INFORMANT_LINK_PLACEHOLDER) {
                    bail!(
                        "pack {character}: informant_tip has no {INFORMANT_LINK_PLACEHOLDER} \
                         placeholder — the player would get no contact link"
                    );
                }
                if texts.contains(tip.as_str()) || successes.contains(tip.as_str()) {
                    bail!("pack {character}: informant_tip collides with a mission");
                }
                // The tip only ever fires on a delivery *to* this character,
                // so a tipping character nobody is sent to seals off the side
                // plot silently.
                if !self
                    .packs
                    .values()
                    .any(|p| p.missions.iter().any(|m| m.to == *character))
                {
                    bail!(
                        "pack {character} hands out the informant, but no mission is addressed \
                         to {character} — the tip could never fire"
                    );
                }
            }
        }

        // Containment matching (a player's paste may carry extra text around
        // the mission) only stays unambiguous if no mission text hides inside
        // another line players could paste back at a bot.
        let missions: Vec<(&str, &str)> = self
            .packs
            .iter()
            .flat_map(|(character, pack)| {
                pack.missions
                    .iter()
                    .map(move |m| (character.as_str(), m.text.as_str()))
            })
            .collect();
        let others: Vec<(&str, &str)> = self
            .packs
            .iter()
            .flat_map(|(character, pack)| {
                let successes = pack.missions.iter().map(|m| m.success.as_str());
                let extras = pack
                    .comeback
                    .iter()
                    .map(|c| c.text.as_str())
                    .chain(pack.misdelivered.iter().map(|m| m.as_str()))
                    .chain(pack.informant_tip.iter().map(|t| t.as_str()));
                successes
                    .chain(extras)
                    .map(move |line| (character.as_str(), line))
            })
            .collect();
        for (character, text) in &missions {
            let needle = normalize(text);
            for (other_character, other) in missions.iter().chain(others.iter()) {
                // Identical lines are already rejected above (unique texts,
                // unique successes, no success/text overlap), so equality here
                // only ever means "this is the same line".
                if text == other {
                    continue;
                }
                if normalize(other).contains(&needle) {
                    bail!(
                        "pack {character}: mission text is contained in another line \
                         (pack {other_character}: {other:?}) — pasted deliveries would be ambiguous"
                    );
                }
            }
        }
        Ok(())
    }

    /// Find the mission a player pasted into a chat.
    ///
    /// Matching is deliberately forgiving: players copy a message on a phone
    /// and may paste it with a quote header, a "look at this:" prefix, or
    /// mangled whitespace. Both sides are normalized (whitespace collapsed,
    /// lowercased) and the pasted text only has to *contain* the mission —
    /// [`Scenarios::lint`] guarantees at most one mission can match, and the
    /// longest match wins if a lint-passing pack ever changes that.
    ///
    /// Returns the mission and the character key that owns it.
    pub fn mission_in_pasted_text(&self, pasted: &str) -> Option<(&str, &Mission)> {
        let haystack = normalize(pasted);
        if haystack.is_empty() {
            return None;
        }
        self.packs
            .iter()
            .flat_map(|(character, pack)| {
                pack.missions
                    .iter()
                    .map(move |m| (character.as_str(), m))
            })
            .filter(|(_, mission)| haystack.contains(&normalize(&mission.text)))
            .max_by_key(|(_, mission)| mission.text.len())
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

    /// `character`'s "this message is not for me!" line, or `None` when the
    /// pack has none.
    pub fn misdelivery_notice(&self, character: &str) -> Option<&str> {
        self.pack(character)?.misdelivered.as_deref()
    }
}

/// Fold away everything a phone's copy-paste can plausibly change: leading and
/// trailing space, collapsed runs of whitespace (including the newlines a
/// quote header introduces) and case.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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
            misdelivered: None,
            informant_tip: None,
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

    /// The shipped cast: the family and the neighbour (docs/design.md).
    /// Sorted, because `Scenarios` keys are a `BTreeMap`.
    const CAST: [&str; 4] = ["grandpa", "mum", "neighbour", "sister"];

    #[test]
    fn shipped_packs_lint() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios");
        let s = Scenarios::load_dir(dir).unwrap();
        assert_eq!(s.packs.keys().map(String::as_str).collect::<Vec<_>>(), CAST);
        for character in CAST {
            let pack = s.pack(character).expect("missing pack");
            assert!(
                pack.avatar.as_deref().is_some_and(|a| a.starts_with("data:image/png;base64,")),
                "pack {character} has no avatar (scenarios/{character}.png missing?)"
            );
        }
        // Mira, stuck outside town, answers the first player message after a
        // quiet spell.
        assert!(s.pack("sister").unwrap().comeback.is_some());
        // The informant has no poster any more, and exactly one character
        // hands him out: Mira, at the desk he wrote to. More than one door
        // would make the side plot a lottery; none would make it unreachable.
        for character in CAST {
            let tip = s.pack(character).unwrap().informant_tip.as_deref();
            if character == "sister" {
                let tip = tip.expect("Mira is the only way to the informant");
                assert!(tip.contains(INFORMANT_LINK_PLACEHOLDER));
            } else {
                assert!(
                    tip.is_none(),
                    "pack {character} hands out the informant — only Mira may"
                );
            }
        }
        // Every character can turn away a message meant for somebody else —
        // without giving away who it IS for. That's the players' job.
        let names: Vec<&str> = s.packs.values().map(|p| p.name.as_str()).collect();
        for character in CAST {
            let notice = s
                .misdelivery_notice(character)
                .unwrap_or_else(|| panic!("pack {character} has no misdelivered line"));
            for name in &names {
                assert!(
                    !notice.contains(name),
                    "pack {character}'s misdelivered line names {name} — it must not say who the message is for"
                );
            }
        }
    }

    #[test]
    fn pasted_text_is_matched_through_whitespace_case_and_extra_prose() {
        let s = scenarios(&[
            ("a", pack(vec![mission("b", "Carry this to B please", "B got it")])),
            ("b", pack(vec![])),
        ]);
        let (owner, mission) = s
            .mission_in_pasted_text("look:\n\n  CARRY  this to B   please\n— forwarded")
            .expect("forgiving match");
        assert_eq!(owner, "a");
        assert_eq!(mission.to, "b");
        assert!(s.mission_in_pasted_text("carry this to").is_none());
        assert!(s.mission_in_pasted_text("").is_none());
    }

    #[test]
    fn lint_rejects_a_mission_text_nested_in_another_line() {
        let s = scenarios(&[
            ("a", pack(vec![mission("b", "fire on main street", "ok")])),
            (
                "b",
                pack(vec![mission("a", "big fire on main street now", "fine")]),
            ),
        ]);
        assert!(s.lint().is_err());
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
    fn informant_tip_renders_the_link_in_place_of_the_placeholder() {
        let mut p = pack(vec![]);
        p.informant_tip = Some("someone knows: {link} — go".into());
        assert_eq!(
            p.informant_tip_message("https://dashchat.org/add-contact/CODE")
                .unwrap(),
            "someone knows: https://dashchat.org/add-contact/CODE — go"
        );
        assert_eq!(pack(vec![]).informant_tip_message("x"), None);
    }

    #[test]
    fn lint_rejects_an_informant_tip_without_the_link_placeholder() {
        let mut p = pack(vec![]);
        p.informant_tip = Some("talk to the informant, somehow".into());
        assert!(scenarios(&[("a", p)]).lint().is_err());
    }

    #[test]
    fn lint_rejects_a_tipping_character_nobody_delivers_to() {
        let mut a = pack(vec![mission("b", "t1", "s1")]);
        a.informant_tip = Some("psst: {link}".into());
        // Nothing is addressed to "a", so the tip could never fire.
        assert!(scenarios(&[("a", a.clone()), ("b", pack(vec![]))]).lint().is_err());
        let b = pack(vec![mission("a", "t2", "s2")]);
        scenarios(&[("a", a), ("b", b)]).lint().unwrap();
    }

    #[test]
    fn lint_rejects_a_mission_text_nested_in_an_informant_tip() {
        let mut p = pack(vec![mission("b", "fire on main street", "ok")]);
        p.informant_tip = Some("psst, fire on main street, and {link}".into());
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
