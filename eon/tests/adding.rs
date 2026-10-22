//! Adding a device by a short code typed on it, and by its own QR code scanned from a device
//! already in.

mod support;

use serde_json::json;
use support::{Device, device_names, events, relay};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn a_device_joins_with_the_code_another_shows() {
    let (_relay, network) = relay().await;
    let laptop = Device::start_on(&network).await;
    laptop.init("laptop").await;
    let added = laptop.say(json!({"op": "add"})).await;
    let code = added["code"].as_str().unwrap_or_else(|| panic!("no code: {added}")).to_string();
    assert_eq!(code.len(), 9, "{code}");

    // Typed as a person might: in lower case, with a space for the dash.
    let phone = Device::start_on(&network).await;
    let typed = code.to_lowercase().replace('-', " ");
    let joined = phone.say(json!({"op": "join", "ticket": typed, "name": "phone"})).await;
    let fingerprint = joined["fingerprint"].as_str().unwrap_or_else(|| panic!("{joined}")).to_string();
    let prompt = laptop.prompt().await;
    assert_eq!(prompt["id"], added["id"]);
    assert_eq!(prompt["fingerprint"], fingerprint, "both screens show the same fingerprint");
    assert_eq!(laptop.say(json!({"op": "decide", "id": prompt["id"]})).await, json!({"ok": true}));
    let state = phone.until("the phone is in", |s| s["mesh"]["member"] == true).await;
    assert_eq!(device_names(&state), ["laptop", "phone"]);
}

#[tokio::test]
async fn a_code_nobody_shows_finds_nothing() {
    let (_relay, network) = relay().await;
    let phone = Device::start_on(&network).await;
    let refused = phone.say(json!({"op": "join", "ticket": "K7QX-M2PA", "name": "phone"})).await;
    assert_eq!(refused["error"], "unreachable", "{refused}");
    assert!(phone.state().await["mesh"].is_null());
}

#[tokio::test]
async fn a_device_already_in_adds_one_that_shows_its_own_qr_code() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    let mac = Device::start().await;

    let shown = mac.say(json!({"op": "show_join", "name": "mac"})).await;
    assert_eq!(shown["status"], "showing", "{shown}");
    assert!(shown["svg"].as_str().unwrap().contains("<svg"));
    let ticket = shown["ticket"].as_str().unwrap().to_string();
    assert!(ticket.starts_with("eris://join/"), "{ticket}");
    assert_eq!(mac.state().await["offer"]["ticket"], ticket.as_str());

    // The phone scans it; its person said yes to "add mac?".
    let invited = phone.say(json!({"op": "invite", "ticket": ticket})).await;
    assert_eq!(invited, json!({"ok": true, "name": "mac"}));
    assert_eq!(phone.state().await["inviting"]["status"], "waiting");

    // The mac's screen names the phone and every device it would join, and its person accepts.
    let state = mac.until("the mac hears the offer", |s| s["offer"]["status"] == "offered").await;
    assert_eq!(state["offer"]["by"], "phone");
    assert_eq!(state["offer"]["devices"], json!(["laptop", "phone"]));
    assert_eq!(mac.say(json!({"op": "accept_offer"})).await, json!({"ok": true}));

    let state = mac.until("the mac is in", |s| s["mesh"]["member"] == true).await;
    assert!(state["offer"].is_null(), "a device in a mesh shows no ticket to be added");
    mac.until("the mac's person hears who added it", |s| events(s).contains(&"phone added this device".into())).await;
    phone.until("the phone added the mac", |s| s["inviting"]["status"] == "added").await;
    let state = laptop.until("the laptop lists the mac", |s| device_names(s).contains(&"mac".into())).await;
    assert_eq!(device_names(&state), ["laptop", "mac", "phone"]);
    laptop.until("the laptop hears of it", |s| events(s).contains(&"phone added mac".into())).await;
}

#[tokio::test]
async fn a_device_turns_down_an_offer_and_stays_out() {
    let laptop = Device::founder("laptop").await;
    let mac = Device::start().await;
    let shown = mac.say(json!({"op": "show_join", "name": "mac"})).await;
    let invited = laptop.say(json!({"op": "invite", "ticket": shown["ticket"]})).await;
    assert_eq!(invited["ok"], true, "{invited}");
    mac.until("the mac hears the offer", |s| s["offer"]["status"] == "offered").await;
    assert_eq!(mac.say(json!({"op": "decline_offer"})).await, json!({"ok": true}));

    laptop.until("the laptop hears it was turned down", |s| s["inviting"]["status"] == "declined").await;
    let state = mac.state().await;
    assert!(state["mesh"].is_null());
    assert_eq!(state["offer"]["status"], "showing", "the ticket stays shown");
    assert_eq!(device_names(&laptop.state().await), ["laptop"]);
}

#[tokio::test]
async fn each_kind_of_ticket_goes_only_its_own_way() {
    let laptop = Device::founder("laptop").await;
    let mac = Device::start().await;
    let shown = mac.say(json!({"op": "show_join", "name": "mac"})).await;
    let other = Device::start().await;
    let joined = other.say(json!({"op": "join", "ticket": shown["ticket"], "name": "other"})).await;
    assert_eq!(joined["error"], "bad_request", "{joined}");
    let (_, letting_in) = laptop.add().await;
    let invited = laptop.say(json!({"op": "invite", "ticket": letting_in})).await;
    assert_eq!(invited["error"], "bad_request", "{invited}");
    assert_eq!(laptop.say(json!({"op": "show_join", "name": "laptop"})).await["error"], "in_a_mesh");
    assert_eq!(mac.say(json!({"op": "invite", "ticket": shown["ticket"]})).await["error"], "not_in_a_mesh");
}

/// At the terminal: `eon join` with no ticket shows one, `eon add` with it on a device already in
/// adds the device, and the person at the new device accepts.
#[tokio::test]
async fn a_device_is_added_by_its_own_ticket_at_the_terminal() {
    let laptop = Device::founder("laptop").await;
    let mac = Device::start().await;
    let mut join = mac.command(&["join", "--name", "mac"]).spawn().unwrap();
    let mut joining = BufReader::new(join.stdout.take().unwrap());
    let mut line = String::new();
    let ticket = loop {
        line.clear();
        assert_ne!(joining.read_line(&mut line).await.unwrap(), 0, "join ended before it showed a ticket");
        if let Some(at) = line.find("eris://join/") {
            break line[at..].split_whitespace().next().unwrap().trim_matches('\'').to_string();
        }
    };

    let add = laptop.command(&["add", &ticket, "--yes"]).spawn().unwrap();
    let mut asked = String::new();
    while !asked.contains("[y/N]") {
        assert_ne!(joining.read_line(&mut asked).await.unwrap(), 0, "join ended before it asked: {asked}");
    }
    assert!(asked.contains("laptop offers to add this device to your devices: laptop."), "{asked}");
    join.stdin.take().unwrap().write_all(b"y\n").await.unwrap();

    let added = add.wait_with_output().await.unwrap();
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
    assert!(String::from_utf8_lossy(&added.stdout).contains("Added mac."));
    let mut rest = String::new();
    joining.read_to_string(&mut rest).await.unwrap();
    assert!(join.wait().await.unwrap().success(), "{rest}");
    assert!(rest.contains("Joined, as mac."), "{rest}");
    assert_eq!(device_names(&mac.state().await), ["laptop", "mac"]);
}
