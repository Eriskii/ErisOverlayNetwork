//! The ErisAuth plugin in ErisDB: it approves what a device cannot, when the
//! person proves they are there with the passphrase. It keeps nothing of its
//! own between calls: each call brings the capability its manifest names.

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
async fn the_plugin_needs_no_setup_and_keeps_only_a_hash_of_the_passphrase() {
    let core = Core::start().await;
    let (device, _) = core.enroll("Phone", DEVICE).await;
    let ((pairing, code), _) = cut_with(&device).await;
    let (app, _) = core.redeem(&code, "Notes", &["notes:read"]).await;

    let (status, answer) = device.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!((status, answer["error"].clone()), (503, json!("no_passphrase")), "{answer}");
    let (status, answer) = device.call("passphrase", json!({"current": PASSPHRASE, "new": "a new passphrase"})).await;
    assert_eq!((status, answer["error"].clone()), (503, json!("no_passphrase")), "{answer}");

    core.set_passphrase().await;
    let (_, clients) = core.root.get("/v1/clients").await;
    let names: Vec<&Value> = clients["clients"].as_array().unwrap().iter().map(|c| &c["name"]).collect();
    assert_eq!(names, [&json!("Phone")], "the plugin is no installation of its own");

    let (status, items) = core.root.get("/v1/items?facet=erisauth.passphrase").await;
    assert_eq!(status, 200, "{items}");
    let items = items["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let stored = items[0]["body"].to_string();
    assert!(items[0]["body"]["passphrase"].as_str().unwrap().starts_with("$argon2id$"), "{stored}");
    assert!(!stored.contains(PASSPHRASE));
    assert_eq!(items[0]["source"]["user"], "plugin:erisauth", "what it writes says it is the plugin's");
    assert_eq!(device.get("/v1/items?facet=erisauth.passphrase").await.0, 403, "a device cannot read the hash");

    let (status, answer) = device.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 200, "{answer}");
}

#[tokio::test]
async fn a_device_approves_beyond_its_own_authority_with_the_passphrase() {
    let core = Core::start().await;
    core.set_passphrase().await;
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
    core.set_passphrase().await;
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

    let (status, _) = phone.call("unlock", json!({"installation": phone_id})).await;
    assert_eq!(status, 403, "a device does not unlock itself without erisauth:unlock");
    let (admin, _) = core.enroll("Desk", &["erisauth:unlock"]).await;
    let (status, answer) = admin.call("unlock", json!({"installation": phone_id})).await;
    assert_eq!(status, 200, "{answer}");
    let (status, answer) = phone.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 200, "{answer}");
}

#[tokio::test]
async fn the_plugin_approves_only_the_app_the_device_vouched_for() {
    let core = Core::start().await;
    core.set_passphrase().await;
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
    core.set_passphrase().await;
    let (device, _) = core.enroll("Phone", DEVICE).await;
    let ((pairing, code), _) = cut_with(&device).await;
    let (app, _) = core.redeem(&code, "Notes", &["notes:read"]).await;

    let (status, answer) = core.root.call("approve", approve(&pairing, &["notes:read"], &app, PASSPHRASE)).await;
    assert_eq!(status, 403, "{answer}");
    assert_eq!(answer["error"], "not_a_device");
    assert_eq!(status_of(&core, &pairing).await, "requested");
}

#[tokio::test]
async fn the_passphrase_changes_with_the_old_one_or_resets_without_it() {
    let core = Core::start().await;
    core.set_passphrase().await;
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

    // Forgotten: whoever holds erisauth:reset sets a new one without the old.
    let (status, _) = device.call("reset", json!({"new": "set without the old"})).await;
    assert_eq!(status, 403, "an ordinary device does not hold erisauth:reset");
    let (admin, _) = core.enroll("Desk", &["erisauth:reset"]).await;
    let (status, answer) = admin.call("reset", json!({"new": "set without the old"})).await;
    assert_eq!(status, 200, "{answer}");
    let (status, answer) =
        device.call("approve", approve(&pairing, &["notes:read"], &app, "set without the old")).await;
    assert_eq!(status, 200, "{answer}");
}
