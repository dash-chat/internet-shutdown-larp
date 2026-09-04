use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
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
    /// The pack's designated opener: fired before any random draw, so it
    /// reaches each player right after the greeting. One per pack at most
    /// (linted). Nadia's network announcement uses this — her greeting
    /// promises "your first one is coming now", and this is what makes that
    /// promise deterministic instead of a lucky draw.
    #[serde(default)]
    pub first: bool,
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
    /// The mayor's secret: the first time a delivery lands here, this
    /// character tells the player what they know — ending with the one line
    /// the mayor's bot listens for, and where to paste it. Not a chance:
    /// carrying something to them earns it, once per player.
    ///
    /// Exactly one character carries this: Nadia, who saw the order on the
    /// mayor's own desk. The side plot has one door, and it is behind an
    /// actual delivery — which conveniently lands the player at the base
    /// station, the wifi the mayor's chat answers in.
    ///
    /// Blank lines split it into separate messages, one per paragraph (see
    /// [`Pack::secret_tip_messages`]), like `mayor_fallen`. One paragraph
    /// must carry the mayor's trigger phrase verbatim — a unit test in
    /// `crate::spec` fails if the two drift.
    #[serde(default)]
    pub secret_tip: Option<String>,
    /// Sent unprompted when the mayor falls — the character bot polls the
    /// flag his spec bot writes (`BotConfig::mayor_fallen_flag`) and erupts
    /// the moment it appears, into the chat of the player who felled him
    /// (the flag names them; once per chat). Blank lines split it into
    /// separate messages (see [`Pack::mayor_fallen_messages`]): each gets
    /// its own typing pause, so the first burst lands fast instead of the
    /// whole eruption arriving as one long-typed block. Only Nadia carries
    /// this line: her bot shares the base-station Pi with the mayor's, so
    /// she is the one character who can actually see it happen.
    #[serde(default)]
    pub mayor_fallen: Option<String>,
    /// The town map, sent as its own message right after the greeting: a
    /// short line plus a photo attachment, so a new player knows where the
    /// other stations physically are. Only Nadia carries one — she is the
    /// character whose greeting is the tutorial. The photo is armed at
    /// RUNTIME (see [`MapMessage`]): until someone sends her one captioned
    /// with the trigger, greetings go out with no map message at all.
    #[serde(default)]
    pub map: Option<MapMessage>,
    #[serde(default)]
    pub missions: Vec<Mission>,
    /// The character's chat avatar as a `data:image/png;base64,…` URI (the
    /// only image form the app renders). Not toml: `load_dir` fills it from
    /// the sibling `scenarios/<character>.png`, if present.
    #[serde(skip)]
    pub avatar: Option<String>,
}

impl Pack {
    /// A mission this player has not been given yet: the pack's `first`
    /// opener before anything else (the deterministic opener — Nadia's
    /// network announcement), then a random draw. When `avoid` is non-empty
    /// the draw prefers missions not addressed to those characters — used
    /// after a delivery lands, so the follow-up doesn't send the courier
    /// straight back where they came from — but falls back to the full
    /// unused pool rather than starving the player. `None` once the pack is
    /// exhausted: templates never repeat in a chat, so a pasted delivery
    /// always maps to exactly one mission.
    pub fn pick_unused_mission(
        &self,
        fired_texts: &[&str],
        avoid: &BTreeSet<String>,
    ) -> Option<&Mission> {
        use rand::Rng as _;
        let unused: Vec<&Mission> = self
            .missions
            .iter()
            .filter(|m| !fired_texts.contains(&m.text.as_str()))
            .collect();
        if let Some(opener) = unused.iter().find(|m| m.first) {
            return Some(opener);
        }
        let preferred: Vec<&Mission> = unused
            .iter()
            .copied()
            .filter(|m| !avoid.contains(&m.to))
            .collect();
        let pool = if preferred.is_empty() {
            &unused
        } else {
            &preferred
        };
        if pool.is_empty() {
            return None;
        }
        let idx = rand::thread_rng().gen_range(0..pool.len());
        Some(pool[idx])
    }
    /// The eruption as it goes into the chat: `mayor_fallen` split on blank
    /// lines, one message per paragraph. A person yelling news types in
    /// bursts, and with the typing pause charged per message, the first
    /// burst reaches the player in about a second. Empty when the pack has
    /// no line. (The nested-text lint stays sound: a mission text hiding in
    /// a paragraph is contained in the whole line, which is what it checks.)
    pub fn mayor_fallen_messages(&self) -> Vec<String> {
        self.mayor_fallen
            .as_deref()
            .map(split_paragraphs)
            .unwrap_or_default()
    }

    /// The greeting as it goes into the chat: split on blank lines like
    /// [`Pack::mayor_fallen_messages`], so a long welcome (Nadia's is the
    /// game's tutorial) reads as chat bursts rather than one wall of text.
    /// A greeting without blank lines stays a single message.
    pub fn greeting_messages(&self) -> Vec<String> {
        split_paragraphs(&self.greeting)
    }

    /// The secret as it goes into the chat: `secret_tip` split on blank
    /// lines, one message per paragraph — a confession typed in bursts, the
    /// same shape as [`Pack::mayor_fallen_messages`]. Empty when the pack
    /// carries none.
    pub fn secret_tip_messages(&self) -> Vec<String> {
        self.secret_tip
            .as_deref()
            .map(split_paragraphs)
            .unwrap_or_default()
    }
}

/// Blank-line paragraphs of a pack line, one chat message each.
fn split_paragraphs(raw: &str) -> Vec<String> {
    raw.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// The map message: `text` is the line the photo goes out with. The photo
/// itself is NOT in the repo — the organizer draws it (assets/
/// town-map-base.png is the blank canvas) and arms the bot at runtime by
/// sending it a photo captioned `update_trigger` in any direct chat. The bot
/// keeps the latest such photo and attaches it after every greeting from
/// then on; a fresh trigger message replaces it on the spot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapMessage {
    /// The line sent with the photo, verbatim.
    pub text: String,
    /// The caption that arms/replaces the map (matched like a mission:
    /// normalized containment, so surrounding prose is fine).
    pub update_trigger: String,
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
    /// - at most one mission per pack is marked `first` (the deterministic
    ///   opener),
    /// - mission texts are unique across ALL packs (a text identifies exactly
    ///   one mission),
    /// - success lines are unique across ALL packs and never collide with a
    ///   mission text,
    /// - a character with a `secret_tip` is the recipient of at least one
    ///   mission (the tip only fires on a delivery *to* them, so otherwise the
    ///   side plot is sealed off),
    /// - no mission text is *contained* in any other line a player might
    ///   paste (another mission, a success line, a comeback, a misdelivery
    ///   notice, a secret tip, a mayor_fallen burst, the map line or its
    ///   trigger) — matching is containment-based (see
    ///   [`Scenarios::mission_in_pasted_text`]), so a nested text would make
    ///   the paste ambiguous,
    /// - the map trigger is not contained in any mission text (the bot checks
    ///   the trigger first, so such a paste would arm the map instead of
    ///   landing the delivery).
    pub fn lint(&self) -> Result<()> {
        let mut texts: BTreeSet<&str> = BTreeSet::new();
        let mut successes: BTreeSet<&str> = BTreeSet::new();
        for (character, pack) in &self.packs {
            if pack.greeting.trim().is_empty() {
                bail!("pack {character}: empty greeting");
            }
            if pack.missions.iter().filter(|m| m.first).count() > 1 {
                bail!("pack {character}: more than one mission marked first");
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
            if let Some(tip) = &pack.secret_tip {
                if tip.trim().is_empty() {
                    bail!("pack {character}: empty secret_tip text");
                }
                if texts.contains(tip.as_str()) || successes.contains(tip.as_str()) {
                    bail!("pack {character}: secret_tip collides with a mission");
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
                        "pack {character} carries the secret, but no mission is addressed \
                         to {character} — the tip could never fire"
                    );
                }
            }
            if let Some(fallen) = &pack.mayor_fallen {
                if fallen.trim().is_empty() {
                    bail!("pack {character}: empty mayor_fallen text");
                }
                if texts.contains(fallen.as_str()) || successes.contains(fallen.as_str()) {
                    bail!("pack {character}: mayor_fallen collides with a mission");
                }
            }
            if let Some(map) = &pack.map {
                if map.text.trim().is_empty() {
                    bail!("pack {character}: empty map text");
                }
                if texts.contains(map.text.as_str()) || successes.contains(map.text.as_str()) {
                    bail!("pack {character}: map text collides with a mission");
                }
                // The trigger is matched by normalized containment against
                // everything players (or the organizer) type, exactly like a
                // mission text — so it must not hide inside one, or carry one
                // inside itself. The containment sweep below covers the
                // mission-inside-trigger direction; the reverse would need
                // the trigger to be a mission, which uniqueness rules out.
                if map.update_trigger.trim().is_empty() {
                    bail!("pack {character}: empty map update_trigger");
                }
                if texts.contains(map.update_trigger.as_str())
                    || successes.contains(map.update_trigger.as_str())
                {
                    bail!("pack {character}: map update_trigger collides with a mission");
                }
                // The bot checks the trigger BEFORE mission recognition, so a
                // mission text carrying the trigger would arm the map instead
                // of acking the delivery.
                if let Some(t) = texts
                    .iter()
                    .find(|t| normalize(t).contains(&normalize(&map.update_trigger)))
                {
                    bail!(
                        "pack {character}: map update_trigger is contained in mission text {t:?}"
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
                    .chain(pack.secret_tip.iter().map(|t| t.as_str()))
                    .chain(pack.mayor_fallen.iter().map(|t| t.as_str()))
                    .chain(pack.map.iter().map(|m| m.text.as_str()))
                    .chain(pack.map.iter().map(|m| m.update_trigger.as_str()));
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
                pack.missions.iter().map(move |m| (character.as_str(), m))
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
            secret_tip: None,
            map: None,
            mayor_fallen: None,
            missions,
            avatar: None,
        }
    }

    fn mission(to: &str, text: &str, success: &str) -> Mission {
        Mission {
            first: false,
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
    fn lint_rejects_a_map_trigger_hiding_in_a_mission_text() {
        // The bot checks the trigger before mission recognition: a mission
        // text carrying it would arm the map instead of acking the delivery.
        let mut a = pack(vec![mission("b", "carry THISISTHENEWMAP to town", "ok")]);
        a.map = Some(MapMessage {
            text: "here is the map".into(),
            update_trigger: "thisisthenewmap".into(),
        });
        let s = scenarios(&[("a", a), ("b", pack(vec![]))]);
        assert!(s.lint().is_err());
    }

    #[test]
    fn lint_accepts_a_distinct_map_trigger() {
        let mut a = pack(vec![mission("b", "t1", "s1")]);
        a.map = Some(MapMessage {
            text: "here is the map".into(),
            update_trigger: "thisisthenewmap".into(),
        });
        let s = scenarios(&[("a", a), ("b", pack(vec![]))]);
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
    const CAST: [&str; 5] = ["cousin", "grandpa", "mum", "neighbour", "sister"];

    #[test]
    fn shipped_packs_lint() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios");
        let s = Scenarios::load_dir(dir).unwrap();
        assert_eq!(s.packs.keys().map(String::as_str).collect::<Vec<_>>(), CAST);
        for character in CAST {
            let pack = s.pack(character).expect("missing pack");
            assert!(
                pack.avatar
                    .as_deref()
                    .is_some_and(|a| a.starts_with("data:image/png;base64,")),
                "pack {character} has no avatar (scenarios/{character}.png missing?)"
            );
        }
        // Mira, stuck behind the shelter desk, answers the first player
        // message after a quiet spell.
        assert!(s.pack("sister").unwrap().comeback.is_some());
        // Only Nadia erupts when the mayor falls: the flag his bot writes
        // exists only on the base-station Pi they share, so a mayor_fallen
        // line anywhere else would be dead content.
        for character in CAST {
            assert_eq!(
                s.pack(character).unwrap().mayor_fallen.is_some(),
                character == "neighbour",
                "pack {character}: only Nadia can see the mayor fall"
            );
        }
        // Only Nadia's greeting teaches the courier job. Everyone else just
        // says hello in character — the tutorial has exactly one voice, and
        // repeating it five times would drown it.
        for character in CAST {
            let greeting = &s.pack(character).unwrap().greeting;
            assert_eq!(
                normalize(greeting).contains("copy"),
                character == "neighbour",
                "pack {character}: only Nadia's greeting explains what to do"
            );
        }
        // Exactly one opener in the whole game, and it is Nadia's: her
        // greeting is the tutorial and promises the first message, so her
        // pack must actually deliver it deterministically.
        for character in CAST {
            let firsts = s
                .pack(character)
                .unwrap()
                .missions
                .iter()
                .filter(|m| m.first)
                .count();
            assert_eq!(
                firsts,
                usize::from(character == "neighbour"),
                "pack {character}: only Nadia opens the game, with exactly one first mission"
            );
        }
        // Exactly one character carries the mayor's secret: Nadia, who saw
        // the order on his desk. More than one door would make the side plot
        // a lottery; none would make it unreachable.
        for character in CAST {
            let tip = s.pack(character).unwrap().secret_tip.as_deref();
            if character == "neighbour" {
                tip.expect("Nadia is the only way into the side plot");
            } else {
                assert!(
                    tip.is_none(),
                    "pack {character} carries the secret — only Nadia may"
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
            (
                "a",
                pack(vec![mission("b", "Carry this to B please", "B got it")]),
            ),
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
        p.comeback = Some(Comeback {
            after_secs: 60,
            text: "s1".into(),
        });
        let s = scenarios(&[("a", p), ("b", pack(vec![]))]);
        assert!(s.lint().is_err());
    }

    #[test]
    fn greeting_splits_on_blank_lines_into_separate_messages() {
        let mut p = pack(vec![]);
        p.greeting = "hello there\n\n  \n\nyou are the wire\n\nget going".into();
        assert_eq!(
            p.greeting_messages(),
            vec!["hello there", "you are the wire", "get going"]
        );
        // No blank lines — the greeting stays one message.
        assert_eq!(pack(vec![]).greeting_messages(), vec!["hello"]);
    }

    #[test]
    fn lint_rejects_two_first_missions_in_one_pack() {
        let mut m1 = mission("b", "t1", "s1");
        let mut m2 = mission("b", "t2", "s2");
        m1.first = true;
        m2.first = true;
        let s = scenarios(&[("a", pack(vec![m1.clone(), m2])), ("b", pack(vec![]))]);
        assert!(s.lint().is_err());
        let s = scenarios(&[
            ("a", pack(vec![m1, mission("b", "t2", "s2")])),
            ("b", pack(vec![])),
        ]);
        s.lint().unwrap();
    }

    #[test]
    fn lint_rejects_a_tipping_character_nobody_delivers_to() {
        let mut a = pack(vec![mission("b", "t1", "s1")]);
        a.secret_tip = Some("psst, someone wants to talk to you".into());
        // Nothing is addressed to "a", so the tip could never fire.
        assert!(scenarios(&[("a", a.clone()), ("b", pack(vec![]))])
            .lint()
            .is_err());
        let b = pack(vec![mission("a", "t2", "s2")]);
        scenarios(&[("a", a), ("b", b)]).lint().unwrap();
    }

    #[test]
    fn lint_rejects_a_mission_text_nested_in_an_secret_tip() {
        let mut p = pack(vec![mission("b", "fire on main street", "ok")]);
        p.secret_tip = Some("psst, fire on main street, and more".into());
        let s = scenarios(&[("a", p), ("b", pack(vec![]))]);
        assert!(s.lint().is_err());
    }

    #[test]
    fn mayor_fallen_splits_into_messages_on_blank_lines() {
        let mut p = pack(vec![]);
        p.mayor_fallen = Some("HE'S GONE!!\n\nThe car tore out.\n\n\n  Relax now.  ".into());
        assert_eq!(
            p.mayor_fallen_messages(),
            vec!["HE'S GONE!!", "The car tore out.", "Relax now."]
        );
        assert!(pack(vec![]).mayor_fallen_messages().is_empty());
    }

    #[test]
    fn pick_prefers_missions_away_from_the_avoided_character() {
        let p = pack(vec![mission("b", "t1", "s1"), mission("c", "t2", "s2")]);
        let avoid = BTreeSet::from(["b".to_string()]);
        // The draw is random, so hammer it: with "b" avoided and a "c"
        // mission available, "b" must never come up.
        for _ in 0..50 {
            assert_eq!(p.pick_unused_mission(&[], &avoid).unwrap().to, "c");
        }
    }

    #[test]
    fn pick_falls_back_to_avoided_targets_rather_than_starving() {
        let p = pack(vec![mission("b", "t1", "s1"), mission("b", "t2", "s2")]);
        let avoid = BTreeSet::from(["b".to_string()]);
        assert_eq!(p.pick_unused_mission(&["t1"], &avoid).unwrap().text, "t2");
        assert!(p.pick_unused_mission(&["t1", "t2"], &avoid).is_none());
    }

    #[test]
    fn pick_serves_the_opener_first_even_when_its_target_is_avoided() {
        let mut opener = mission("b", "t1", "s1");
        opener.first = true;
        let p = pack(vec![opener, mission("c", "t2", "s2")]);
        let avoid = BTreeSet::from(["b".to_string()]);
        assert_eq!(p.pick_unused_mission(&[], &avoid).unwrap().text, "t1");
        assert_eq!(p.pick_unused_mission(&["t1"], &avoid).unwrap().text, "t2");
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
