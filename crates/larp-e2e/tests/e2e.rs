//! Milestone-1 end-to-end test (docs/design.md §8):
//! one player node + two character bots share one in-memory mailbox.
//!
//! Phase 1: poster-QR onboarding — each bot accepts the contact request and
//!          greets the player in their private direct chat.
//! Phase 2: the firefighters bot fires a mission into its own chat; the player
//!          copies it into the *hospital's* chat (with a prefix, mangled
//!          whitespace and the wrong case — the courier's clipboard is not
//!          careful) and the hospital bot answers with the success line.
//!          Pasting the same mission back at the firefighters instead earns a
//!          "this message is not for me!" nudge.
//! Phase 3: wipe the firefighters bot's data dir, restart it from the same
//!          identity bundle, and prove the *same printed QR string* still
//!          onboards a new player into a new direct chat.

use std::collections::BTreeMap;
use std::time::Duration;

use dashchat_node::mailbox::MailboxOperation;
use dashchat_node::testing::TestNode;
use dashchat_node::{ChatId, NodeConfig, Profile};
use mailbox_client::mem::MemMailbox;

use larp_bot::bot::{Bot, BotState, build_node};
use larp_bot::cast::Cast;
use larp_bot::config::Timing;
use larp_bot::identity::IdentityBundle;
use larp_bot::qr;
use larp_bot::scenario::{Mission, Pack, Scenarios};

const FF_MISSION: &str = "FF-MISSION-1: smoke on Main Street, carry this to the hospital!";
const FF_SUCCESS: &str = "HOSP-ACK-1: received, ambulances rolling.";
const HOSP_MISSION: &str = "HOSP-MISSION-1: trapped person reported, carry this to the firefighters!";
const HOSP_SUCCESS: &str = "FF-ACK-1: rescue crew dispatched.";
const FF_MISDELIVERED: &str = "FF-NOPE: this message is not for me!";
const FF_AVATAR: &str = "data:image/png;base64,AQID";

fn test_scenarios() -> Scenarios {
    let mut packs = BTreeMap::new();
    packs.insert(
        "firefighters".to_string(),
        Pack {
            name: "Firefighters".into(),
            greeting: "FF-GREETING: fire station online.".into(),
            comeback: None,
            misdelivered: Some(FF_MISDELIVERED.into()),
            missions: vec![Mission {
                to: "hospital".into(),
                text: FF_MISSION.into(),
                success: FF_SUCCESS.into(),
            }],
            avatar: Some(FF_AVATAR.into()),
        },
    );
    packs.insert(
        "hospital".to_string(),
        Pack {
            name: "Hospital".into(),
            greeting: "HOSP-GREETING: hospital online.".into(),
            comeback: None,
            misdelivered: None,
            missions: vec![Mission {
                to: "firefighters".into(),
                text: HOSP_MISSION.into(),
                success: HOSP_SUCCESS.into(),
            }],
            avatar: None,
        },
    );
    let scenarios = Scenarios { packs };
    scenarios.lint().unwrap();
    scenarios
}

/// Test node config: fast mailbox polling. The v0.18.9 node has no further
/// networking knobs; all transport in this test is the shared in-memory
/// mailbox.
fn test_node_config() -> NodeConfig {
    NodeConfig::testing()
}

fn fast_timing() -> Timing {
    Timing {
        min_interval_secs: 1,
        max_interval_secs: 2,
        first_mission_delay_secs: 1,
        poll_interval_secs: 1,
    }
}

/// Poll `f` until it returns true or the timeout elapses.
async fn wait_until<F, Fut>(what: &str, timeout: Duration, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if f().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn messages_of(node: &TestNode, chat: ChatId) -> Vec<String> {
    node.get_messages(chat)
        .await
        .map(|msgs| {
            msgs.iter()
                .map(|m| m.content.message().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The direct chat a player has with a character, derived from the printed
/// bundle alone — the same derivation both sides do.
fn chat_with(player: &TestNode, bundle: &IdentityBundle) -> ChatId {
    #[allow(deprecated)] // FakeAgentId is what direct_chat_topic takes today
    player.direct_chat_topic(dashchat_node::FakeAgentId::from(bundle.device_id().unwrap()))
}

struct RunningBot {
    node: dashchat_node::Node,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

async fn start_bot(
    data_dir: &std::path::Path,
    bundle: &IdentityBundle,
    cast: &Cast,
    mailbox: &MemMailbox<MailboxOperation>,
) -> RunningBot {
    let (node, rx) = build_node(data_dir, bundle, test_node_config())
        .await
        .expect("bot node builds");
    node.mailboxes.register(mailbox.client()).await;
    let bot = Bot::new(
        node.clone(),
        bundle.clone(),
        cast.resolve().unwrap(),
        test_scenarios(),
        fast_timing(),
        data_dir.join("state.json"),
    )
    .expect("bot constructs");
    let task = tokio::spawn(bot.run_loop(rx));
    RunningBot { node, task }
}

async fn player(name: &str, mailbox: &MemMailbox<MailboxOperation>) -> TestNode {
    let node = TestNode::new(test_node_config(), name).await;
    node.add_mailbox_client(mailbox.client()).await;
    node.set_profile(Profile {
        name: name.into(),
        surname: None,
        avatar: None,
        about: None,
    })
    .await
    .expect("player sets a profile");
    node
}

#[tokio::test(flavor = "multi_thread")]
async fn paste_delivery_roundtrip_and_wipe_survival() {
    dashchat_node::testing::setup_tracing(&["info"], false);
    let mailbox = MemMailbox::<MailboxOperation>::new();

    // --- The cast: two characters, generated offline like `larp-bot keygen`.
    let ff_bundle = IdentityBundle::generate("firefighters");
    let hosp_bundle = IdentityBundle::generate("hospital");
    let mut cast = Cast::default();
    cast.characters
        .insert("firefighters".into(), ff_bundle.cast_entry().unwrap());
    cast.characters
        .insert("hospital".into(), hosp_bundle.cast_entry().unwrap());

    // The printed wall posters (QR strings), rendered before any node exists.
    let ff_poster = ff_bundle.contact_code().unwrap();
    let hosp_poster = hosp_bundle.contact_code().unwrap();

    // --- Stations come up.
    let ff_dir = tempfile::tempdir().unwrap();
    let hosp_dir = tempfile::tempdir().unwrap();
    let ff_bot = start_bot(ff_dir.path(), &ff_bundle, &cast, &mailbox).await;
    let _hosp_bot = start_bot(hosp_dir.path(), &hosp_bundle, &cast, &mailbox).await;

    // --- A player arrives and scans both wall posters.
    let p1 = player("p1", &mailbox).await;
    p1.add_contact(qr::decode_contact_code(&ff_poster).unwrap())
        .await
        .expect("p1 adds firefighters");
    p1.add_contact(qr::decode_contact_code(&hosp_poster).unwrap())
        .await
        .expect("p1 adds hospital");

    let ff_chat = chat_with(&p1, &ff_bundle);
    let hosp_chat = chat_with(&p1, &hosp_bundle);

    // --- Phase 1: both bots accept, and greet in their own direct chat.
    wait_until("both bots greet the player", Duration::from_secs(90), || async {
        messages_of(&p1, ff_chat)
            .await
            .iter()
            .any(|t| t.contains("FF-GREETING"))
            && messages_of(&p1, hosp_chat)
                .await
                .iter()
                .any(|t| t.contains("HOSP-GREETING"))
    })
    .await;

    // The bots' profiles (avatar included — it rides the same SetProfile op)
    // reach the player, so the chats show a name and a face.
    let ff_agent = ff_bundle.agent_id().unwrap();
    let hosp_agent = hosp_bundle.agent_id().unwrap();
    wait_until("bot profiles reach p1", Duration::from_secs(60), || async {
        let ff = p1.projection.get_profile(ff_agent).await.ok().flatten();
        let hosp = p1.projection.get_profile(hosp_agent).await.ok().flatten();
        ff.is_some_and(|p| p.avatar.as_deref() == Some(FF_AVATAR)) && hosp.is_some()
    })
    .await;

    // --- Phase 2: the firefighters hand out a mission for the hospital.
    wait_until("firefighters fire a mission", Duration::from_secs(90), || async {
        messages_of(&p1, ff_chat).await.iter().any(|t| t == FF_MISSION)
    })
    .await;

    // The courier copies it into the hospital's chat. Deliberately sloppy:
    // a prefix, collapsed newlines and the wrong case all have to survive.
    p1.send_message(
        hosp_chat,
        format!("look what they gave me:\n\n   {}\n", FF_MISSION.to_uppercase()),
        None,
        None,
    )
    .await
    .expect("p1 pastes the mission at the hospital");

    wait_until("the hospital acks the delivery", Duration::from_secs(90), || async {
        messages_of(&p1, hosp_chat).await.iter().any(|t| t == FF_SUCCESS)
    })
    .await;

    // Same mission pasted back at its author: not for them either. The nudge
    // must never name the real recipient — finding them is the game.
    p1.send_message(ff_chat, FF_MISSION.to_string(), None, None)
        .await
        .expect("p1 pastes the mission at the wrong station");
    wait_until("the firefighters turn the message away", Duration::from_secs(90), || async {
        messages_of(&p1, ff_chat)
            .await
            .contains(&FF_MISDELIVERED.to_string())
    })
    .await;
    assert!(!FF_MISDELIVERED.contains("Hospital"));

    // The origin bot recorded the mission it handed out (one per player).
    wait_until("origin records the fired mission", Duration::from_secs(30), || async {
        BotState::load(&ff_dir.path().join("state.json"))
            .fired
            .get(&ff_chat.to_string())
            .is_some_and(|texts| texts.iter().any(|t| t == FF_MISSION))
    })
    .await;

    // --- Phase 3: wipe the firefighters station and restart from the bundle.
    ff_bot.task.abort();
    let _ = ff_bot.task.await;
    ff_bot.node.shutdown().await.expect("ff node shuts down");
    std::fs::remove_dir_all(ff_dir.path()).unwrap();
    std::fs::create_dir_all(ff_dir.path()).unwrap();
    let _ff_bot2 = start_bot(ff_dir.path(), &ff_bundle, &cast, &mailbox).await;

    // The SAME printed poster still onboards a brand-new player...
    let p2 = player("p2", &mailbox).await;
    p2.add_contact(qr::decode_contact_code(&ff_poster).unwrap())
        .await
        .expect("p2 adds firefighters after the wipe");
    wait_until("rebuilt bot's profile reaches p2", Duration::from_secs(60), || async {
        p2.projection
            .get_profile(ff_agent)
            .await
            .ok()
            .flatten()
            .is_some()
    })
    .await;

    // ...and the character greets them in their own chat. Generous: the
    // rebuilt bot first re-syncs the entire pre-wipe history (its sync-tracker
    // watermarks were wiped too) before it gets to p2.
    let p2_ff_chat = chat_with(&p2, &ff_bundle);
    wait_until("rebuilt bot greets the new player", Duration::from_secs(180), || async {
        messages_of(&p2, p2_ff_chat)
            .await
            .iter()
            .any(|t| t.contains("FF-GREETING"))
    })
    .await;
}
