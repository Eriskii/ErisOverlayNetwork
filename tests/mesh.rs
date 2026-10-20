//! Real participants on loopback Iroh endpoints: devices that join one another by ticket,
//! apps under notes, and what happens when a device leaves.

use std::sync::Arc;
use std::time::Duration;

use erisoverlaynetwork::ceremony::{self, Approval, Ask, PairService};
use erisoverlaynetwork::devices::Op;
use erisoverlaynetwork::mesh::{self, Config, FileStore, Local, MemoryStore, Node};
use erisoverlaynetwork::net::{self, Network};
use erisoverlaynetwork::pairing::Pairings;
use erisoverlaynetwork::ticket::Ticket;
use erisoverlaynetwork::{Error, Note};
use iroh::SecretKey;
use iroh::protocol::Router;

struct Device {
    name: String,
    node: Node,
    pairings: Arc<Pairings>,
    _router: Router,
}

struct App {
    node: Node,
    note: Note,
    _router: Router,
}

async fn device(name: &str) -> Device {
    device_with(name, Arc::new(MemoryStore::default())).await
}

async fn device_with(name: &str, store: Arc<dyn mesh::Store>) -> Device {
    let key = SecretKey::generate();
    let endpoint = net::endpoint(key.clone(), &Network::Loopback).await.unwrap();
    let node = Node::start(Config {
        endpoint: endpoint.clone(),
        network: Network::Loopback,
        local: Local::Device(key),
        store,
    })
    .await
    .unwrap();
    let pairings = Arc::new(Pairings::new());
    let shown = name.to_string();
    let router = Router::builder(endpoint)
        .accept(mesh::ALPN, node.protocol())
        .accept(ceremony::ALPN, PairService::new(pairings.clone(), move || shown.clone()))
        .spawn();
    Device { name: name.into(), node, pairings, _router: router }
}

async fn ticket(host: &Device) -> (uuid::Uuid, Ticket) {
    let (id, code) = host.pairings.open(600);
    let addr = net::advertised(host.node.endpoint(), &Network::Loopback).await;
    (id, Ticket::new(code, host.node.id(), Some(host.name.clone())).unwrap().at(&addr))
}

/// `newcomer` joins `host`'s mesh the way a person does it: ticket, fingerprint, approval.
async fn join(newcomer: &Device, host: &Device) {
    let (session, ticket) = ticket(host).await;
    let joining =
        ceremony::redeem(newcomer.node.endpoint(), &ticket, Ask::Device { name: newcomer.name.clone() }).await.unwrap();
    let asked = host.pairings.get(session).unwrap();
    assert_eq!(asked.fingerprint.as_deref(), Some(joining.fingerprint.as_str()));
    assert_eq!(asked.identity, Some(newcomer.node.id()));
    assert_eq!(joining.host, host.name);

    host.node.append(Op::Add { device: newcomer.node.id(), name: newcomer.name.clone() }).await.unwrap();
    let snapshot = host.node.snapshot().await.unwrap();
    host.pairings.approve(session, Approval::Device { snapshot }).unwrap();
    match joining.wait(Duration::from_secs(10)).await.unwrap() {
        Some(Approval::Device { snapshot }) => newcomer.node.adopt(snapshot).await.unwrap(),
        other => panic!("expected the list, got {other:?}"),
    }
}

/// An app approved by `on` for `service`.
async fn app(on: &Device, name: &str, service: &str, grants: &[&str], serves: bool) -> App {
    let key = SecretKey::generate();
    let note = on
        .node
        .issue(key.public(), name, service, grants.iter().map(|g| g.to_string()).collect(), serves, None)
        .unwrap();
    let endpoint = net::endpoint(key, &Network::Loopback).await.unwrap();
    let node = Node::start(Config {
        endpoint: endpoint.clone(),
        network: Network::Loopback,
        local: Local::App(note.clone()),
        store: Arc::new(MemoryStore::default()),
    })
    .await
    .unwrap();
    node.adopt(on.node.snapshot().await.unwrap()).await.unwrap();
    node.sync_all().await;
    let router = Router::builder(endpoint).accept(mesh::ALPN, node.protocol()).spawn();
    App { node, note, _router: router }
}

async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !ok() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn devices_joined_through_one_another_all_know_each_other() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let desktop = device("desktop").await;
    join(&desktop, &laptop).await;
    let server = device("server").await;
    join(&server, &desktop).await;

    // The phone only ever met the laptop, and still learns of the desktop and the server, and
    // where they are.
    let everyone = [laptop.node.id(), phone.node.id(), desktop.node.id(), server.node.id()];
    for node in [&laptop.node, &phone.node, &desktop.node, &server.node] {
        eventually("every device lists every device", || everyone.iter().all(|d| node.list().unwrap().is_member(d)))
            .await;
    }
    eventually("the phone finds the server", || phone.node.directory().iter().any(|p| p.id == server.node.id())).await;
    let server_on_phone = phone.node.list().unwrap();
    assert_eq!(server_on_phone.member(&server.node.id()).unwrap().added_by, Some(desktop.node.id()));
    assert_eq!(server_on_phone.state(), server.node.list().unwrap().state());
}

#[tokio::test]
async fn an_app_on_one_device_is_let_in_by_an_app_on_another_for_what_its_note_grants() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;

    let files_laptop = app(&laptop, "ErisFiles", "files", &["files:read"], true).await;
    let files_phone = app(&phone, "ErisFiles", "files", &["files:read"], true).await;
    let notes_phone = app(&phone, "Notes", "notes", &["notes:read"], false).await;

    // The phone's ErisFiles finds the laptop's among the providers of `files`.
    eventually("the phone's app finds the laptop's", || {
        files_phone.node.providers("files").iter().any(|p| p.id == files_laptop.node.id())
    })
    .await;
    files_phone.node.sync_with(files_laptop.node.id()).await.unwrap();
    notes_phone.node.sync_with(files_laptop.node.id()).await.unwrap();

    let caller = files_laptop.node.authorize(files_phone.node.id(), "files", Some("files:read")).unwrap();
    assert_eq!(caller.device, phone.node.id());
    assert_eq!(caller.device_name, "phone");
    assert_eq!(caller.note, files_phone.note);
    assert!(matches!(
        files_laptop.node.authorize(files_phone.node.id(), "files", Some("files:write")),
        Err(Error::Forbidden { .. })
    ));
    // Approved for another service, the notes app is not let into this one.
    assert!(files_laptop.node.authorize(notes_phone.node.id(), "files", None).is_err());

    // And a service dials a provider under its own name, which the provider then knows.
    let conn = files_phone.node.connect(files_laptop.node.id(), mesh::ALPN).await.unwrap();
    assert_eq!(conn.remote_id(), files_laptop.node.id());
}

#[tokio::test]
async fn removing_a_device_cuts_its_apps_off_everywhere_at_once() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let desktop = device("desktop").await;
    join(&desktop, &laptop).await;

    let files_desktop = app(&desktop, "ErisFiles", "files", &["files:read"], true).await;
    let files_phone = app(&phone, "ErisFiles", "files", &["files:read"], true).await;
    files_phone.node.sync_with(files_desktop.node.id()).await.unwrap();
    assert!(files_desktop.node.authorize(files_phone.node.id(), "files", Some("files:read")).is_ok());

    let mut changes = files_desktop.node.subscribe();
    changes.borrow_and_update();
    laptop.node.append(Op::Remove { devices: vec![phone.node.id()] }).await.unwrap();

    // Pushed, not waited for: no one asks for it, and no periodic sync runs in this time.
    tokio::time::timeout(Duration::from_secs(5), changes.changed()).await.unwrap().unwrap();
    eventually("the desktop's app refuses the phone's", || {
        files_desktop.node.authorize(files_phone.node.id(), "files", Some("files:read")).is_err()
    })
    .await;
    assert!(!files_desktop.node.directory().iter().any(|p| p.id == files_phone.node.id()));
    eventually("the desktop drops the phone", || !desktop.node.list().unwrap().is_member(&phone.node.id())).await;
}

#[tokio::test]
async fn a_revoked_note_stops_counting() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let files_laptop = app(&laptop, "ErisFiles", "files", &["files:read"], true).await;
    let files_phone = app(&phone, "ErisFiles", "files", &["files:read"], true).await;
    files_phone.node.sync_with(files_laptop.node.id()).await.unwrap();
    assert!(files_laptop.node.authorize(files_phone.node.id(), "files", None).is_ok());

    // The laptop revokes the phone's app; the phone itself stays.
    laptop.node.append(Op::Revoke { note: files_phone.note.id }).await.unwrap();
    eventually("the laptop's app refuses the revoked one", || {
        files_laptop.node.authorize(files_phone.node.id(), "files", None).is_err()
    })
    .await;
    assert!(laptop.node.list().unwrap().is_member(&phone.node.id()));
}

#[tokio::test]
async fn a_stranger_learns_nothing_and_changes_nothing() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let stranger = device("stranger").await;
    stranger.node.create("stranger").await.unwrap();

    let addr = net::advertised(laptop.node.endpoint(), &Network::Loopback).await;
    let conn = stranger.node.endpoint().connect(addr, mesh::ALPN).await.unwrap();
    let request = mesh::Request::Sync {
        mesh: laptop.node.mesh().unwrap(),
        me: stranger.node.presence().await,
        heads: vec![],
        entries: stranger.node.list().unwrap().entries(),
        directory: vec![],
    };
    let refused: Result<mesh::Synced, _> = erisoverlaynetwork::wire::ask(&conn, &request).await;
    assert!(matches!(refused, Err(Error::Forbidden { .. })));
    assert_eq!(laptop.node.list().unwrap().len(), 1);
    assert!(laptop.node.directory().is_empty());
}

#[tokio::test]
async fn an_app_without_erisauth_beside_it_gets_its_note_by_ticket() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let browser = SecretKey::generate();
    let endpoint = net::endpoint(browser.clone(), &Network::Loopback).await.unwrap();

    let (session, ticket) = ticket(&laptop).await;
    let ask =
        Ask::App { name: "Tasks".into(), service: "erisdb".into(), grants: vec!["tasks:*".into()], serves: false };
    let joining = ceremony::redeem(&endpoint, &ticket, ask).await.unwrap();
    assert_eq!(laptop.pairings.get(session).unwrap().fingerprint.as_deref(), Some(joining.fingerprint.as_str()));

    let note = laptop.node.issue(browser.public(), "Tasks", "erisdb", vec!["tasks:*".into()], false, None).unwrap();
    let snapshot = laptop.node.snapshot().await.unwrap();
    laptop.pairings.approve(session, Approval::App { note: note.clone(), snapshot }).unwrap();
    match joining.wait(Duration::from_secs(10)).await.unwrap() {
        Some(Approval::App { note: got, snapshot }) => {
            assert_eq!(got, note);
            got.check(&snapshot.list().unwrap()).unwrap();
        }
        other => panic!("expected a note, got {other:?}"),
    }
}

#[tokio::test]
async fn a_denied_newcomer_hears_so() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    let (session, ticket) = ticket(&laptop).await;
    let joining = ceremony::redeem(phone.node.endpoint(), &ticket, Ask::Device { name: "phone".into() }).await.unwrap();
    laptop.pairings.deny(session).unwrap();
    assert!(joining.wait(Duration::from_secs(10)).await.unwrap().is_none());
}

#[tokio::test]
async fn a_node_remembers_its_mesh_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FileStore::new(dir.path().join("mesh.json")));
    let key = SecretKey::generate();
    let endpoint = net::endpoint(key.clone(), &Network::Loopback).await.unwrap();
    let node = Node::start(Config {
        endpoint,
        network: Network::Loopback,
        local: Local::Device(key.clone()),
        store: store.clone(),
    })
    .await
    .unwrap();
    let mesh = node.create("laptop").await.unwrap();
    let phone = SecretKey::generate().public();
    node.append(Op::Add { device: phone, name: "phone".into() }).await.unwrap();
    node.shutdown().await;

    let endpoint = net::endpoint(key.clone(), &Network::Loopback).await.unwrap();
    let again =
        Node::start(Config { endpoint, network: Network::Loopback, local: Local::Device(key), store }).await.unwrap();
    assert_eq!(again.mesh(), Some(mesh));
    assert!(again.list().unwrap().is_member(&phone));
}

#[tokio::test]
async fn many_changes_at_once_all_reach_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FileStore::new(dir.path().join("mesh.json")));
    let laptop = device_with("laptop", store.clone()).await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    // Fifty renames racing each other and the syncs they set off.
    let renames = (0..50).map(|i| {
        let node = laptop.node.clone();
        let phone = phone.node.id();
        async move { node.append(Op::Name { device: phone, name: format!("phone {i}") }).await }
    });
    for renamed in n0_future::join_all(renames).await {
        renamed.unwrap();
    }
    let saved = mesh::Store::load(&*store).await.unwrap();
    let list = erisoverlaynetwork::DeviceList::from_entries(saved.entries).unwrap();
    assert_eq!(list.len(), laptop.node.list().unwrap().len());
}

#[tokio::test]
async fn entries_written_back_to_back_all_arrive() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let a = app(&phone, "One", "files", &["files:read"], true).await;
    let b = app(&phone, "Two", "files", &["files:read"], true).await;
    let watcher = app(&laptop, "ErisFiles", "files", &["files:read"], true).await;
    laptop.node.append(Op::Revoke { note: a.note.id }).await.unwrap();
    laptop.node.append(Op::Revoke { note: b.note.id }).await.unwrap();
    eventually("both revocations reach an app", || {
        let list = watcher.node.list().unwrap();
        list.is_revoked(&a.note.id) && list.is_revoked(&b.note.id)
    })
    .await;
}

#[tokio::test]
async fn whoever_is_cut_off_hears_so() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let files_laptop = app(&laptop, "ErisFiles", "files", &["files:read"], true).await;
    files_laptop.node.sync_all().await;
    phone.node.sync_all().await;

    laptop.node.append(Op::Revoke { note: files_laptop.note.id }).await.unwrap();
    eventually("the revoked app learns of its revocation", || {
        files_laptop.node.list().unwrap().is_revoked(&files_laptop.note.id)
    })
    .await;

    laptop.node.append(Op::Remove { devices: vec![phone.node.id()] }).await.unwrap();
    eventually("the removed phone learns it was removed", || {
        phone.node.list().unwrap().state().removed.contains(&phone.node.id())
    })
    .await;
    // And from then on it is told why it is refused.
    let refused = phone.node.sync_with(laptop.node.id()).await;
    assert!(matches!(refused, Err(Error::Forbidden { permission }) if permission.contains("removed")));
}

#[tokio::test]
async fn apps_heard_of_second_hand_meet_without_waiting() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let files_laptop = app(&laptop, "ErisFiles", "files", &["files:read"], true).await;
    let files_phone = app(&phone, "ErisFiles", "files", &["files:read"], true).await;
    // Neither app is told of the other by anyone but the devices; both find each other, and each
    // lets the other in, well before any periodic announce.
    eventually("each app lets the other in", || {
        files_laptop.node.authorize(files_phone.node.id(), "files", Some("files:read")).is_ok()
            && files_phone.node.authorize(files_laptop.node.id(), "files", Some("files:read")).is_ok()
    })
    .await;
}

#[tokio::test]
async fn a_newcomer_that_leaves_takes_its_request_back() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    let (session, ticket) = ticket(&laptop).await;
    let joining = ceremony::redeem(phone.node.endpoint(), &ticket, Ask::Device { name: "phone".into() }).await.unwrap();
    assert!(laptop.pairings.get(session).is_some());
    joining.withdraw().await.unwrap();
    assert!(laptop.pairings.get(session).is_none());
}

#[tokio::test]
async fn an_app_approved_again_right_after_a_revocation_gets_a_note_that_counts() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let app = SecretKey::generate().public();
    let first = laptop.node.issue(app, "Notes", "notes", vec!["notes:read".into()], false, None).unwrap();
    laptop.node.append(Op::Revoke { note: first.id }).await.unwrap();
    let again = laptop.node.issue(app, "Notes", "notes", vec!["notes:read".into()], false, None).unwrap();
    assert_ne!(again.id, first.id);
    again.check(&laptop.node.list().unwrap()).unwrap();
}

#[tokio::test]
async fn a_device_that_joins_later_finds_a_service_it_never_met() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let server = device("server").await;
    join(&server, &laptop).await;
    let db = app(&server, "ErisDB", "erisdb", &[], true).await;
    // The phone joins through the laptop, which may or may not have heard of ErisDB yet.
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    eventually("the phone finds the mesh's ErisDB", || {
        phone.node.provider("erisdb").is_some_and(|p| p.id == db.node.id())
    })
    .await;
    assert!(phone.node.counts(&db.note));
}

#[tokio::test]
async fn the_one_provider_of_a_service_is_the_most_recently_approved_that_counts() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let old = app(&laptop, "ErisDB", "erisdb", &[], true).await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let new = app(&laptop, "ErisDB", "erisdb", &[], true).await;
    let watcher = app(&laptop, "Tasks", "notes", &[], true).await;
    eventually("the newer ErisDB provides", || watcher.node.provider("erisdb").is_some_and(|p| p.id == new.node.id()))
        .await;
    // Revoking the newer one leaves the older; a person approving a new ErisDB revokes the old.
    laptop.node.append(Op::Revoke { note: new.note.id }).await.unwrap();
    eventually("the older ErisDB provides again", || {
        watcher.node.provider("erisdb").is_some_and(|p| p.id == old.node.id())
    })
    .await;
}

#[tokio::test]
async fn a_node_knows_when_its_endpoint_has_closed() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    assert!(!laptop.node.is_closed());
    laptop.node.endpoint().close().await;
    tokio::time::timeout(Duration::from_secs(5), laptop.node.closed()).await.unwrap();
    assert!(laptop.node.is_closed());
}

#[tokio::test]
async fn a_newcomer_whose_page_went_away_collects_its_answer_from_a_new_one() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let browser = SecretKey::generate();
    let endpoint = net::endpoint(browser.clone(), &Network::Loopback).await.unwrap();
    let (session, ticket) = ticket(&laptop).await;
    let ask =
        Ask::App { name: "Tasks".into(), service: "erisdb".into(), grants: vec!["tasks:*".into()], serves: false };
    let first = ceremony::redeem(&endpoint, &ticket, ask.clone()).await.unwrap();
    let host = first.host.clone();
    drop(first);

    // A later page, under the same key, waits for the same answer.
    let again = ceremony::resume(&endpoint, &ticket, ask, host).unwrap();
    assert_eq!(laptop.pairings.get(session).unwrap().fingerprint.as_deref(), Some(again.fingerprint.as_str()));
    let note = laptop.node.issue(browser.public(), "Tasks", "erisdb", vec!["tasks:*".into()], false, None).unwrap();
    laptop
        .pairings
        .approve(session, Approval::App { note: note.clone(), snapshot: laptop.node.snapshot().await.unwrap() })
        .unwrap();
    assert!(
        matches!(again.wait(Duration::from_secs(10)).await.unwrap(), Some(Approval::App { note: got, .. }) if got == note)
    );

    // Another key resuming the same ticket collects nothing.
    let other = net::endpoint(SecretKey::generate(), &Network::Loopback).await.unwrap();
    let ask =
        Ask::App { name: "Tasks".into(), service: "erisdb".into(), grants: vec!["tasks:*".into()], serves: false };
    let thief = ceremony::resume(&other, &ticket, ask, "laptop".into()).unwrap();
    assert!(matches!(thief.collect().await, Err(Error::Unauthorized)));
}
