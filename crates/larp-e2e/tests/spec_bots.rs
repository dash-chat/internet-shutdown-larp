//! Spec-bot end-to-end tests — the side plot, from both ends.
//!
//! 1. The informant: a player follows the contact link Mira handed them (he
//!    has no poster), the bot accepts the contact request and sends its
//!    greeting — the whole secret — into the direct chat.
//! 2. The mayor: a player sends him the line the informant leaked out of his
//!    own written order, in his own chat, and he answers with the collapse —
//!    exactly once, however often the chat is re-scanned.

use std::time::Duration;

use dashchat_node::mailbox::MailboxOperation;
use dashchat_node::testing::TestNode;
use dashchat_node::NodeConfig;
use mailbox_client::mem::MemMailbox;

use larp_bot::bot::build_node;
use larp_bot::identity::IdentityBundle;
use larp_bot::qr;
use larp_bot::spec::{Spec, SpecBot};

const LEAKED_LINE: &str = "ANON-LINE: let the north side burn";

fn informant_spec() -> Spec {
    let spec: Spec = toml::from_str(&format!(
        r#"
        name = "Anonymous"
        greeting = [
            "ANON-REVEAL: the mayor lit the fires himself.",
            "ANON-LEAK: I copied one line out of his order: {LEAKED_LINE}.",
            "ANON-SEND: paste it into the mayor's own chat.",
        ]
        "#
    ))
    .unwrap();
    spec.lint().unwrap();
    spec
}

fn mayor_spec() -> Spec {
    let spec: Spec = toml::from_str(&format!(
        r#"
        name = "The Mayor"
        greeting = ["MAYOR-GREETING: citizens, scan the four posters."]

        [[triggers]]
        phrase = "{LEAKED_LINE}"
        reply = [
            "MAYOR-CAUGHT: where did you get that sentence?",
            "MAYOR-FLED: the mayor has fled town.",
        ]
        "#
    ))
    .unwrap();
    spec.lint().unwrap();
    spec
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

async fn messages_of(node: &TestNode, chat: dashchat_node::ChatId) -> Vec<String> {
    node.get_messages(chat)
        .await
        .map(|msgs| msgs.iter().map(|m| m.content.message().to_string()).collect())
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn informant_whispers_after_contact_request() {
    dashchat_node::testing::setup_tracing(&["info"], false);
    let mailbox = MemMailbox::<MailboxOperation>::new();

    // The informant, generated offline; his contact code is never printed —
    // Mira hands it out as a deep link.
    let bundle = IdentityBundle::generate("anonymous");
    let contact_code = bundle.contact_code().unwrap();
    let spec = informant_spec();
    let script = spec.greeting.clone();

    // The informant's station comes up.
    let dir = tempfile::tempdir().unwrap();
    let (node, rx) = build_node(dir.path(), &bundle, NodeConfig::testing())
        .await
        .expect("informant node builds");
    node.mailboxes.register(mailbox.client()).await;
    let bot = SpecBot::new(
        node.clone(),
        bundle.device_id().unwrap(),
        spec,
        None,
        Duration::from_secs(1),
        dir.path().join("state.json"),
        None,
    );
    let _task = tokio::spawn(bot.run_loop(rx));

    // A player follows the link: this queues a contact request into the
    // informant's inbox topic through the shared mailbox.
    let p1 = TestNode::new(NodeConfig::testing(), "p1").await;
    p1.add_mailbox_client(mailbox.client()).await;
    p1.set_profile(dashchat_node::Profile {
        name: "Player One".into(),
        surname: None,
        avatar: None,
        about: None,
    })
    .await
    .unwrap();
    p1.add_contact(qr::decode_contact_code(&contact_code).unwrap())
        .await
        .expect("p1 adds the informant");

    // The informant accepts and whispers; the whole script reaches the
    // player's side of the direct chat.
    let anon_agent = bundle.agent_id().unwrap();
    // The direct-chat topic is derived from device ids, not agent ids.
    #[allow(deprecated)]
    let chat = p1.direct_chat_topic(dashchat_node::FakeAgentId::from(
        bundle.device_id().unwrap(),
    ));
    wait_until("the script reaches the player", Duration::from_secs(90), || async {
        let texts = messages_of(&p1, chat).await;
        script.iter().all(|line| texts.contains(line))
    })
    .await;

    // The informant's profile made it across too (the player sees a name,
    // not a bare key).
    wait_until("the informant's profile reaches p1", Duration::from_secs(60), || async {
        p1.projection
            .get_profile(anon_agent)
            .await
            .ok()
            .flatten()
            .is_some_and(|p| p.name == "Anonymous")
    })
    .await;
}

/// The endgame: the line the informant leaked is delivered to the
/// mayor the same way every mission is — pasted into his chat, sloppily —
/// and he answers once and only once.
#[tokio::test(flavor = "multi_thread")]
async fn mayor_falls_when_a_player_sends_him_his_own_words() {
    dashchat_node::testing::setup_tracing(&["info"], false);
    let mailbox = MemMailbox::<MailboxOperation>::new();

    let bundle = IdentityBundle::generate("mayor");
    let contact_code = bundle.contact_code().unwrap();
    let spec = mayor_spec();
    let collapse = spec.triggers[0].reply.clone();

    let dir = tempfile::tempdir().unwrap();
    let (node, rx) = build_node(dir.path(), &bundle, NodeConfig::testing())
        .await
        .expect("mayor node builds");
    node.mailboxes.register(mailbox.client()).await;
    let bot = SpecBot::new(
        node.clone(),
        bundle.device_id().unwrap(),
        spec,
        None,
        Duration::from_secs(1),
        dir.path().join("state.json"),
        None,
    );
    let task = tokio::spawn(bot.run_loop(rx));

    let p1 = TestNode::new(NodeConfig::testing(), "p1").await;
    p1.add_mailbox_client(mailbox.client()).await;
    p1.set_profile(dashchat_node::Profile {
        name: "Player One".into(),
        surname: None,
        avatar: None,
        about: None,
    })
    .await
    .unwrap();
    p1.add_contact(qr::decode_contact_code(&contact_code).unwrap())
        .await
        .expect("p1 adds the mayor");

    #[allow(deprecated)]
    let chat = p1.direct_chat_topic(dashchat_node::FakeAgentId::from(
        bundle.device_id().unwrap(),
    ));

    // Onboarding first: his greeting is how the game explains itself.
    wait_until("the mayor's greeting arrives", Duration::from_secs(90), || async {
        messages_of(&p1, chat)
            .await
            .iter()
            .any(|t| t.contains("MAYOR-GREETING"))
    })
    .await;

    // The player pastes the line in, as sloppily as a phone clipboard
    // makes it: a prefix, mangled case, trailing prose.
    p1.send_message(
        chat,
        format!("anonymous said to send you this:\n  {}\n", LEAKED_LINE.to_lowercase()),
        None,
        None,
    )
    .await
    .expect("p1 sends the leaked line");

    wait_until("the mayor comes apart", Duration::from_secs(90), || async {
        let texts = messages_of(&p1, chat).await;
        collapse.iter().all(|line| texts.contains(line))
    })
    .await;

    // His first line is threaded onto the evidence the player put in front of
    // him; the rest of the unravelling is plain.
    let threaded: Vec<(String, bool)> = p1
        .get_messages(chat)
        .await
        .map(|msgs| {
            msgs.iter()
                .map(|m| (m.content.message().to_string(), m.content.reply().is_some()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        threaded.contains(&(collapse[0].clone(), true)),
        "the mayor's first answer should reply to the pasted line"
    );
    for line in &collapse[1..] {
        assert!(
            threaded.contains(&(line.clone(), false)),
            "only the first line is threaded: {line:?}"
        );
    }

    // The fall is signalled outside the chat too: the flag Nadia's bot polls
    // (same Pi, BotConfig::mayor_fallen_flag) exists once the trigger fired —
    // and names the player who felled him, so her eruption reaches only the
    // courier who earned it.
    let flag = std::fs::read_to_string(dir.path().join("triggered"))
        .expect("the collapse must write the triggered flag for Nadia's eruption");
    assert!(
        flag.split_whitespace().any(|l| l == p1.device_id().to_string()),
        "the flag must name the player who felled the mayor, got {flag:?}"
    );

    // Exactly once: the op hash is remembered, so the bot's repeated scans of
    // the same chat never replay the collapse.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let texts = messages_of(&p1, chat).await;
    for line in &collapse {
        assert_eq!(
            texts.iter().filter(|t| *t == line).count(),
            1,
            "the mayor repeated himself: {line:?}"
        );
    }

    // --- A game-day reset must not replay the endgame. Wipe the mayor's
    // whole data dir (state, node data, flag) and restart him from the
    // bundle: the mailbox still holds tonight's history, including the
    // trigger message, but a fresh-start bot BASELINES what it re-syncs
    // instead of answering it.
    task.abort();
    let _ = task.await;
    node.shutdown().await.expect("mayor node shuts down");
    std::fs::remove_dir_all(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    let (node2, rx2) = build_node(dir.path(), &bundle, NodeConfig::testing())
        .await
        .expect("rebuilt mayor node builds");
    node2.mailboxes.register(mailbox.client()).await;
    let bot2 = SpecBot::new(
        node2.clone(),
        bundle.device_id().unwrap(),
        mayor_spec(),
        None,
        Duration::from_secs(1),
        dir.path().join("state.json"),
        None,
    );
    let _task2 = tokio::spawn(bot2.run_loop(rx2));

    // The baseline is persisted into the fresh state's answered set — once
    // it lands, the re-synced trigger has definitively been swallowed.
    wait_until("the rebuilt mayor baselines the history", Duration::from_secs(90), || async {
        !larp_bot::spec::SpecState::load(&dir.path().join("state.json"))
            .answered
            .is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let texts = messages_of(&p1, chat).await;
    for line in &collapse {
        assert_eq!(
            texts.iter().filter(|t| *t == line).count(),
            1,
            "the wiped mayor replayed his collapse: {line:?}"
        );
    }
    assert!(
        !dir.path().join("triggered").exists(),
        "the wiped mayor re-wrote the triggered flag from old history"
    );
}
