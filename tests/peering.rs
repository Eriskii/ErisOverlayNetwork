//! Devices in ErisAuth become ErisFiles peers of each other with no further
//! ceremony: approving a device into ErisAuth is the approval.

mod support;

use std::time::Duration;

use erisfiles::client::Client as Files;
use erisfiles::daemon::{Config, Daemon, Network};
use serde_json::{Value, json};
use support::{Caller, Core, Device};

struct Node {
    daemon: Daemon,
    files: Files,
    home: tempfile::TempDir,
}

async fn erisfiles(name: &str) -> Node {
    let home = tempfile::tempdir().unwrap();
    let daemon =
        Daemon::start(Config::new(home.path().to_path_buf(), Some(name.into()), 0, Network::Loopback)).await.unwrap();
    let files = Files::new(&daemon.url(), daemon.token());
    Node { daemon, files, home }
}

async fn peers(node: &Node) -> Vec<String> {
    node.files.devices().await.unwrap().into_iter().filter(|d| !d.this).map(|d| d.id).collect()
}

async fn installation(core: &Core, name: &str) -> String {
    let (_, clients) = core.root.get("/v1/clients").await;
    clients["clients"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn items(core: &Core, facet: &str) -> Vec<Value> {
    let (status, body) = core.root.get(&format!("/v1/items?facet={facet}")).await;
    assert_eq!(status, 200, "{body}");
    body["items"].as_array().unwrap().clone()
}

#[tokio::test]
async fn devices_in_erisauth_become_erisfiles_peers() {
    let core = Core::start().await;
    let laptop_files = erisfiles("laptop").await;
    let phone_files = erisfiles("phone").await;
    let mut laptop = Device::new();
    laptop.join(&core, "Laptop", &["*"]).await;
    laptop.serve_with(&[("ERISFILES_HOME", laptop_files.home.path())]).await;
    let mut phone = Device::new();
    phone.join(&core, "Phone", &["notes:read"]).await;
    phone.serve_with(&[("ERISFILES_HOME", phone_files.home.path())]).await;

    let laptop_id = laptop_files.daemon.id().to_string();
    let phone_id = phone_files.daemon.id().to_string();
    tokio::time::timeout(Duration::from_secs(60), async {
        while peers(&laptop_files).await != [phone_id.clone()] || peers(&phone_files).await != [laptop_id.clone()] {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the two devices never became ErisFiles peers");

    let devices = items(&core, "erisauth.devices").await;
    assert_eq!(devices.len(), 2, "{devices:?}");
    for device in &devices {
        assert_eq!(device["body"]["installation"], device["source"]["installation"], "each device announces itself");
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        while !items(&core, "erisauth.mail").await.is_empty() {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("an invitation was left behind");
}

#[tokio::test]
async fn an_invitation_not_from_an_erisauth_device_is_never_joined() {
    let core = Core::start().await;
    let phone_files = erisfiles("phone").await;
    let mut phone = Device::new();
    phone.join(&core, "Phone", &["notes:read"]).await;
    phone.serve_with(&[("ERISFILES_HOME", phone_files.home.path())]).await;
    let phone_installation = installation(&core, "ErisAuth on Phone").await;
    // The phone announces itself, registering the facets as it does.
    tokio::time::timeout(Duration::from_secs(30), async {
        while items(&core, "erisauth.devices").await.is_empty() {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the phone never announced itself");

    // Someone who may post invitations, but is no ErisAuth device, invites
    // the phone to their own ErisFiles: once in their own name, once
    // claiming to be the phone itself.
    let (mallory, mallory_installation) = core.enroll("Mallory", &["erisauth.mail:*", "erisauth.devices:read"]).await;
    let mallory_files = erisfiles("mallory").await;
    let mut sessions = Vec::new();
    for from in [mallory_installation.as_str(), phone_installation.as_str()] {
        let opened = mallory_files.files.open_pairing(None).await.unwrap();
        sessions.push(opened.id);
        let mail = json!({"to": phone_installation, "from": from, "kind": "erisfiles", "ticket": opened.ticket});
        let (status, body) = mallory.post("/v1/items", json!({"facet": "erisauth.mail", "body": mail})).await;
        assert_eq!(status, 201, "{body}");
    }

    tokio::time::timeout(Duration::from_secs(30), async {
        while !items(&core, "erisauth.mail").await.is_empty() {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the phone never dropped the forged invitations");
    for session in sessions {
        let status = mallory_files.files.pairing(session).await.unwrap().status;
        assert_eq!(status, erislogin::pairing::Status::Pending, "the phone redeemed a forged invitation");
    }
    assert!(peers(&phone_files).await.is_empty());
    let _: &Caller = &mallory;
}
