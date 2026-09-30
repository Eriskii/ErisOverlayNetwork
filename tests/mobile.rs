//! ErisAuth inside an Android app: the daemon runs in the app's process, and
//! apps reach it through the app's bound service, which passes their requests
//! to [`erisauth::device::mobile`] along with who Android says is calling.

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

/// Approve through the page, as the person does in the app's WebView.
async fn approve_on_page(mobile: &Mobile, id: &str, granted: &[&str]) -> Value {
    let (base, token) = mobile.link().split_once("/#token=").map(|(b, t)| (b.to_string(), t.to_string())).unwrap();
    reqwest::Client::new()
        .post(format!("{base}/api/approvals/{id}/approve"))
        .bearer_auth(token)
        .json(&json!({"granted": granted}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
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
    assert_eq!(approve_on_page(&mobile, &id, &["notes:read"]).await, json!({"ok": true}));
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
async fn an_unjoined_phone_says_so_and_joins_from_its_page() {
    let device = Device::new();
    let mobile = Mobile::start(device.data.path().join("erisauth"), device.runtime.path().to_path_buf()).await.unwrap();
    let app = App::new("Notes");
    let refused: Value = serde_json::from_str(
        &mobile.ask(&serde_json::to_string(&app.ask(&["notes:read"])).unwrap(), &caller().to_string()).await,
    )
    .unwrap();
    assert!(refused["error"].as_str().unwrap().contains("not joined"), "{refused}");
    assert!(mobile.link().starts_with("http://127.0.0.1:"));
}
