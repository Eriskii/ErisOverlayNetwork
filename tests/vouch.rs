//! A device already in ErisAuth vouches for a new one: it shows a ticket,
//! the new device joins with it, the person compares fingerprints and
//! approves on the device they already trust. No server command is needed.

mod support;

use std::time::Duration;

use serde_json::{Value, json};
use support::{Core, Device, PASSPHRASE};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Start `erisauth join` on `device` with a ticket another device showed; returns
/// the child and the fingerprint it prints.
async fn join_with(device: &Device, name: &str, ticket: &str, ceiling: &[&str]) -> (tokio::process::Child, String) {
    let mut args = vec!["join", "--name", name];
    for grant in ceiling {
        args.extend(["--ceiling", grant]);
    }
    args.push(ticket);
    let mut child = device.command(&args).spawn().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("Compare") {
        line.clear();
        if out.read_line(&mut line).await.unwrap() == 0 {
            let mut why = String::new();
            child.stderr.take().unwrap().read_to_string(&mut why).await.unwrap();
            panic!("join ended early: {why}");
        }
    }
    let fingerprint = line.trim().rsplit(' ').next().unwrap().to_string();
    // Keep draining so the child never blocks on a full pipe.
    tokio::spawn(async move {
        let mut rest = String::new();
        let _ = out.read_to_string(&mut rest).await;
    });
    (child, fingerprint)
}

async fn grants_of(core: &Core, name: &str) -> Value {
    let (_, clients) = core.root.get("/v1/clients").await;
    clients["clients"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()["grants"].clone()
}

#[tokio::test]
async fn a_trusted_device_adds_a_new_one_after_the_person_compares_fingerprints() {
    let core = Core::start().await;
    let mut laptop = Device::new();
    laptop.join(&core, "Laptop", &["*"]).await;
    laptop.serve().await;

    let added = laptop.say(json!({"op": "add"})).await;
    let ticket = added["ticket"].as_str().unwrap();
    assert!(added["svg"].as_str().unwrap().starts_with("<?xml") || added["svg"].as_str().unwrap().starts_with("<svg"));
    let phone = Device::new();
    let (joining, fingerprint) = join_with(&phone, "Phone", ticket, &["notes:read"]).await;

    let approval = laptop.asked().await;
    assert_eq!(approval["kind"], "device");
    assert_eq!(approval["fingerprint"], fingerprint, "both screens show the same code");
    assert_eq!(approval["client"], "ErisAuth on Phone");
    assert_eq!(approval["needs_passphrase"], false);
    assert_eq!(
        approval["essential"],
        json!([
            "meta:pairing:create",
            "meta:pairing:read",
            "meta:pairing:approve",
            "erisauth:approve",
            "erisauth:passphrase"
        ]),
        "what makes it an ErisAuth device is shown apart from its ceiling"
    );
    let granted = approval["requested"].clone();
    assert_eq!(
        laptop.say(json!({"op": "decide", "id": approval["id"], "granted": granted})).await,
        json!({"ok": true})
    );

    let output = tokio::time::timeout(Duration::from_secs(60), joining.wait_with_output()).await.unwrap().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let grants = grants_of(&core, "ErisAuth on Phone").await;
    assert!(grants.as_array().unwrap().contains(&json!("notes:read")), "{grants}");
    assert!(grants.as_array().unwrap().contains(&json!("meta:pairing:approve")), "{grants}");
}

#[tokio::test]
async fn a_new_device_asking_for_more_than_the_voucher_holds_takes_the_passphrase() {
    let core = Core::start().await;
    core.setup_plugin().await;
    let mut phone = Device::new();
    phone.join(&core, "Phone", &["notes:read"]).await;
    phone.serve().await;

    let added = phone.say(json!({"op": "add"})).await;
    let laptop = Device::new();
    let (joining, _) = join_with(&laptop, "Laptop", added["ticket"].as_str().unwrap(), &["*"]).await;
    let approval = phone.asked().await;
    assert_eq!(approval["needs_passphrase"], true);
    let decide = |passphrase: Option<&str>| json!({"op": "decide", "id": approval["id"], "granted": approval["requested"], "passphrase": passphrase});
    assert_eq!(phone.say(decide(None)).await["error"], "needs_passphrase");
    assert_eq!(phone.say(decide(Some(PASSPHRASE))).await, json!({"ok": true}));
    let output = tokio::time::timeout(Duration::from_secs(60), joining.wait_with_output()).await.unwrap().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(grants_of(&core, "ErisAuth on Phone").await.as_array().unwrap().contains(&json!("notes:read")));
    assert!(grants_of(&core, "ErisAuth on Laptop").await.as_array().unwrap().contains(&json!("*")));
}

#[tokio::test]
async fn erisauth_add_shows_the_ticket_and_asks_at_the_terminal() {
    let core = Core::start().await;
    let mut laptop = Device::new();
    laptop.join(&core, "Laptop", &["*"]).await;
    laptop.serve().await;

    let mut adding = laptop.command(&["add"]).spawn().unwrap();
    let mut out = BufReader::new(adding.stdout.take().unwrap());
    let mut shown = String::new();
    let ticket = loop {
        let mut line = String::new();
        assert_ne!(out.read_line(&mut line).await.unwrap(), 0, "add ended early: {shown}");
        shown.push_str(&line);
        if let Some(start) = line.find("erisdb://pair/") {
            break line[start..].trim().to_string();
        }
    };
    let phone = Device::new();
    let (joining, fingerprint) = join_with(&phone, "Phone", &ticket, &["notes:read"]).await;
    while !shown.contains("[a] approve") {
        let mut line = String::new();
        assert_ne!(out.read_line(&mut line).await.unwrap(), 0, "add ended early: {shown}");
        shown.push_str(&line);
    }
    assert!(shown.contains(&fingerprint), "the terminal shows the code to compare: {shown}");
    let mut stdin = adding.stdin.take().unwrap();
    stdin.write_all(b"a\n").await.unwrap();
    drop(stdin);
    out.read_to_string(&mut shown).await.unwrap();
    let output = tokio::time::timeout(Duration::from_secs(60), adding.wait_with_output()).await.unwrap().unwrap();
    assert!(output.status.success(), "{shown}{}", String::from_utf8_lossy(&output.stderr));
    let joined = tokio::time::timeout(Duration::from_secs(60), joining.wait_with_output()).await.unwrap().unwrap();
    assert!(joined.status.success(), "{}", String::from_utf8_lossy(&joined.stderr));
}
