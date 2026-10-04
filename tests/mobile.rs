//! ErisAuth inside an Android app: the daemon runs in the app's process, and
//! apps reach it through the app's bound service, which passes their requests
//! to [`erisauth::device::mobile`] along with who Android says is calling.
//! The app's own screens send the daemon the commands its socket takes.

mod support;

use std::time::Duration;

use erisauth::device::mobile::Mobile;
use serde_json::{Value, json};
use support::{App, Core, Device};

async fn phone(core: &Core) -> (Device, Mobile) {
    let device = Device::new();
    device.join(core, "Phone", &["notes:*"]).await;
    let mobile = Mobile::start(device.data.path().join("erisauth"), device.runtime.path().to_path_buf()).await.unwrap();
    (device, mobile)
}

fn caller() -> Value {
    json!({"package": "dev.example.notes", "certificate": "ab".repeat(32), "pid": 4242})
}

/// One command from the app's screens, and its answer.
async fn command(mobile: &Mobile, message: Value) -> Value {
    serde_json::from_str(&mobile.command(&message.to_string()).await).unwrap()
}

#[tokio::test]
async fn an_android_app_signs_in_through_the_service() {
    let core = Core::start().await;
    let (_device, mobile) = phone(&core).await;
    let app = App::new("Notes");

    let opened: Value = serde_json::from_str(
        &mobile.ask(&serde_json::to_string(&app.ask(&["notes:read"])).unwrap(), &caller().to_string()).await,
    )
    .unwrap();
    let id = opened["id"].as_str().expect("an id").to_string();
    let _client = app.redeem(opened["ticket"].as_str().unwrap(), &["notes:read"], app.key).await;

    let answer = tokio::spawn({
        let mobile = mobile.clone();
        let id = id.clone();
        async move { mobile.answer(&id).await }
    });
    let asked: Value = serde_json::from_str(&mobile.asked(&id, Duration::from_secs(30)).await).unwrap();
    assert_eq!(asked["client"], "Notes");
    assert_eq!(asked["package"], "dev.example.notes", "Android's word for who is asking");
    assert_eq!(asked["certificate"], "ab".repeat(32));
    let state = command(&mobile, json!({"op": "state"})).await;
    assert_eq!(state["device"]["name"], "Phone");
    assert_eq!(state["approvals"][0]["id"], id.as_str(), "the screen lists what waits for the person");
    let decided = command(&mobile, json!({"op": "decide", "id": id, "granted": ["notes:read"]})).await;
    assert_eq!(decided, json!({"ok": true}));
    let answer: Value = serde_json::from_str(&answer.await.unwrap()).unwrap();
    assert_eq!(answer, json!({"approved": ["notes:read"]}));
}

#[tokio::test]
async fn an_app_that_goes_away_withdraws_its_request() {
    let core = Core::start().await;
    let (_device, mobile) = phone(&core).await;
    let app = App::new("Notes");
    let opened: Value = serde_json::from_str(
        &mobile.ask(&serde_json::to_string(&app.ask(&["notes:read"])).unwrap(), &caller().to_string()).await,
    )
    .unwrap();
    let id = opened["id"].as_str().unwrap().to_string();
    let _client = app.redeem(opened["ticket"].as_str().unwrap(), &["notes:read"], app.key).await;
    let answer = tokio::spawn({
        let mobile = mobile.clone();
        let id = id.clone();
        async move { mobile.answer(&id).await }
    });
    serde_json::from_str::<Value>(&mobile.asked(&id, Duration::from_secs(30)).await).unwrap();

    mobile.withdraw(&id).await;
    let answer: Value = serde_json::from_str(&answer.await.unwrap()).unwrap();
    assert!(answer["denied"].is_string(), "{answer}");
    let (_, session) = core.root.get(&format!("/v1/pairings/{id}")).await;
    assert_eq!(session["body"]["status"], "denied");
}

#[tokio::test]
async fn an_unjoined_phone_says_so_and_joins_from_its_screen() {
    let core = Core::start().await;
    let device = Device::new();
    let mobile = Mobile::start(device.data.path().join("erisauth"), device.runtime.path().to_path_buf()).await.unwrap();
    assert!(!device.runtime.path().join("erisauth/page").exists(), "the phone serves no web page");
    let app = App::new("Notes");
    let refused: Value = serde_json::from_str(
        &mobile.ask(&serde_json::to_string(&app.ask(&["notes:read"])).unwrap(), &caller().to_string()).await,
    )
    .unwrap();
    assert!(refused["error"].as_str().unwrap().contains("not joined"), "{refused}");
    assert_eq!(command(&mobile, json!({"op": "state"})).await["device"]["joined"], false);

    let ((pairing, code), _) = core.cut_raw().await;
    let ticket = core.ticket(&code);
    let started =
        command(&mobile, json!({"op": "join", "ticket": ticket, "name": "Phone", "ceiling": ["notes:*"]})).await;
    assert_eq!(started, json!({"ok": true}));
    core.await_requested(&pairing).await;
    let (status, approved) = core.root.post(&format!("/v1/pairings/{pairing}/approve"), json!({})).await;
    assert_eq!(status, 200, "{approved}");
    tokio::time::timeout(Duration::from_secs(60), async {
        while command(&mobile, json!({"op": "state"})).await["device"]["joined"] != true {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the phone never joined");
}

/// A new device joins with a ticket the phone shows as a QR code, drawn by
/// the app from the modules the daemon lays out.
#[tokio::test]
async fn the_phone_shows_a_new_device_its_ticket_as_a_qr_code() {
    let core = Core::start().await;
    let (_device, mobile) = phone(&core).await;
    let added = command(&mobile, json!({"op": "add"})).await;
    assert!(added["ticket"].as_str().unwrap().starts_with("erisdb://pair/"), "{added}");
    let side = added["qr"]["side"].as_u64().unwrap() as usize;
    let modules = added["qr"]["modules"].as_str().unwrap();
    assert_eq!(modules.len(), side * side, "one character per module, row by row");
    assert!(modules.chars().all(|c| c == '0' || c == '1'));
    assert!(modules.contains('1') && modules.contains('0'));
    assert_eq!(&modules[..side], "0".repeat(side), "a quiet zone around the code");
}
