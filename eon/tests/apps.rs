//! Apps asking a device for a note: over the agent socket, as an app beside ErisOverlayNetwork does, and
//! by ticket, as an app with no ErisOverlayNetwork beside it does.

mod support;

use std::time::Duration;

use erisoverlaynetwork::agent::{self, Answer, Ask};
use erisoverlaynetwork::ceremony::{self, Approval};
use erisoverlaynetwork::net::{self, Network};
use erisoverlaynetwork::ticket::Ticket;
use erisoverlaynetwork::{Serves, Uses};
use iroh::SecretKey;
use serde_json::{Value, json};
use support::{Device, PATIENCE};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn uses(capability: &str, grants: &[&str]) -> Uses {
    Uses::from([(capability.parse().unwrap(), grants.iter().map(|g| g.to_string()).collect())])
}

fn files(app: &SecretKey) -> Ask {
    Ask {
        name: "ErisFiles".into(),
        uses: uses("files/1", &["files:read", "files:write"]),
        serves: vec![Serves::new("files/1".parse().unwrap())],
        app: app.public(),
    }
}

/// Ask `device` over its socket in the background, as an app does.
fn ask(device: &Device, ask: Ask) -> tokio::task::JoinHandle<erisoverlaynetwork::Result<Answer>> {
    let socket = device.socket();
    tokio::spawn(async move { agent::connect_to(&socket).await?.expect("the daemon listens").approve(&ask).await })
}

fn note_listed<'a>(state: &'a Value, id: &str) -> &'a Value {
    state["notes"]
        .as_array()
        .and_then(|notes| notes.iter().find(|n| n["id"] == id))
        .unwrap_or_else(|| panic!("note {id} is not listed: {state:#}"))
}

#[tokio::test]
async fn an_app_is_approved_with_one_tap() {
    let laptop = Device::founder("laptop").await;
    let key = SecretKey::generate();
    let asking = ask(&laptop, files(&key));

    let prompt = laptop.prompt().await;
    assert_eq!(prompt["kind"], "app");
    assert_eq!(prompt["via"], "agent");
    assert_eq!(prompt["name"], "ErisFiles");
    assert_eq!(prompt["uses"], json!({"files/1": ["files:read", "files:write"]}));
    assert_eq!(prompt["serves"], json!([{"capability": "files/1"}]));
    assert_eq!(prompt["key"], key.public().to_string());
    assert_eq!(prompt["passphrase"], false);
    assert_eq!(prompt["replaces"], json!([]));
    let me = std::env::current_exe().unwrap();
    assert_eq!(prompt["program"], json!(me), "the kernel's word for who asks, not the app's");
    assert_eq!(prompt["pid"], std::process::id());

    // The person approves part of what was asked.
    let decided = laptop.say(json!({"op": "decide", "id": prompt["id"], "grants": {"files/1": ["files:read"]}})).await;
    assert_eq!(decided, json!({"ok": true}));
    let Answer::Approved { note, snapshot } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    assert_eq!(note.body.uses, uses("files/1", &["files:read"]));
    assert_eq!(note.body.program.as_deref(), Some(me.to_str().unwrap()));
    assert_eq!(note.body.device.to_string(), laptop.id().await);
    assert_eq!(note.app(), key.public());
    assert!(note.serving("files/1").is_some());
    note.check(&snapshot.list().unwrap()).expect("the note counts in the mesh it came with");
    assert!(note.grants("files/1", "files:read") && !note.grants("files/1", "files:write"));

    let state = laptop.state().await;
    assert!(state["approvals"].as_array().unwrap().is_empty());
    let listed = note_listed(&state, &note.id.to_string());
    assert_eq!(listed["name"], "ErisFiles");
    assert_eq!(listed["revoked"], false);

    // The person asked for more than was asked once: that is refused, and the request stays.
    let asking = ask(&laptop, files(&key));
    let prompt = laptop.prompt().await;
    let wider = laptop.say(json!({"op": "decide", "id": prompt["id"], "grants": {"files/1": ["files:*"]}})).await;
    assert_eq!(wider["error"], "not_asked", "{wider}");
    let elsewhere = laptop.say(json!({"op": "decide", "id": prompt["id"], "grants": {"notes/1": []}})).await;
    assert_eq!(elsewhere["error"], "not_asked", "{elsewhere}");
    assert_eq!(laptop.approvals().await.len(), 1);
    laptop.say(json!({"op": "decide", "id": prompt["id"]})).await;
    let Answer::Approved { note, .. } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    assert_eq!(note.body.uses, uses("files/1", &["files:read", "files:write"]), "approved as asked");
}

#[tokio::test]
async fn a_denied_app_hears_so() {
    let laptop = Device::founder("laptop").await;
    let asking = ask(&laptop, files(&SecretKey::generate()));
    let prompt = laptop.prompt().await;
    assert_eq!(laptop.say(json!({"op": "deny", "id": prompt["id"]})).await, json!({"ok": true}));
    assert!(matches!(asking.await.unwrap().unwrap(), Answer::Denied(_)));
    assert!(laptop.state().await["notes"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_app_that_hangs_up_withdraws_its_request() {
    let laptop = Device::founder("laptop").await;
    let stream = tokio::net::UnixStream::connect(laptop.socket()).await.unwrap();
    let (read, mut write) = stream.into_split();
    let line = json!({"op": "approve", "v": 3, "name": "ErisFiles", "uses": {"files/1": ["files:read"]},
        "app": SecretKey::generate().public().to_string()});
    write.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    let prompt = laptop.prompt().await;
    drop((read, write));

    laptop.until("the request is withdrawn", |s| s["approvals"].as_array().unwrap().is_empty()).await;
    let late = laptop.say(json!({"op": "decide", "id": prompt["id"]})).await;
    assert_eq!(late["error"], "no_such_request", "{late}");
}

#[tokio::test]
async fn what_an_app_cannot_be_approved_for_it_hears_at_once() {
    let lonely = Device::start().await;
    let refused = ask(&lonely, files(&SecretKey::generate())).await.unwrap().unwrap_err().to_string();
    assert!(refused.contains("no mesh"), "{refused}");

    let laptop = Device::founder("laptop").await;
    let device_key: iroh::EndpointId = laptop.id().await.parse().unwrap();
    let mut posing = files(&SecretKey::generate());
    posing.app = device_key;
    let refused = ask(&laptop, posing).await.unwrap().unwrap_err().to_string();
    assert!(refused.contains("key of its own"), "{refused}");

    let mut nothing = files(&SecretKey::generate());
    (nothing.uses, nothing.serves) = (Uses::new(), vec![]);
    assert!(ask(&laptop, nothing).await.unwrap().is_err());
    assert!(laptop.approvals().await.is_empty(), "nothing was put to the person");

    // The wrong version is refused before anything else.
    let stream = tokio::net::UnixStream::connect(laptop.socket()).await.unwrap();
    let (read, mut write) = stream.into_split();
    write.write_all(b"{\"op\":\"snapshot\",\"v\":1}\n").await.unwrap();
    let mut answer = String::new();
    BufReader::new(read).read_line(&mut answer).await.unwrap();
    assert!(answer.contains("\"error\""), "{answer}");
}

#[tokio::test]
async fn an_app_reads_the_mesh_from_the_socket() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    let snapshot = agent::connect_to(&phone.socket()).await.unwrap().unwrap().snapshot().await.unwrap();
    let list = snapshot.list().unwrap();
    assert_eq!(snapshot.mesh.to_string(), phone.state().await["mesh"]["id"]);
    assert!(list.is_member(&laptop.id().await.parse().unwrap()));
    let me = phone.id().await;
    assert!(snapshot.directory.iter().any(|p| p.id.to_string() == me), "the device's own presence");
}

/// An app with no ErisOverlayNetwork beside it, such as one in a browser, asks by ticket: the device
/// shows the ticket, the app redeems it under its own key, and both screens show a fingerprint.
#[tokio::test]
async fn an_app_without_eon_beside_it_is_approved_by_ticket() {
    let laptop = Device::founder("laptop").await;
    let added = laptop.say(json!({"op": "add_app"})).await;
    let ticket = Ticket::parse(added["ticket"].as_str().unwrap()).unwrap();
    assert!(added["svg"].as_str().unwrap().contains("<svg"), "{added}");

    let key = SecretKey::generate();
    let endpoint = net::endpoint(key.clone(), &Network::Loopback).await.unwrap();
    let wants = ceremony::Ask::App {
        name: "Notes".into(),
        uses: uses("notes/1", &["notes:read", "notes:write"]),
        serves: vec![],
    };
    let joining = ceremony::redeem(&endpoint, &ticket, wants).await.unwrap();
    assert_eq!(joining.host, "laptop");

    let prompt = laptop.prompt().await;
    assert_eq!(prompt["id"], added["id"]);
    assert_eq!(prompt["kind"], "app");
    assert_eq!(prompt["via"], "ticket");
    assert_eq!(prompt["fingerprint"], joining.fingerprint, "both screens show the same code");
    assert_eq!(prompt["key"], key.public().to_string());
    assert!(prompt["program"].is_null(), "nothing on this device vouches for what it is");
    let decided = laptop.say(json!({"op": "decide", "id": prompt["id"], "grants": {"notes/1": ["notes:read"]}})).await;
    assert_eq!(decided, json!({"ok": true}));

    let Some(Approval::App { note, snapshot }) = joining.wait(PATIENCE).await.unwrap() else { panic!("not approved") };
    assert_eq!(note.app(), key.public());
    assert_eq!(note.body.uses, uses("notes/1", &["notes:read"]));
    note.check(&snapshot.list().unwrap()).unwrap();
    note_listed(&laptop.state().await, &note.id.to_string());
}

#[tokio::test]
async fn an_app_that_takes_back_its_ticket_request_is_no_longer_asked_about() {
    let laptop = Device::founder("laptop").await;
    let added = laptop.say(json!({"op": "add_app"})).await;
    let endpoint = net::endpoint(SecretKey::generate(), &Network::Loopback).await.unwrap();
    let ticket = Ticket::parse(added["ticket"].as_str().unwrap()).unwrap();
    let wants = ceremony::Ask::App { name: "Notes".into(), uses: uses("notes/1", &[]), serves: vec![] };
    let joining = ceremony::redeem(&endpoint, &ticket, wants).await.unwrap();
    laptop.prompt().await;
    joining.withdraw().await.unwrap();
    laptop.until("the request is gone", |s| s["approvals"].as_array().unwrap().is_empty()).await;
    let late = laptop.say(json!({"op": "decide", "id": added["id"]})).await;
    assert_eq!(late["error"], "no_such_request", "{late}");
}

#[tokio::test]
async fn a_ticket_for_a_device_takes_no_app_and_a_ticket_for_an_app_takes_no_device() {
    let laptop = Device::founder("laptop").await;
    let (_, ticket) = laptop.add().await;
    let endpoint = net::endpoint(SecretKey::generate(), &Network::Loopback).await.unwrap();
    let app = ceremony::Ask::App { name: "Notes".into(), uses: uses("notes/1", &[]), serves: vec![] };
    let joining = ceremony::redeem(&endpoint, &Ticket::parse(&ticket).unwrap(), app).await.unwrap();
    assert!(joining.wait(PATIENCE).await.unwrap().is_none(), "denied before the person is asked");

    let added = laptop.say(json!({"op": "add_app"})).await;
    let endpoint = net::endpoint(SecretKey::generate(), &Network::Loopback).await.unwrap();
    let ticket = Ticket::parse(added["ticket"].as_str().unwrap()).unwrap();
    let joining = ceremony::redeem(&endpoint, &ticket, ceremony::Ask::Device { name: "phone".into() }).await.unwrap();
    assert!(joining.wait(PATIENCE).await.unwrap().is_none());
    assert!(laptop.approvals().await.is_empty());
}

#[tokio::test]
async fn a_ticket_the_person_closes_is_redeemed_by_no_one() {
    let laptop = Device::founder("laptop").await;
    let (session, ticket) = laptop.add().await;
    assert_eq!(laptop.say(json!({"op": "close", "id": session})).await, json!({"ok": true}));
    let endpoint = net::endpoint(SecretKey::generate(), &Network::Loopback).await.unwrap();
    let ticket = Ticket::parse(&ticket).unwrap();
    assert!(ceremony::redeem(&endpoint, &ticket, ceremony::Ask::Device { name: "phone".into() }).await.is_err());
}

#[tokio::test]
async fn a_revoked_note_stops_counting_everywhere() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    let asking = ask(&phone, files(&SecretKey::generate()));
    let prompt = phone.prompt().await;
    phone.say(json!({"op": "decide", "id": prompt["id"]})).await;
    let Answer::Approved { note, .. } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    let id = note.id.to_string();

    // Revoked from the device that issued it, it stops counting on every device.
    assert_eq!(phone.say(json!({"op": "revoke", "note": id})).await, json!({"ok": true}));
    phone.until("the note shows revoked", |s| note_listed(s, &id)["revoked"] == true).await;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let snapshot = agent::connect_to(&laptop.socket()).await.unwrap().unwrap().snapshot().await.unwrap();
            if note.check(&snapshot.list().unwrap()).is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the laptop never heard of the revocation");

    let listed = phone.command(&["apps"]).output().await.unwrap();
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(listed.contains("ErisFiles") && listed.contains("revoked"), "{listed}");
}

/// An app taking part in the mesh under `note`, as a real server does: its own node on its own
/// endpoint, telling the devices where it is.
async fn serving(key: &SecretKey, note: &erisoverlaynetwork::Note, snapshot: erisoverlaynetwork::Snapshot) -> Serving {
    use erisoverlaynetwork::mesh::{self, Config, Local, MemoryStore, Node};
    let endpoint = net::endpoint(key.clone(), &Network::Loopback).await.unwrap();
    let node = Node::start(Config {
        endpoint: endpoint.clone(),
        network: Network::Loopback,
        local: Local::App(note.clone()),
        store: std::sync::Arc::new(MemoryStore::default()),
    })
    .await
    .unwrap();
    let router = iroh::protocol::Router::builder(endpoint).accept(mesh::ALPN, node.protocol()).spawn();
    node.adopt(snapshot).await.unwrap();
    Serving { _node: node, _router: router }
}

struct Serving {
    _node: erisoverlaynetwork::mesh::Node,
    _router: iroh::protocol::Router,
}

/// A database asking to serve `database/1`, as ErisDB does: a mesh has a single one, and apps
/// using it take its passphrase.
fn database(name: &str, key: &SecretKey) -> Ask {
    let serves = vec![Serves { capability: "database/1".parse().unwrap(), single: true, passphrase: true }];
    Ask { name: name.into(), uses: Uses::new(), serves, app: key.public() }
}

/// The server in use of `capability`, in a state.
fn server<'a>(state: &'a Value, capability: &str) -> &'a Value {
    state["mesh"]["servers"]
        .as_array()
        .and_then(|servers| servers.iter().find(|s| s["capability"] == capability))
        .map(|s| &s["apps"][0])
        .unwrap_or(&Value::Null)
}

/// A mesh has a single database. An app that asks to serve `database/1` asks to be it: the
/// person approves it with one tap and every device finds it; approving another revokes it, so
/// only one ever counts.
#[tokio::test]
async fn approving_a_database_revokes_the_one_before_it() {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    assert_eq!(laptop.state().await["mesh"]["servers"], json!([]));

    let first = SecretKey::generate();
    let asking = ask(&laptop, database("ErisDB", &first));
    let prompt = laptop.prompt().await;
    assert_eq!(prompt["serves"], json!([{"capability": "database/1", "single": true, "passphrase": true}]));
    assert_eq!(prompt["replaces"], json!([]), "there is no database yet");
    assert_eq!(prompt["passphrase"], false, "serving it takes none");
    assert_eq!(laptop.say(json!({"op": "decide", "id": prompt["id"]})).await, json!({"ok": true}));
    let Answer::Approved { note, snapshot } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    assert!(note.serving("database/1").is_some_and(|s| s.single && s.passphrase));
    let _first = serving(&first, &note, snapshot).await;

    let state =
        phone.until("the phone finds the mesh's database", |s| server(s, "database/1")["name"] == "ErisDB").await;
    assert_eq!(server(&state, "database/1")["app"], first.public().to_string());
    assert_eq!(server(&state, "database/1")["device"], "laptop", "the device that approved it");
    let servers = &state["mesh"]["servers"][0];
    assert_eq!((servers["single"].clone(), servers["passphrase"].clone()), (json!(true), json!(true)));
    phone
        .until("the phone hears of it", |s| {
            support::events(s).contains(&"laptop approved ErisDB as your database".into())
        })
        .await;

    // A second database asks the phone; the person there sees which it replaces.
    let second = SecretKey::generate();
    let asking = ask(&phone, database("ErisDB 2", &second));
    let prompt = phone.prompt().await;
    assert_eq!(prompt["replaces"], json!([{"capability": "database/1", "name": "ErisDB"}]));
    phone.say(json!({"op": "decide", "id": prompt["id"]})).await;
    let Answer::Approved { note: replacing, snapshot } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    let list = snapshot.list().unwrap();
    assert!(list.is_revoked(&note.id), "the first database no longer counts");
    replacing.check(&list).unwrap();
    let _second = serving(&second, &replacing, snapshot).await;

    laptop
        .until("the laptop finds the new database", |s| server(s, "database/1")["app"] == second.public().to_string())
        .await;
    let state = laptop
        .until("the laptop hears of it", |s| {
            support::events(s).contains(&"phone approved ErisDB 2 as your database".into())
        })
        .await;
    assert_eq!(note_listed(&state, &note.id.to_string())["revoked"], true);
}

/// An app using something whose server takes a passphrase takes it too, and the server checks it;
/// a server that has not been heard from cannot check it, and the person hears so. An app using
/// something nothing takes a passphrase for is approved with one tap.
#[tokio::test]
async fn an_app_using_a_database_takes_its_passphrase() {
    let laptop = Device::founder("laptop").await;
    let notes = ask(
        &laptop,
        Ask { name: "Notes".into(), uses: uses("notes/1", &[]), serves: vec![], app: SecretKey::generate().public() },
    );
    let prompt = laptop.prompt().await;
    assert_eq!(prompt["passphrase"], false);
    assert_eq!(laptop.say(json!({"op": "decide", "id": prompt["id"]})).await, json!({"ok": true}));
    assert!(matches!(notes.await.unwrap().unwrap(), Answer::Approved { .. }));

    // A database approved here, which is not running.
    let db = ask(&laptop, database("ErisDB", &SecretKey::generate()));
    let prompt = laptop.prompt().await;
    laptop.say(json!({"op": "decide", "id": prompt["id"]})).await;
    db.await.unwrap().unwrap();

    let wants = Ask {
        name: "Tasks".into(),
        uses: uses("database/1", &["tasks:read"]),
        serves: vec![],
        app: SecretKey::generate().public(),
    };
    let _asking = ask(&laptop, wants);
    let prompt = laptop.prompt().await;
    assert_eq!(prompt["passphrase"], true);
    let without = laptop.say(json!({"op": "decide", "id": prompt["id"]})).await;
    assert_eq!(without["error"], "no_server", "{without}");
    assert!(without["message"].as_str().unwrap().contains("ErisDB has not been heard from"), "{without}");
    assert_eq!(laptop.approvals().await.len(), 1, "the request stays open");
    let change = laptop.say(json!({"op": "passphrase", "current": "correct horse", "new": "battery staple"})).await;
    assert_eq!(change["error"], "no_server", "{change}");
}
