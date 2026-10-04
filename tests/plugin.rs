//! The ErisAuth plugin in ErisDB: it approves what a device cannot, when the
//! person proves they are there with the passphrase.

mod support;

use serde_json::{Value, json};
use support::{Caller, Core, PASSPHRASE, cut_with};

/// What an ErisAuth device holds when it approves nothing itself.
const DEVICE: &[&str] = &["meta:pairing:create", "meta:pairing:read", "erisauth:approve", "erisauth:passphrase"];

fn approve(pairing: &str, granted: &[&str], app: &Caller, passphrase: &str) -> Value {
    json!({"pairing": pairing, "granted": granted, "identity": app.identity(), "passphrase": passphrase})
}

async fn status_of(core: &Core, pairing: &str) -> Value {
    core.root.get(&format!("/v1/pairings/{pairing}")).await.1["body"]["status"].clone()
}

#[tokio::test]
async fn setup_enrolls_the_plugin_and_keeps_only_a_hash_of_the_passphrase() {
    let core = Core::start().await;
    let output = core.setup_plugin().await;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Compare"), "setup shows its fingerprint: {stdout}");

    let (_, clients) = core.root.get("/v1/clients").await;
    let plugin: Vec<&Value> =
        clients["clients"].as_array().unwrap().iter().filter(|c| c["name"] == "ErisAuth").collect();
    assert_eq!(plugin.len(), 1, "{clients}");
    assert_eq!(plugin[0]["grants"], json!(["*"]));

    let (status, items) = core.root.get("/v1/items?facet=erisauth.passphrase").await;
    assert_eq!(status, 200, "{items}");
    let items = items["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let stored = items[0]["body"].to_string();
    assert!(items[0]["body"]["passphrase"].as_str().unwrap().starts_with("$argon2id$"), "{stored}");
    assert!(!stored.contains(PASSPHRASE));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let credential = std::fs::metadata(core.plugin_dir.join("erisdb.json")).unwrap();
        assert_eq!(credential.permissions().mode() & 0o777, 0o600);
    }

    let (device, _) = core.enroll("Phone", DEVICE).await;
    assert_eq!(device.get("/v1/items?facet=erisauth.passphrase").await.0, 403, "a device cannot read the hash");
}

#[tokio::test]
async fn a_device_approves_beyond_its_own_authority_with_the_passphrase() {
    let core = Core::start().await;
    core.setup_plugin().await;
    let (device, _) = core.enroll("Phone", DEVICE).await;
    let ((pairing, code), _) = cut_with(&device).await;
    let (app, _) = core.redeem(&code, "Notes", &["notes:read"]).await;

    let (status, _) = device.post(&format!("/v1/pairings/{pairing}/approve"), json!({"granted": ["notes:read"]})).await;
    assert_eq!(status, 403, "the device cannot approve this on its own");

    let (status, answer) = device.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer, json!({"approved": ["notes:read"]}));

    let (app, _) = core.collect(app).await;
    let (status, permissions) = app.get("/v1/permissions").await;
    assert_eq!(status, 200);
    assert_eq!(permissions["grants"], json!(["notes:read"]));
}

#[tokio::test]
async fn wrong_passphrases_lock_that_device_out_and_only_that_device() {
    let core = Core::start().await;
    core.setup_plugin().await;
    let (phone, phone_id) = core.enroll("Phone", DEVICE).await;
    let ((pairing, code), _) = cut_with(&phone).await;
    let (app, _) = core.redeem(&code, "Notes", &["notes:read"]).await;

    for attempt in 1..=3 {
        let (status, answer) = phone.call("approve", approve(&pairing, &["notes:read"], &app, "guess")).await;
        assert_eq!(status, 403, "attempt {attempt}: {answer}");
        assert_eq!(answer["error"], "wrong_passphrase");
    }
    assert_eq!(status_of(&core, &pairing).await, "requested");

    let (status, answer) = phone.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 429, "locked even with the right passphrase: {answer}");
    let wait = answer["retry_after"].as_i64().unwrap();
    assert!((1..=60).contains(&wait), "{answer}");
    assert_eq!(status_of(&core, &pairing).await, "requested");

    let (laptop, _) = core.enroll("Laptop", DEVICE).await;
    let ((other, code), _) = cut_with(&laptop).await;
    let (other_app, _) = core.redeem(&code, "Tasks", &["tasks:read"]).await;
    let (status, answer) = laptop.call("approve", approve(&other, &["tasks:read"], &other_app, PASSPHRASE)).await;
    assert_eq!(status, 200, "another device is not locked: {answer}");

    let output = core.run(&["unlock", &phone_id]).await;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let (status, answer) = phone.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 200, "{answer}");
}

#[tokio::test]
async fn the_plugin_approves_only_the_app_the_device_vouched_for() {
    let core = Core::start().await;
    core.setup_plugin().await;
    let (device, _) = core.enroll("Phone", DEVICE).await;
    let ((pairing, code), _) = cut_with(&device).await;
    let (_raced, _) = core.redeem(&code, "Notes", &["notes:read"]).await;
    let expected = Caller::new(&core.base, "unused");

    let (status, answer) = device.call("approve", approve(&pairing, &["notes:read"], &expected, PASSPHRASE)).await;
    assert_eq!(status, 409, "{answer}");
    assert_eq!(answer["error"], "not_the_app");
    assert_eq!(status_of(&core, &pairing).await, "requested");
}

#[tokio::test]
async fn a_caller_that_is_not_an_installation_is_refused() {
    let core = Core::start().await;
    core.setup_plugin().await;
    let (device, _) = core.enroll("Phone", DEVICE).await;
    let ((pairing, code), _) = cut_with(&device).await;
    let (app, _) = core.redeem(&code, "Notes", &["notes:read"]).await;

    let (status, answer) = core.root.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 403, "{answer}");
    assert_eq!(answer["error"], "not_a_device");
    assert_eq!(status_of(&core, &pairing).await, "requested");
}

#[tokio::test]
async fn the_passphrase_changes_from_a_device_or_on_the_server() {
    let core = Core::start().await;
    core.setup_plugin().await;
    let (device, _) = core.enroll("Phone", DEVICE).await;

    let (status, _) = device.call("passphrase", json!({"current": PASSPHRASE, "new": "short"})).await;
    assert_eq!(status, 422, "a new passphrase has at least eight characters");
    let (status, answer) = device.call("passphrase", json!({"current": "guess", "new": "a new passphrase"})).await;
    assert_eq!((status, answer["error"].clone()), (403, json!("wrong_passphrase")));
    let (status, answer) = device.call("passphrase", json!({"current": PASSPHRASE, "new": "a new passphrase"})).await;
    assert_eq!(status, 200, "{answer}");

    let ((pairing, code), _) = cut_with(&device).await;
    let (app, _) = core.redeem(&code, "Notes", &["notes:read"]).await;
    let (status, _) = device.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 403, "the old passphrase no longer works");

    // Forgotten: whoever holds the server sets a new one without the old.
    let mut child = core.erisauth(&["passphrase"]);
    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"set on the server\n").await.unwrap();
    drop(stdin);
    let output = child.wait_with_output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let (status, answer) = device.call("approve", approve(&pairing, &["notes:read"], &app, "set on the server")).await;
    assert_eq!(status, 200, "{answer}");
}
