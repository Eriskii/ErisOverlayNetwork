//! What the library's tests share: devices and apps on real Iroh endpoints.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use erisoverlaynetwork::ceremony::{self, Approval, Ask, PairService};
use erisoverlaynetwork::devices::Op;
use erisoverlaynetwork::join::{self, JoinService, Offers};
use erisoverlaynetwork::mesh::{self, Config, Local, MemoryStore, Node};
use erisoverlaynetwork::net::{self, Network};
use erisoverlaynetwork::pairing::Pairings;
use erisoverlaynetwork::ticket::Ticket;
use erisoverlaynetwork::{Note, Serves, Uses};
use iroh::SecretKey;
use iroh::protocol::Router;

pub struct Device {
    pub name: String,
    pub node: Node,
    pub pairings: Arc<Pairings>,
    /// What is offered to this device while it shows a join ticket.
    pub offers: Arc<Offers>,
    pub _router: Router,
}

pub struct App {
    pub node: Node,
    pub note: Note,
    pub _router: Router,
}

pub async fn device(name: &str) -> Device {
    device_with(name, Arc::new(MemoryStore::default())).await
}

pub async fn device_with(name: &str, store: Arc<dyn mesh::Store>) -> Device {
    device_on(name, store, &Network::Loopback).await
}

/// A device on `network`, as `eon` runs one: its node, the ceremony for letting others in, and
/// the offers made to it while it shows a join ticket.
pub async fn device_on(name: &str, store: Arc<dyn mesh::Store>, network: &Network) -> Device {
    let key = SecretKey::generate();
    let endpoint = net::endpoint(key.clone(), network).await.unwrap();
    let node =
        Node::start(Config { endpoint: endpoint.clone(), network: network.clone(), local: Local::Device(key), store })
            .await
            .unwrap();
    let pairings = Arc::new(Pairings::new());
    let offers = Arc::new(Offers::new(node.id()));
    let shown = name.to_string();
    let adopting = node.clone();
    let adopt: join::Adopt = Arc::new(move |snapshot| {
        let node = adopting.clone();
        Box::pin(async move { node.adopt(snapshot).await })
    });
    let router = Router::builder(endpoint)
        .accept(mesh::ALPN, node.protocol())
        .accept(ceremony::ALPN, PairService::new(pairings.clone(), move || shown.clone()))
        .accept(join::ALPN, JoinService::new(offers.clone(), adopt))
        .spawn();
    Device { name: name.into(), node, pairings, offers, _router: router }
}

/// A ticket `host` shows to let someone in, and its session.
pub async fn ticket(host: &Device) -> (uuid::Uuid, Ticket) {
    let (id, code) = host.pairings.open(600);
    let addr = net::advertised(host.node.endpoint(), &host_network(host)).await;
    (id, Ticket::new(code, host.node.id(), Some(host.name.clone())).unwrap().at(&addr))
}

/// The join ticket `newcomer` shows to be added.
pub async fn join_ticket(newcomer: &Device) -> Ticket {
    let code = newcomer.offers.open(600);
    let addr = net::advertised(newcomer.node.endpoint(), &host_network(newcomer)).await;
    Ticket::join(code, newcomer.node.id(), newcomer.name.clone()).unwrap().at(&addr)
}

fn host_network(device: &Device) -> Network {
    match device.node.endpoint().addr().relay_urls().next() {
        Some(url) => Network::Relay(url.clone()),
        None => Network::Loopback,
    }
}

/// `newcomer` joins `host`'s mesh the way a person does it: ticket, fingerprint, approval.
pub async fn join(newcomer: &Device, host: &Device) {
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

/// What an app uses: one capability, with `grants` there.
pub fn uses(capability: &str, grants: &[&str]) -> Uses {
    Uses::from([(capability.parse().unwrap(), grants.iter().map(|g| g.to_string()).collect())])
}

/// Serving `capability`, of which a mesh has many servers.
pub fn serving(capability: &str) -> Vec<Serves> {
    vec![Serves::new(capability.parse().unwrap())]
}

/// An app approved by `on` to use `capability` with `grants`, and to serve it too with `serves`.
pub async fn app(on: &Device, name: &str, capability: &str, grants: &[&str], serves: bool) -> App {
    let key = SecretKey::generate();
    let note = on
        .node
        .issue(
            key.public(),
            name,
            uses(capability, grants),
            if serves { serving(capability) } else { Vec::new() },
            None,
        )
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

pub async fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !ok() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
