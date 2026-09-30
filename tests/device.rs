//! ErisAuth on a device: it joins ErisDB once, then an app asks it instead
//! of showing a ticket, and the person approves on the device.

mod support;

use std::time::Duration;

use erisdb_client::{Cancel, Pairing};
use erislogin::agent::{self, Answer};
use serde_json::{Value, json};
use support::{App, Core, Device, PASSPHRASE};

/// What a device asks for besides its ceiling.
const BASE: &[&str] =
    &["meta:pairing:create", "meta:pairing:read", "meta:pairing:approve", "erisauth:approve", "erisauth:passphrase"];

async fn collect(client: &erisdb_client::Client) -> Vec<String> {
    match client.await_pairing(Duration::from_secs(30), &Cancel::new()).await.unwrap() {
        Pairing::Approved { granted, .. } => granted,
        other => panic!("the app was not approved: {other:?}"),
    }
}

async fn pairing_status(core: &Core, pairing: &str) -> Value {
    core.root.get(&format!("/v1/pairings/{pairing}")).await.1["body"]["status"].clone()
}

#[tokio::test]
async fn a_device_joins_then_approves_an_app_inside_its_ceiling() {
    let core = Core::start().await;
    let mut device = Device::new();
    let output = device.join(&core, "Laptop", &["notes:read"]).await;
    assert!(String::from_utf8_lossy(&output.stdout).contains("Compare"));

    let (_, clients) = core.root.get("/v1/clients").await;
    let laptop = clients["clients"].as_array().unwrap().iter().find(|c| c["name"] == "ErisAuth on Laptop").unwrap();
    let mut expected: Vec<&str> = BASE.to_vec();
    expected.push("notes:read");
    assert_eq!(laptop["grants"], json!(expected));
    assert_eq!(laptop["identity"]["kind"], "iroh");

    device.serve().await;
    let app = App::new("Notes");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().expect("the daemon listens");
    let ticket = asking.ask(&app.ask(&["notes:read"])).await.unwrap();
    let client = app.redeem(&ticket, &["notes:read"], app.key).await;

    let approval = device.asked().await;
    assert_eq!(approval["client"], "Notes");
    assert_eq!(approval["service"], "erisdb");
    assert_eq!(approval["requested"], json!(["notes:read"]));
    assert_eq!(approval["needs_passphrase"], false);
    assert_eq!(approval["executable"], json!(std::env::current_exe().unwrap()), "the kernel's word, not the app's");

    let decided = device.say(json!({"op": "decide", "id": approval["id"], "granted": ["notes:read"]})).await;
    assert_eq!(decided, json!({"ok": true}));
    assert_eq!(asking.answer().await.unwrap(), Answer::Approved(vec!["notes:read".into()]));
    assert_eq!(collect(&client).await, vec!["notes:read".to_string()]);
    assert!(device.approvals().await.is_empty());
}

#[tokio::test]
async fn beyond_its_ceiling_a_device_asks_home_with_the_passphrase() {
    let core = Core::start().await;
    core.setup_home().await;
    let mut device = Device::new();
    device.join(&core, "Phone", &["notes:read"]).await;
    device.serve().await;

    let app = App::new("Tasks");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["tasks:read", "tasks:create"])).await.unwrap();
    let client = app.redeem(&ticket, &["tasks:read", "tasks:create"], app.key).await;
    let approval = device.asked().await;
    assert_eq!(approval["needs_passphrase"], true);

    let decide = |passphrase: Option<&str>| json!({"op": "decide", "id": approval["id"], "granted": ["tasks:read"], "passphrase": passphrase});
    assert_eq!(device.say(decide(None)).await["error"], "needs_passphrase");
    assert_eq!(device.say(decide(Some("guess"))).await["error"], "wrong_passphrase");
    assert_eq!(device.approvals().await.len(), 1, "a wrong passphrase leaves the question open");
    assert_eq!(device.say(decide(Some(PASSPHRASE))).await, json!({"ok": true}));

    assert_eq!(asking.answer().await.unwrap(), Answer::Approved(vec!["tasks:read".into()]));
    assert_eq!(collect(&client).await, vec!["tasks:read".to_string()], "the person chose part of the request");
}

#[tokio::test]
async fn a_different_key_redeeming_the_ticket_is_turned_away_before_anyone_is_asked() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Laptop", &["*"]).await;
    device.serve().await;

    let app = App::new("Notes");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["notes:read"])).await.unwrap();
    let _intruder = app.redeem(&ticket, &["notes:read"], rand::random()).await;

    match asking.answer().await.unwrap() {
        Answer::Denied(why) => assert!(why.contains("different"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(device.approvals().await.is_empty(), "the person is never asked");
    let (_, pairings) = core.root.get("/v1/pairings").await;
    let latest = pairings["pairings"][0]["id"].as_str().unwrap();
    assert_eq!(pairing_status(&core, latest).await, "denied");
}

#[tokio::test]
async fn an_app_that_hangs_up_withdraws_its_request() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Laptop", &["*"]).await;
    device.serve().await;

    let app = App::new("Notes");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["notes:read"])).await.unwrap();
    let _client = app.redeem(&ticket, &["notes:read"], app.key).await;
    let approval = device.asked().await;
    drop(asking);

    tokio::time::timeout(Duration::from_secs(10), async {
        while !device.approvals().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the request was not withdrawn");
    let pairing = approval["id"].as_str().unwrap();
    assert_eq!(pairing_status(&core, pairing).await, "denied");
}

#[tokio::test]
async fn the_person_answers_at_a_terminal_or_says_no() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Laptop", &["*"]).await;
    device.serve().await;

    let app = App::new("Notes");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["notes:read", "notes:delete"])).await.unwrap();
    let client = app.redeem(&ticket, &["notes:read", "notes:delete"], app.key).await;
    device.asked().await;

    // The terminal prompt answers the first request: select only the first grant.
    // What is typed before a request is shown is not an answer, so answer once it is.
    let mut prompt = device.command(&["approvals", "--once"]).spawn().unwrap();
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let mut stdout = BufReader::new(prompt.stdout.take().unwrap());
    let mut shown = String::new();
    while !shown.contains("[a] approve") {
        assert_ne!(stdout.read_line(&mut shown).await.unwrap(), 0, "the prompt ended early: {shown}");
    }
    let mut stdin = prompt.stdin.take().unwrap();
    stdin.write_all(b"s\n1\n").await.unwrap();
    drop(stdin);
    stdout.read_to_string(&mut shown).await.unwrap();
    let output = tokio::time::timeout(Duration::from_secs(30), prompt.wait_with_output()).await.unwrap().unwrap();
    assert!(output.status.success(), "{shown}{}", String::from_utf8_lossy(&output.stderr));
    assert!(shown.contains("Approved: notes:read"), "{shown}");
    assert!(shown.contains("Notes asks to use ErisDB") && shown.contains("notes:delete"), "{shown}");
    assert_eq!(asking.answer().await.unwrap(), Answer::Approved(vec!["notes:read".into()]));
    assert_eq!(collect(&client).await, vec!["notes:read".to_string()]);

    // And a plain no.
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["notes:read"])).await.unwrap();
    let _client = app.redeem(&ticket, &["notes:read"], app.key).await;
    let approval = device.asked().await;
    assert_eq!(device.say(json!({"op": "deny", "id": approval["id"]})).await, json!({"ok": true}));
    assert!(matches!(asking.answer().await.unwrap(), Answer::Denied(_)));
}

#[tokio::test]
async fn an_app_on_a_device_that_has_not_joined_hears_why() {
    let mut device = Device::new();
    device.serve().await;
    let app = App::new("Notes");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let error = asking.ask(&app.ask(&["notes:read"])).await.unwrap_err().to_string();
    assert!(error.contains("erisauth join"), "{error}");
}

#[tokio::test]
async fn beyond_its_ceiling_without_home_the_person_hears_why() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Phone", &["notes:read"]).await;
    device.serve().await;

    let app = App::new("Tasks");
    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["tasks:read"])).await.unwrap();
    let _client = app.redeem(&ticket, &["tasks:read"], app.key).await;
    let approval = device.asked().await;
    let answer = device
        .say(json!({"op": "decide", "id": approval["id"], "granted": ["tasks:read"], "passphrase": PASSPHRASE}))
        .await;
    assert_eq!(answer["error"], "no_home", "{answer}");
    assert!(answer["message"].as_str().unwrap().contains("erisauth home setup"), "{answer}");
    assert_eq!(device.approvals().await.len(), 1, "the request stays open");
}
