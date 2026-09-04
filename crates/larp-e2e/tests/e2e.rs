//! Milestone-1 end-to-end test (docs/design.md §8):
//! one player node + two character bots share one in-memory mailbox.
//!
//! Phase 1: poster-QR onboarding — each bot accepts the contact request and
//!          greets the player in their private direct chat.
//! Phase 2: mum's bot fires a mission into its own chat; the player
//!          copies it into *grandpa's* chat (with a prefix, mangled
//!          whitespace and the wrong case — the courier's clipboard is not
//!          careful) and grandpa's bot answers with the success line.
//!          Pasting the same mission back at mum instead earns a
//!          "this message is not for me!" nudge. The completed delivery
//!          also earns the mayor's secret, told as one message per
//!          paragraph — once, and only for a real delivery — and the
//!          secret REPLACES the usual follow-up mission. A second delivery
//!          finds the chat already tipped and earns the follow-up, drawn
//!          away from mum (the delivered mission's originator).
//! Phase 3: wipe mum's bot's data dir, restart it from the same
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

const MUM_MISSION: &str = "MUM-MISSION-1: smoke on Main Street, carry this to grandpa!";
const MUM_SUCCESS: &str = "GP-ACK-1: received, ambulances rolling.";
const MUM_MISSION_2: &str = "MUM-MISSION-2: the road is blocked, carry this to grandpa!";
const MUM_SUCCESS_2: &str = "GP-ACK-2: noted, taking the long way.";
const GP_MISSION: &str = "GP-MISSION-1: trapped person reported, carry this to mum!";
const GP_SUCCESS: &str = "MUM-ACK-1: rescue crew dispatched.";
const GP_MISSION_2: &str = "GP-MISSION-2: medicine running low, carry this to mum!";
const GP_SUCCESS_2: &str = "MUM-ACK-2: pharmacy run arranged.";
const GP_SIDE_MISSION: &str = "GP-MISSION-3: the shelter needs blankets, carry this to sis!";
const GP_SIDE_SUCCESS: &str = "SIS-ACK-1: blankets on the way.";
const MUM_MISDELIVERED: &str = "MUM-NOPE: this message is not for me!";
const MUM_AVATAR: &str = "data:image/png;base64,AQID";
// Two paragraphs, so the test pins the burst behavior: one message each.
const GP_TIP_1: &str = "GP-TIP: I saw the mayor's order on his desk myself.";
const GP_TIP_2: &str = "GP-TIP-LINE: let the north side burn.";

fn test_scenarios() -> Scenarios {
    let mut packs = BTreeMap::new();
    packs.insert(
        "mum".to_string(),
        Pack {
            name: "Firefighters".into(),
            greeting: "MUM-GREETING: mum online.".into(),
            comeback: None,
            misdelivered: Some(MUM_MISDELIVERED.into()),
            // No tip here: mum only ever gets the mission pasted back at her
            // (a misdelivery), which must never earn the secret.
            secret_tip: None,
            mayor_fallen: None,
            map: None,
            // The opener is what mum hands out in the test; the second
            // template exists so the player can retype it by hand (matching
            // is text-based) for a second delivery at grandpa — the one that
            // earns a follow-up, the first being eaten by the secret.
            missions: vec![
                Mission {
                    first: true,
                    to: "grandpa".into(),
                    text: MUM_MISSION.into(),
                    success: MUM_SUCCESS.into(),
                },
                Mission {
                    first: false,
                    to: "grandpa".into(),
                    text: MUM_MISSION_2.into(),
                    success: MUM_SUCCESS_2.into(),
                },
            ],
            avatar: Some(MUM_AVATAR.into()),
        },
    );
    packs.insert(
        "grandpa".to_string(),
        Pack {
            name: "Grandpa".into(),
            greeting: "GP-GREETING: grandpa online.".into(),
            comeback: None,
            misdelivered: None,
            secret_tip: Some(format!("{GP_TIP_1}\n\n{GP_TIP_2}")),
            mayor_fallen: None,
            map: None,
            // The opener goes out on the first-mission delay; the other two
            // sit behind the (test-long) timer, so only the delivery-triggered
            // follow-up can surface one of them — and it must prefer the one
            // NOT addressed to mum, the delivered mission's originator.
            missions: vec![
                Mission {
                    first: true,
                    to: "mum".into(),
                    text: GP_MISSION.into(),
                    success: GP_SUCCESS.into(),
                },
                Mission {
                    first: false,
                    to: "mum".into(),
                    text: GP_MISSION_2.into(),
                    success: GP_SUCCESS_2.into(),
                },
                Mission {
                    first: false,
                    to: "sister".into(),
                    text: GP_SIDE_MISSION.into(),
                    success: GP_SIDE_SUCCESS.into(),
                },
            ],
            avatar: None,
        },
    );
    // Pack-only third character: nobody runs her bot here, she just gives
    // grandpa's follow-up somewhere to point besides mum.
    packs.insert(
        "sister".to_string(),
        Pack {
            name: "Sis".into(),
            greeting: "SIS-GREETING: sis online.".into(),
            comeback: None,
            misdelivered: None,
            secret_tip: None,
            mayor_fallen: None,
            map: None,
            missions: vec![],
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
        // Longer than the whole test: after the first-delay opener, the only
        // way another mission appears is the delivery-triggered follow-up.
        min_interval_secs: 600,
        max_interval_secs: 600,
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

/// Every message in a chat as `(text, is_a_reply)` — the ack has to arrive
/// threaded onto the delivery it answers, not as a loose line.
async fn messages_with_replies(node: &TestNode, chat: ChatId) -> Vec<(String, bool)> {
    node.get_messages(chat)
        .await
        .map(|msgs| {
            msgs.iter()
                .map(|m| {
                    (
                        m.content.message().to_string(),
                        m.content.reply().is_some(),
                    )
                })
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
        None,
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
    let mut mum_bundle = IdentityBundle::generate("mum");
    mum_bundle.profile_name = Some("Firefighters".into());
    let mut gp_bundle = IdentityBundle::generate("grandpa");
    gp_bundle.profile_name = Some("Grandpa".into());
    let mut cast = Cast::default();
    cast.characters
        .insert("mum".into(), mum_bundle.cast_entry().unwrap());
    cast.characters
        .insert("grandpa".into(), gp_bundle.cast_entry().unwrap());

    // The printed wall posters (QR strings), rendered before any node exists.
    let mum_poster = mum_bundle.contact_code().unwrap();
    let gp_poster = gp_bundle.contact_code().unwrap();

    // --- Stations come up.
    let mum_dir = tempfile::tempdir().unwrap();
    let gp_dir = tempfile::tempdir().unwrap();
    let mum_bot = start_bot(mum_dir.path(), &mum_bundle, &cast, &mailbox).await;
    let _hosp_bot = start_bot(gp_dir.path(), &gp_bundle, &cast, &mailbox).await;

    // --- A player arrives and scans both wall posters.
    let p1 = player("p1", &mailbox).await;
    p1.add_contact(qr::decode_contact_code(&mum_poster).unwrap())
        .await
        .expect("p1 adds mum");
    p1.add_contact(qr::decode_contact_code(&gp_poster).unwrap())
        .await
        .expect("p1 adds grandpa");

    let mum_chat = chat_with(&p1, &mum_bundle);
    let gp_chat = chat_with(&p1, &gp_bundle);

    // --- Phase 1: both bots accept, and greet in their own direct chat.
    wait_until("both bots greet the player", Duration::from_secs(90), || async {
        messages_of(&p1, mum_chat)
            .await
            .iter()
            .any(|t| t.contains("MUM-GREETING"))
            && messages_of(&p1, gp_chat)
                .await
                .iter()
                .any(|t| t.contains("GP-GREETING"))
    })
    .await;

    // The bots' profiles (avatar included — it rides the same SetProfile op)
    // reach the player, so the chats show a name and a face.
    let mum_agent = mum_bundle.agent_id().unwrap();
    let gp_agent = gp_bundle.agent_id().unwrap();
    wait_until("bot profiles reach p1", Duration::from_secs(60), || async {
        let mum = p1.projection.get_profile(mum_agent).await.ok().flatten();
        let gp = p1.projection.get_profile(gp_agent).await.ok().flatten();
        mum.is_some_and(|p| p.avatar.as_deref() == Some(MUM_AVATAR)) && gp.is_some()
    })
    .await;

    // --- Phase 2: mum hands out a mission for grandpa.
    wait_until("mum fires a mission", Duration::from_secs(90), || async {
        messages_of(&p1, mum_chat).await.iter().any(|t| t == MUM_MISSION)
    })
    .await;

    // Grandpa's opener goes out on the same first-mission delay — wait it
    // out, so the delivery-triggered follow-up below draws from the two
    // remaining templates rather than the deterministic opener.
    wait_until("grandpa fires his opener", Duration::from_secs(90), || async {
        messages_of(&p1, gp_chat).await.iter().any(|t| t == GP_MISSION)
    })
    .await;

    // The courier copies it into grandpa's chat. Deliberately sloppy:
    // a prefix, collapsed newlines and the wrong case all have to survive.
    p1.send_message(
        gp_chat,
        format!("look what they gave me:\n\n   {}\n", MUM_MISSION.to_uppercase()),
        None,
        None,
    )
    .await
    .expect("p1 pastes the mission at grandpa");

    wait_until("grandpa acks the delivery", Duration::from_secs(90), || async {
        messages_of(&p1, gp_chat).await.iter().any(|t| t == MUM_SUCCESS)
    })
    .await;

    // The ack is threaded onto the pasted delivery, so a courier who dropped
    // three messages at one station can tell which one was answered.
    assert!(
        messages_with_replies(&p1, gp_chat)
            .await
            .contains(&(MUM_SUCCESS.to_string(), true)),
        "the success line should be a reply to the delivery, not a loose message"
    );

    // The delivery earns the secret: both paragraphs of grandpa's tip land
    // as separate messages, neither threaded — the secret answers nothing,
    // it starts something.
    wait_until("grandpa tells the secret", Duration::from_secs(90), || async {
        let msgs = messages_of(&p1, gp_chat).await;
        msgs.contains(&GP_TIP_1.to_string()) && msgs.contains(&GP_TIP_2.to_string())
    })
    .await;
    let with_replies = messages_with_replies(&p1, gp_chat).await;
    for part in [GP_TIP_1, GP_TIP_2] {
        assert!(
            with_replies.contains(&(part.to_string(), false)),
            "each burst of the secret should be its own message, not a reply"
        );
    }
    // Once per player: grandpa's remaining deliveries stay tip-free.
    wait_until("grandpa records the tip", Duration::from_secs(30), || async {
        BotState::load(&gp_dir.path().join("state.json"))
            .tipped
            .contains(&gp_chat.to_string())
    })
    .await;

    // The secret ate the follow-up: a delivery that earns the secret
    // must NOT also hand out a regular mission — the side plot is the job.
    // The tip lands after the follow-up decision, so by now it's final.
    assert!(
        !messages_of(&p1, gp_chat)
            .await
            .iter()
            .any(|t| t == GP_MISSION_2 || t == GP_SIDE_MISSION),
        "a tipping delivery must not also fire a follow-up mission"
    );

    // A SECOND delivery (retyped by hand — matching is text-based, mum never
    // sent this one to the player) finds the chat already tipped, so it earns
    // the regular follow-up. With the timer parked 600s out, only the success
    // trigger can fire it — and it steers away from mum, the delivered
    // mission's originator: of grandpa's two remaining templates, the
    // sis-bound one must come up.
    p1.send_message(gp_chat, MUM_MISSION_2.to_string(), None, None)
        .await
        .expect("p1 delivers a second message to grandpa");
    wait_until("grandpa acks the second delivery", Duration::from_secs(90), || async {
        messages_of(&p1, gp_chat).await.iter().any(|t| t == MUM_SUCCESS_2)
    })
    .await;
    wait_until("grandpa fires a follow-up mission", Duration::from_secs(90), || async {
        messages_of(&p1, gp_chat).await.iter().any(|t| t == GP_SIDE_MISSION)
    })
    .await;
    assert!(
        !messages_of(&p1, gp_chat).await.iter().any(|t| t == GP_MISSION_2),
        "the follow-up must not send the courier straight back to the originator \
         while another destination is available"
    );

    // Same mission pasted back at its author: not for them either. The nudge
    // must never name the real recipient — finding them is the game.
    p1.send_message(mum_chat, MUM_MISSION.to_string(), None, None)
        .await
        .expect("p1 pastes the mission at the wrong station");
    wait_until("mum turns the message away", Duration::from_secs(90), || async {
        messages_of(&p1, mum_chat)
            .await
            .contains(&MUM_MISDELIVERED.to_string())
    })
    .await;
    assert!(!MUM_MISDELIVERED.contains("Grandpa"));
    // Threaded too: the nudge points at the message that went astray.
    assert!(
        messages_with_replies(&p1, mum_chat)
            .await
            .contains(&(MUM_MISDELIVERED.to_string(), true)),
        "the misdelivery nudge should be a reply to the message it turns away"
    );
    // This is mum's chat, where no part of the secret may appear at all.
    assert!(
        !messages_of(&p1, mum_chat)
            .await
            .iter()
            .any(|t| t.contains("GP-TIP")),
        "mum has no secret — a misdelivery must not tell it"
    );

    // The origin bot recorded the mission it handed out (one per player).
    wait_until("origin records the fired mission", Duration::from_secs(30), || async {
        BotState::load(&mum_dir.path().join("state.json"))
            .fired
            .get(&mum_chat.to_string())
            .is_some_and(|texts| texts.iter().any(|t| t == MUM_MISSION))
    })
    .await;

    // --- Phase 3: wipe mum's station and restart from the bundle.
    mum_bot.task.abort();
    let _ = mum_bot.task.await;
    mum_bot.node.shutdown().await.expect("mum node shuts down");
    std::fs::remove_dir_all(mum_dir.path()).unwrap();
    std::fs::create_dir_all(mum_dir.path()).unwrap();
    let _ff_bot2 = start_bot(mum_dir.path(), &mum_bundle, &cast, &mailbox).await;

    // The SAME printed poster still onboards a brand-new player...
    let p2 = player("p2", &mailbox).await;
    p2.add_contact(qr::decode_contact_code(&mum_poster).unwrap())
        .await
        .expect("p2 adds mum after the wipe");
    wait_until("rebuilt bot's profile reaches p2", Duration::from_secs(60), || async {
        p2.projection
            .get_profile(mum_agent)
            .await
            .ok()
            .flatten()
            .is_some()
    })
    .await;

    // ...and the character greets them in their own chat. Generous: the
    // rebuilt bot first re-syncs the entire pre-wipe history (its sync-tracker
    // watermarks were wiped too) before it gets to p2.
    let p2_ff_chat = chat_with(&p2, &mum_bundle);
    wait_until("rebuilt bot greets the new player", Duration::from_secs(180), || async {
        messages_of(&p2, p2_ff_chat)
            .await
            .iter()
            .any(|t| t.contains("MUM-GREETING"))
    })
    .await;
}
