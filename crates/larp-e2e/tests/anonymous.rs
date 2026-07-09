//! Anonymous-informant end-to-end test: a player scans the hidden QR poster,
//! the informant accepts the contact request and whispers its script (the
//! common reveal + this station's variant) into the direct chat.

use std::time::Duration;

use dashchat_node::mailbox::MailboxOperation;
use dashchat_node::testing::TestNode;
use dashchat_node::NodeConfig;
use mailbox_client::mem::MemMailbox;

use larp_bot::anonymous::{AnonymousBot, AnonymousSpec};
use larp_bot::bot::build_node;
use larp_bot::identity::IdentityBundle;
use larp_bot::qr;

fn test_spec() -> AnonymousSpec {
    let spec: AnonymousSpec = toml::from_str(
        r#"
        name = "Anonymous"
        reveal = ["ANON-REVEAL: the mayor lit the fires himself."]
        [variants]
        portal = ["ANON-PORTAL: tap the mayor's head on the town hall page."]
        code = ["ANON-CODE: tap it FIVE times in a row."]
        "#,
    )
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

#[tokio::test(flavor = "multi_thread")]
async fn informant_whispers_after_contact_request() {
    dashchat_node::testing::setup_tracing(&["info"], false);
    let mailbox = MemMailbox::<MailboxOperation>::new();

    // The hidden character, generated offline; its poster is printed once.
    let bundle = IdentityBundle::generate("anonymous");
    let poster = qr::encode_contact_code(&bundle.qr_code().unwrap()).unwrap();
    let spec = test_spec();
    let script = spec.script("portal").unwrap();

    // The informant's station comes up.
    let dir = tempfile::tempdir().unwrap();
    let (node, rx) = build_node(dir.path(), &bundle, NodeConfig::testing())
        .await
        .expect("informant node builds");
    node.mailboxes.register(mailbox.client()).await;
    let bot = AnonymousBot::new(
        node.clone(),
        spec.name.clone(),
        None,
        script.clone(),
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
    let chat = p1.direct_chat_topic(anon_agent);
    wait_until("the script reaches the player", Duration::from_secs(90), || async {
        let texts: Vec<String> = p1
            .get_messages(chat)
            .await
            .map(|msgs| msgs.iter().map(|m| m.content.message().to_string()).collect())
            .unwrap_or_default();
        script.iter().all(|line| texts.contains(line))
    })
    .await;

    // The informant's profile made it across too (the player sees a name,
    // not a bare key).
    wait_until("the informant's profile reaches p1", Duration::from_secs(60), || async {
        p1.local_store
            .get_profile(anon_agent)
            .await
            .ok()
            .flatten()
            .is_some_and(|p| p.name == "Anonymous")
    })
    .await;
}
