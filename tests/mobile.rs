//! ErisAuth inside the Android app: the daemon runs in the app's process. Apps reach it through
//! the app's bound service, which hands each request line to [`Mobile::request`] with who
//! Android says is calling; the app's own screens send it the commands the socket takes.

mod support;

use std::time::Duration;

use erisauth::device::mobile::Mobile;
use erisoverlaynetwork::agent::Reply;
use erisoverlaynetwork::net::Network;
use serde_json::{Value, json};
use support::{Device, PATIENCE, device_names};

async fn phone() -> (tempfile::TempDir, Mobile) {
    let data = tempfile::tempdir().unwrap();
    let mobile = Mobile::start(data.path().join("erisauth"), Network::Loopback).await.unwrap();
    (data, mobile)
}

/// One command from the app's screens, and its answer.
async fn command(mobile: &Mobile, message: Value) -> Value {
    serde_json::from_str(&mobile.command(&message.to_string()).await).unwrap()
}

async fn until(mobile: &Mobile, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(PATIENCE, async {
        loop {
            let state = command(mobile, json!({"op": "state"})).await;
            if ok(&state) {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never: {what}"))
}

fn caller() -> String {
    json!({"package": "dev.example.notes", "certificate": "ab".repeat(32), "pid": 4242}).to_string()
}

fn approve(app: &iroh::SecretKey) -> String {
    json!({"op": "approve", "v": 2, "service": "notes", "name": "Notes", "grants": ["notes:read"], "serves": false,
        "app": app.public().to_string()})
    .to_string()
}

#[tokio::test]
async fn an_android_app_is_approved_as_android_names_it() {
    let (_data, mobile) = phone().await;
    assert!(command(&mobile, json!({"op": "state"})).await["mesh"].is_null());
    assert_eq!(command(&mobile, json!({"op": "init", "name": "phone"})).await["ok"], true);

    let key = iroh::SecretKey::generate();
    let asking = tokio::spawn({
        let mobile = mobile.clone();
        let line = approve(&key);
        async move { mobile.request("10123/1", &line, &caller()).await }
    });
    let state =
        until(&mobile, "the request reaches the person", |s| !s["approvals"].as_array().unwrap().is_empty()).await;
    let prompt = &state["approvals"][0];
    assert_eq!(prompt["package"], "dev.example.notes", "Android's word for who asks");
    assert_eq!(prompt["certificate"], "ab".repeat(32));
    assert_eq!(prompt["program"], format!("dev.example.notes sha256:{}", "ab".repeat(32)));
    assert_eq!(command(&mobile, json!({"op": "decide", "id": prompt["id"]})).await, json!({"ok": true}));

    let answered: Reply = serde_json::from_str(&asking.await.unwrap()).unwrap();
    let Reply::Approved { note, snapshot } = answered else { panic!("{answered:?}") };
    assert_eq!(note.body.program.as_deref(), Some(format!("dev.example.notes sha256:{}", "ab".repeat(32)).as_str()));
    assert_eq!(note.app(), key.public());
    note.check(&snapshot.list().unwrap()).unwrap();

    let read: Value =
        serde_json::from_str(&mobile.request("10123/2", r#"{"op":"snapshot","v":2}"#, &caller()).await).unwrap();
    assert_eq!(read["snapshot"]["mesh"], state["mesh"]["id"]);
}

#[tokio::test]
async fn an_android_app_that_goes_away_withdraws_its_request() {
    let (_data, mobile) = phone().await;
    command(&mobile, json!({"op": "init", "name": "phone"})).await;
    let asking = tokio::spawn({
        let mobile = mobile.clone();
        let line = approve(&iroh::SecretKey::generate());
        async move { mobile.request("10123/7", &line, &caller()).await }
    });
    until(&mobile, "the request reaches the person", |s| !s["approvals"].as_array().unwrap().is_empty()).await;
    mobile.withdraw("10123/7");
    let answered: Value = serde_json::from_str(&asking.await.unwrap()).unwrap();
    assert!(answered["error"].is_string(), "{answered}");
    until(&mobile, "the request is gone", |s| s["approvals"].as_array().unwrap().is_empty()).await;

    // The service hands requests on from threads of their own, so a withdrawal can come first.
    mobile.withdraw("10123/8");
    let line = approve(&iroh::SecretKey::generate());
    let answered: Value = serde_json::from_str(&mobile.request("10123/8", &line, &caller()).await).unwrap();
    assert!(answered["error"].is_string(), "{answered}");
    assert!(command(&mobile, json!({"op": "state"})).await["approvals"].as_array().unwrap().is_empty());
}

/// The phone joins a computer's mesh from its screen, and shows the next device a ticket as a QR
/// code, drawn by the app from the modules the daemon lays out.
#[tokio::test]
async fn a_phone_joins_from_its_screen_and_shows_the_next_device_a_qr_code() {
    let laptop = Device::founder("laptop").await;
    let (_data, mobile) = phone().await;
    let (session, ticket) = laptop.add().await;
    let joined = command(&mobile, json!({"op": "join", "ticket": ticket, "name": "phone"})).await;
    assert_eq!(laptop.prompt().await["fingerprint"], joined["fingerprint"]);
    assert_eq!(laptop.say(json!({"op": "decide", "id": session})).await, json!({"ok": true}));
    let state = until(&mobile, "the phone is a member", |s| s["mesh"]["member"] == true).await;
    assert_eq!(device_names(&state), ["laptop", "phone"]);

    let added = command(&mobile, json!({"op": "add"})).await;
    assert!(added["ticket"].as_str().unwrap().starts_with("eris://pair/"), "{added}");
    let side = added["qr"]["side"].as_u64().unwrap() as usize;
    let modules = added["qr"]["modules"].as_str().unwrap();
    assert_eq!(modules.len(), side * side, "one character per module, row by row");
    assert!(modules.chars().all(|c| c == '0' || c == '1'));
    assert_eq!(&modules[..side], "0".repeat(side), "a quiet zone around the code");
}
