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
//!          also earns the informant's contact as a deep link (there is no
//!          informant poster) — once, and only for a real delivery.
//! Phase 3: wipe mum's bot's data dir, restart it from the same
//!          identity bundle, and prove the *same printed QR string* still
//!          onboards a new player into a new direct chat.

use std::collections::BTreeMap;
use std::time::Duration;

use dashchat_node::mailbox::MailboxOperation;
use dashchat_node::testing::TestNode;
use dashchat_node::{ChatId, NodeConfig, Profile};
use mailbox_client::mem::MemMailbox;

use larp_bot::bot::{Bot, BotState, InformantTip, build_node};
use larp_bot::cast::Cast;
use larp_bot::config::Timing;
use larp_bot::identity::IdentityBundle;
use larp_bot::qr;
use larp_bot::scenario::{Mission, Pack, Scenarios};

const MUM_MISSION: &str = "MUM-MISSION-1: smoke on Main Street, carry this to grandpa!";
const MUM_SUCCESS: &str = "GP-ACK-1: received, ambulances rolling.";
const GP_MISSION: &str = "GP-MISSION-1: trapped person reported, carry this to mum!";
const GP_SUCCESS: &str = "MUM-ACK-1: rescue crew dispatched.";
const MUM_MISDELIVERED: &str = "MUM-NOPE: this message is not for me!";
const MUM_AVATAR: &str = "data:image/png;base64,AQID";
const GP_TIP: &str = "GP-TIP: somebody inside the town hall wants you: {link}";

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
            // (a misdelivery), which must never earn the informant.
            informant_tip: None,
            missions: vec![Mission {
                to: "grandpa".into(),
                text: MUM_MISSION.into(),
                success: MUM_SUCCESS.into(),
            }],
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
            informant_tip: Some(GP_TIP.into()),
            missions: vec![Mission {
                to: "mum".into(),
                text: GP_MISSION.into(),
                success: GP_SUCCESS.into(),
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
    informant: Option<InformantTip>,
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
        informant,
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
    let mum_bundle = IdentityBundle::generate("mum");
    let gp_bundle = IdentityBundle::generate("grandpa");
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
    // Grandpa's card also carries the informant's identity, like every station
    // card does. In the shipped game it is Mira who tips; here it is grandpa,
    // because he is the one who receives the delivery in this test.
    let informant_bundle = IdentityBundle::generate("anonymous");
    let informant_link = qr::contact_deep_link(&informant_bundle.contact_code().unwrap());
    let informant = InformantTip {
        link: informant_link.clone(),
    };
    let mum_bot = start_bot(mum_dir.path(), &mum_bundle, &cast, &mailbox, None).await;
    let _hosp_bot = start_bot(
        gp_dir.path(),
        &gp_bundle,
        &cast,
        &mailbox,
        Some(informant.clone()),
    )
    .await;

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

    // The delivery earns the informant: his contact arrives as a tappable
    // add-contact deep link, since he has no poster to scan any more. Sent
    // plain, not threaded — it answers nothing, it starts something.
    let expected_tip = GP_TIP.replace("{link}", &informant_link);
    wait_until("grandpa passes on the informant", Duration::from_secs(90), || async {
        messages_of(&p1, gp_chat).await.contains(&expected_tip)
    })
    .await;
    assert!(
        messages_with_replies(&p1, gp_chat)
            .await
            .contains(&(expected_tip.clone(), false)),
        "the informant tip should be its own message, not a reply"
    );
    // The link the player taps really does encode the informant's contact.
    let code = expected_tip
        .rsplit("/add-contact/")
        .next()
        .expect("the tip carries a deep link");
    assert_eq!(
        qr::decode_contact_code(code).unwrap().device_pubkey,
        informant_bundle.device_id().unwrap()
    );
    // Once per player: grandpa's remaining deliveries stay tip-free.
    wait_until("grandpa records the tip", Duration::from_secs(30), || async {
        BotState::load(&gp_dir.path().join("state.json"))
            .tipped
            .contains(&gp_chat.to_string())
    })
    .await;

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
    // The informant tip is NOT a reply: it opens a subject of its own, and
    // this is mum's chat, where no tip may appear at all.
    assert!(
        !messages_of(&p1, mum_chat)
            .await
            .iter()
            .any(|t| t.contains("/add-contact/")),
        "mum has no tip line — a misdelivery must not hand out the informant"
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
    let _ff_bot2 = start_bot(mum_dir.path(), &mum_bundle, &cast, &mailbox, None).await;

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
