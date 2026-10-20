//! The approval page: the daemon serves it on loopback, its files to anyone and its API only
//! to whoever holds the token in the link it writes for `erisauth open`.

mod support;

use serde_json::{Value, json};
use support::Device;

/// The page's address and token, as the daemon writes them for `erisauth open`.
fn page(device: &Device) -> (String, String) {
    use std::os::unix::fs::PermissionsExt;
    let path = device.runtime.path().join("erisauth/page");
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let link = std::fs::read_to_string(path).unwrap();
    let (base, token) = link.trim().split_once("/#token=").unwrap();
    assert!(base.starts_with("http://127.0.0.1:"), "{link}");
    (base.to_string(), token.to_string())
}

#[tokio::test]
async fn the_page_loads_without_the_token_and_its_api_needs_it() {
    let laptop = Device::founder("laptop").await;
    let (base, token) = page(&laptop);
    let http = reqwest::Client::new();

    let index = http.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(index.status(), 200);
    let headers = index.headers().clone();
    assert!(headers["content-security-policy"].to_str().unwrap().contains("frame-ancestors 'none'"));
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["referrer-policy"], "no-referrer");
    let html = index.text().await.unwrap();
    assert!(html.contains("<title>ErisAuth</title>"), "{html}");
    assert!(html.contains("script-src 'self' 'sha256-"), "the page limits its scripts");

    assert_eq!(http.get(format!("{base}/api/state")).send().await.unwrap().status(), 401);
    assert_eq!(http.get(format!("{base}/api/state")).bearer_auth("wrong").send().await.unwrap().status(), 401);
    let command = json!({"op": "add"});
    assert_eq!(http.post(format!("{base}/api/command")).json(&command).send().await.unwrap().status(), 401);

    let state = http.get(format!("{base}/api/state")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(state.headers()["cache-control"], "no-store");
    let state: Value = state.json().await.unwrap();
    assert_eq!(state["device"]["name"], "laptop");
    assert_eq!(state["mesh"]["member"], true);

    let added: Value = http
        .post(format!("{base}/api/command"))
        .bearer_auth(&token)
        .json(&command)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(added["ticket"].as_str().unwrap().starts_with("eris://pair/"), "{added}");
    assert!(added["svg"].as_str().unwrap().contains("<svg"), "{added}");

    // The page answers only what the person's tools ask; an app asks on the socket.
    let posing = json!({"op": "approve", "v": 2, "service": "files", "name": "x", "grants": [], "serves": false,
        "app": iroh::SecretKey::generate().public().to_string()});
    let refused: Value = http
        .post(format!("{base}/api/command"))
        .bearer_auth(&token)
        .json(&posing)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(refused["error"], "bad_request", "{refused}");
}

#[tokio::test]
async fn every_file_the_page_references_is_served() {
    let device = Device::start().await;
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
async fn a_stopped_daemon_takes_its_socket_and_link_with_it() {
    let mut device = Device::start().await;
    let link = device.runtime.path().join("erisauth/page");
    assert!(link.exists() && device.socket().exists());
    device.stop().await;
    assert!(!link.exists() && !device.socket().exists());
    let open = device.command(&["devices"]).output().await.unwrap();
    assert!(!open.status.success());
    assert!(String::from_utf8_lossy(&open.stderr).contains("erisauth serve"), "it says what to run");
}
