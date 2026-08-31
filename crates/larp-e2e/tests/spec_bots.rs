//! Spec-bot end-to-end tests — the side plot, from both ends.
//!
//! 1. The informant: a player scans the hidden QR poster, the bot accepts the
//!    contact request and sends its greeting (the whole secret) into the
//!    direct chat.
//! 2. The mayor: a player sends him the informant's password in his own chat,
//!    and he answers with the collapse — exactly once, however often the chat
//!    is re-scanned.

use std::time::Duration;

use dashchat_node::mailbox::MailboxOperation;
use dashchat_node::testing::TestNode;
use dashchat_node::NodeConfig;
use mailbox_client::mem::MemMailbox;

use larp_bot::bot::build_node;
use larp_bot::identity::IdentityBundle;
use larp_bot::qr;
use larp_bot::spec::{Spec, SpecBot};

const PASSWORD: &str = "ANON-CODE-XYZZY";

fn informant_spec() -> Spec {
    let spec: Spec = toml::from_str(&format!(
        r#"
        name = "Anonymous"
        greeting = [
            "ANON-REVEAL: the mayor lit the fires himself.",
            "ANON-PASS: his password is {PASSWORD}.",
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
        phrase = "{PASSWORD}"
        reply = [
            "MAYOR-CAUGHT: where did you get that word?",
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

    // The hidden character, generated offline; its poster is printed once.
    let bundle = IdentityBundle::generate("anonymous");
    let poster = bundle.contact_code().unwrap();
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
    );
    let _task = tokio::spawn(bot.run_loop(rx));

    // A player scans the poster: this queues a contact request into the
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
    p1.add_contact(qr::decode_contact_code(&poster).unwrap())
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

/// The endgame: the password the informant handed out is delivered to the
/// mayor the same way every mission is — pasted into his chat, sloppily —
/// and he answers once and only once.
#[tokio::test(flavor = "multi_thread")]
async fn mayor_falls_when_a_player_sends_him_the_password() {
    dashchat_node::testing::setup_tracing(&["info"], false);
    let mailbox = MemMailbox::<MailboxOperation>::new();

    let bundle = IdentityBundle::generate("mayor");
    let poster = bundle.contact_code().unwrap();
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
    );
    let _task = tokio::spawn(bot.run_loop(rx));

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
    p1.add_contact(qr::decode_contact_code(&poster).unwrap())
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

    // The player pastes the password in, as sloppily as a phone clipboard
    // makes it: a prefix, mangled case, trailing prose.
    p1.send_message(
        chat,
        format!("anonymous said to send you this:\n  {}\n", PASSWORD.to_lowercase()),
        None,
        None,
    )
    .await
    .expect("p1 sends the password");

    wait_until("the mayor comes apart", Duration::from_secs(90), || async {
        let texts = messages_of(&p1, chat).await;
        collapse.iter().all(|line| texts.contains(line))
    })
    .await;

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
}
