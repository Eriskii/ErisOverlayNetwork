//! A person's devices as one mesh: started on one device, joined by ticket through any device
//! already in it, renamed, and removed with the devices they added.

mod support;

use serde_json::{Value, json};
use support::{Device, device_named, device_names, events};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn three(state: &Value) -> bool {
    state["mesh"]["devices"].as_array().is_some_and(|d| d.len() == 3)
}

#[tokio::test]
async fn devices_join_through_one_another_and_all_know_each_other() {
    let laptop = Device::founder("laptop").await;
    let founded = laptop.state().await;
    assert_eq!(founded["mesh"]["member"], true);
    assert_eq!(device_named(&founded, "laptop")["this"], true);
    assert!(device_named(&founded, "laptop")["added_by"].is_null(), "the founder was added by no one");

    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    // The desktop only ever meets the phone.
    let desktop = Device::start().await;
    desktop.join_through(&phone, "desktop").await;

    let mesh = founded["mesh"]["id"].clone();
    for device in [&laptop, &phone, &desktop] {
        let state = device.until("every device lists all three", three).await;
        assert_eq!(state["mesh"]["id"], mesh, "one mesh");
        assert_eq!(device_names(&state), ["desktop", "laptop", "phone"]);
        assert_eq!(device_named(&state, "desktop")["added_by_name"], "phone");
        assert_eq!(device_named(&state, "phone")["added_by_name"], "laptop");
    }

    // The laptop learns where the desktop is, and reaches it, though they never met.
    let state = laptop
        .until("the laptop reaches the desktop", |s| three(s) && device_named(s, "desktop")["reachable"] == true)
        .await;
    assert_eq!(device_named(&state, "desktop")["id"], desktop.id().await);
    // And the person at the laptop hears that the phone added the desktop.
    let state =
        laptop.until("the laptop hears of the desktop", |s| events(s).contains(&"phone added desktop".into())).await;
    assert!(!events(&state).iter().any(|e| e.contains("added phone")), "a device does not tell its person what it did");
}

#[tokio::test]
async fn a_device_is_started_joined_and_added_at_the_terminal() {
    let laptop = Device::start().await;
    let init = laptop.command(&["init", "--name", "laptop"]).output().await.unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));

    // `erisauth add` shows the ticket, then the newcomer's fingerprint and name, and asks.
    let mut add = laptop.command(&["add"]).spawn().unwrap();
    let mut shown = BufReader::new(add.stdout.take().unwrap());
    let mut line = String::new();
    let ticket = loop {
        line.clear();
        assert_ne!(shown.read_line(&mut line).await.unwrap(), 0, "add ended before it showed a ticket");
        if let Some(at) = line.find("eris://pair/") {
            break line[at..].split_whitespace().next().unwrap().trim_matches('\'').to_string();
        }
    };

    let phone = Device::start().await;
    let mut join = phone.command(&["join", "--name", "phone", &ticket]).spawn().unwrap();
    let mut joining = BufReader::new(join.stdout.take().unwrap());
    let fingerprint = loop {
        line.clear();
        assert_ne!(joining.read_line(&mut line).await.unwrap(), 0, "join ended before it showed a code");
        if line.contains("Compare") {
            break line.trim().rsplit(' ').next().unwrap().to_string();
        }
    };

    let mut asked = String::new();
    while !asked.contains("[y/N]") {
        assert_ne!(shown.read_line(&mut asked).await.unwrap(), 0, "add ended before it asked: {asked}");
    }
    assert!(asked.contains(&fingerprint), "the same code on both screens: {asked}");
    assert!(asked.contains("phone"), "{asked}");
    add.stdin.take().unwrap().write_all(b"y\n").await.unwrap();
    let mut rest = String::new();
    shown.read_to_string(&mut rest).await.unwrap();
    assert!(add.wait().await.unwrap().success(), "{rest}");

    let mut said = String::new();
    joining.read_to_string(&mut said).await.unwrap();
    assert!(join.wait().await.unwrap().success(), "{said}");
    assert!(said.contains("Joined"), "{said}");
    let state = phone.state().await;
    assert_eq!(state["mesh"]["member"], true);
    assert_eq!(device_names(&state), ["laptop", "phone"]);

    let devices = phone.command(&["devices"]).output().await.unwrap();
    let listed = String::from_utf8_lossy(&devices.stdout);
    assert!(listed.contains("laptop") && listed.contains("phone (this device)"), "{listed}");
}

#[tokio::test]
async fn a_denied_device_hears_so_and_stays_out() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    let (session, ticket) = laptop.add().await;
    let joined = phone.say(json!({"op": "join", "ticket": ticket, "name": "phone"})).await;
    assert!(joined["fingerprint"].is_string(), "{joined}");
    laptop.prompt().await;
    assert_eq!(laptop.say(json!({"op": "deny", "id": session})).await, json!({"ok": true}));

    let state = phone.until("the phone hears it was denied", |s| s["joining"]["error"].is_string()).await;
    assert!(state["joining"]["error"].as_str().unwrap().contains("denied"), "{state}");
    assert!(state["mesh"].is_null());
    assert_eq!(device_names(&laptop.state().await), ["laptop"]);
    assert!(laptop.approvals().await.is_empty());
}

#[tokio::test]
async fn a_device_in_a_mesh_neither_starts_nor_joins_another() {
    let laptop = Device::founder("laptop").await;
    let other = Device::founder("other").await;
    assert_eq!(laptop.say(json!({"op": "init", "name": "again"})).await["error"], "in_a_mesh");
    let (_, ticket) = other.add().await;
    assert_eq!(laptop.say(json!({"op": "join", "ticket": ticket, "name": "laptop"})).await["error"], "in_a_mesh");
    let lonely = Device::start().await;
    assert_eq!(lonely.say(json!({"op": "add"})).await["error"], "not_in_a_mesh");
}

#[tokio::test]
async fn any_device_renames_any_device() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    let id = phone.id().await;
    assert_eq!(laptop.say(json!({"op": "rename", "device": id, "name": "old phone"})).await, json!({"ok": true}));
    let state = phone.until("the phone has its new name", |s| s["device"]["name"] == "old phone").await;
    assert_eq!(device_names(&state), ["laptop", "old phone"]);
    let bad = laptop.say(json!({"op": "rename", "device": id, "name": "a/b"})).await;
    assert_eq!(bad["error"], "bad_request", "{bad}");
}

/// Removing a device offers the devices it added as also removed, each ticked; the person
/// unticks any to keep, and one removal carries the rest.
#[tokio::test]
async fn removing_a_device_offers_the_devices_it_added() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    let tablet = Device::start().await;
    tablet.join_through(&phone, "tablet").await;
    let watch = Device::start().await;
    watch.join_through(&phone, "watch").await;
    let desktop = Device::start().await;
    desktop.join_through(&laptop, "desktop").await;
    let five = |s: &Value| s["mesh"]["devices"].as_array().is_some_and(|d| d.len() == 5);
    desktop.until("the desktop lists all five", five).await;

    let (phone_id, tablet_id, watch_id) = (phone.id().await, tablet.id().await, watch.id().await);
    let offer = desktop.say(json!({"op": "removal", "device": phone_id})).await;
    assert_eq!(offer["device"]["name"], "phone");
    let mut also: Vec<(String, String)> = offer["also"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["name"].as_str().unwrap().to_string(), d["id"].as_str().unwrap().to_string()))
        .collect();
    also.sort();
    assert_eq!(also, [("tablet".to_string(), tablet_id.clone()), ("watch".to_string(), watch_id.clone())]);

    // The person unticks the watch.
    let removed = desktop.say(json!({"op": "remove", "devices": [phone_id, tablet_id]})).await;
    assert_eq!(removed, json!({"ok": true}));

    for device in [&laptop, &desktop, &watch] {
        let state =
            device.until("the phone and tablet are gone", |s| device_names(s) == ["desktop", "laptop", "watch"]).await;
        assert_eq!(device_named(&state, "watch")["added_by_name"], "phone", "who added it is history");
    }
    for gone in [&phone, &tablet] {
        gone.until("a removed device hears it was removed", |s| s["mesh"]["member"] == false).await;
    }
    let state =
        laptop.until("the laptop hears of it", |s| events(s).contains(&"desktop removed phone, tablet".into())).await;
    assert!(events(&state).contains(&"phone added watch".into()), "{state}");

    // A mesh keeps at least one device.
    let everyone = json!({"op": "remove", "devices": [laptop.id().await, desktop.id().await, watch_id]});
    assert_eq!(desktop.say(everyone).await["error"], "bad_request");
}
