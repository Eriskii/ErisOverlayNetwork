//! The approval page: the daemon serves it on loopback, and the person
//! answers there as at the terminal.

mod support;

use erislogin::agent::{self, Answer};
use serde_json::json;
use support::{App, Core, Device};

/// The page's address and token, as the daemon writes them for `erisauth open`.
fn page(device: &Device) -> (String, String) {
    let path = device.runtime.path().join("erisauth/page");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let link = std::fs::read_to_string(path).unwrap();
    let (base, token) = link.trim().split_once("/#token=").unwrap();
    assert!(base.starts_with("http://127.0.0.1:"), "{link}");
    (base.to_string(), token.to_string())
}

#[tokio::test]
async fn the_page_loads_without_the_token_and_its_api_needs_it() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Laptop", &["notes:*"]).await;
    device.serve().await;
    let (base, token) = page(&device);
    let http = reqwest::Client::new();

    let index = http.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(index.status(), 200);
    assert!(index.headers()["content-security-policy"].to_str().unwrap().contains("frame-ancestors 'none'"));
    let html = index.text().await.unwrap();
    assert!(html.contains("<title>ErisAuth</title>"), "{html}");
    assert!(html.contains("script-src 'self' 'sha256-"), "the page limits its scripts");

    assert_eq!(http.get(format!("{base}/api/state")).send().await.unwrap().status(), 401);
    assert_eq!(http.get(format!("{base}/api/state")).bearer_auth("wrong").send().await.unwrap().status(), 401);
    let state: serde_json::Value =
        http.get(format!("{base}/api/state")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(state["device"]["name"], "Laptop");
    assert_eq!(state["device"]["joined"], true);
    assert!(state["device"]["grants"].as_array().unwrap().contains(&json!("notes:*")), "{state}");
    assert_eq!(state["approvals"], json!([]));
}

#[tokio::test]
async fn the_person_approves_and_denies_on_the_page() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Laptop", &["notes:*"]).await;
    device.serve().await;
    let (base, token) = page(&device);
    let http = reqwest::Client::new();
    let app = App::new("Notes");

    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["notes:read", "notes:delete"])).await.unwrap();
    let _client = app.redeem(&ticket, &["notes:read", "notes:delete"], app.key).await;
    let id = device.asked().await["id"].as_str().unwrap().to_string();

    let state: serde_json::Value =
        http.get(format!("{base}/api/state")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(state["approvals"][0]["id"], id);
    let approve = format!("{base}/api/approvals/{id}/approve");
    assert_eq!(http.post(&approve).json(&json!({"granted": ["notes:read"]})).send().await.unwrap().status(), 401);
    let answer: serde_json::Value = http
        .post(&approve)
        .bearer_auth(&token)
        .json(&json!({"granted": ["notes:read"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(answer, json!({"ok": true}));
    assert_eq!(asking.answer().await.unwrap(), Answer::Approved(vec!["notes:read".into()]));

    let mut asking = agent::connect_to(&device.socket()).await.unwrap().unwrap();
    let ticket = asking.ask(&app.ask(&["notes:read"])).await.unwrap();
    let _client = app.redeem(&ticket, &["notes:read"], app.key).await;
    let id = device.asked().await["id"].as_str().unwrap().to_string();
    let answer: serde_json::Value = http
        .post(format!("{base}/api/approvals/{id}/deny"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(answer, json!({"ok": true}));
    assert!(matches!(asking.answer().await.unwrap(), Answer::Denied(_)));
}

#[tokio::test]
async fn every_file_the_page_references_is_served() {
    let mut device = Device::new();
    device.serve().await;
    let (base, _) = page(&device);
    let html = reqwest::get(format!("{base}/")).await.unwrap().text().await.unwrap();
    let files: Vec<&str> =
        html.split("\"./").skip(1).filter_map(|rest| rest.split('"').next()).filter(|p| p.starts_with("ui/")).collect();
    assert!(files.iter().any(|f| f.ends_with(".js")), "{html}");
    for file in files {
        let response = reqwest::get(format!("{base}/{file}")).await.unwrap();
        assert_eq!(response.status(), 200, "{file}");
    }
    assert_eq!(reqwest::get(format!("{base}/ui/missing.js")).await.unwrap().status(), 404);
}

#[tokio::test]
async fn the_page_shows_a_ticket_for_a_new_device() {
    let core = Core::start().await;
    let mut device = Device::new();
    device.join(&core, "Laptop", &["*"]).await;
    device.serve().await;
    let (base, token) = page(&device);
    let http = reqwest::Client::new();
    assert_eq!(http.post(format!("{base}/api/devices")).send().await.unwrap().status(), 401);
    let added: serde_json::Value =
        http.post(format!("{base}/api/devices")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert!(added["ticket"].as_str().unwrap().starts_with("erisdb://pair/"), "{added}");
    assert!(added["svg"].as_str().unwrap().contains("<svg"), "{added}");
    assert!(added["expires"].as_i64().unwrap() > 0);
}
