//! Short codes: typed in place of a ticket, they find the device showing it.

mod support;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use erisoverlaynetwork::ceremony::{self, Approval, Ask};
use erisoverlaynetwork::code::{self, Shown};
use erisoverlaynetwork::devices::Op;
use erisoverlaynetwork::mesh::MemoryStore;
use erisoverlaynetwork::net::Network;
use iroh_relay::server::{RelayConfig, Server, ServerConfig};
use support::{device_on, ticket};

/// A relay on loopback, and the network that reaches nothing but it.
async fn relay() -> (Server, Network) {
    let mut config = ServerConfig::default();
    config.relay = Some(RelayConfig::new((Ipv4Addr::LOCALHOST, 0)));
    let server = Server::spawn(config).await.unwrap();
    let url = format!("http://{}", server.http_addr().unwrap()).parse().unwrap();
    (server, Network::Relay(url))
}

fn relay_url(network: &Network) -> Option<iroh::RelayUrl> {
    match network {
        Network::Relay(url) => Some(url.clone()),
        _ => None,
    }
}

#[test]
fn a_code_is_read_however_it_is_typed() {
    let code = code::generate();
    assert_eq!(code.len(), 9);
    assert_eq!(code.as_bytes()[4], b'-');
    assert_eq!(code::normalize(&code).unwrap(), code);
    assert_eq!(code::normalize("k7qx m2pa").unwrap(), "K7QX-M2PA");
    assert_eq!(code::normalize("K7QXM2PA").unwrap(), "K7QX-M2PA");
    // An O is a zero and an I or L a one, as they are easily misread.
    assert_eq!(code::normalize("OIL0-1234").unwrap(), "0110-1234");
    assert_eq!(code::id("o1l0-1234").unwrap(), code::id("0110 1234").unwrap());
    for bad in ["", "K7QX-M2P", "K7QX-M2PAA", "K7QX-M2PU", "K7QX/M2PA"] {
        assert!(code::normalize(bad).is_err(), "{bad:?}");
    }
    assert_ne!(code::id(&code::generate()).unwrap(), code::id(&code::generate()).unwrap());
}

#[tokio::test]
async fn a_typed_code_finds_the_ticket_and_the_newcomer_joins_with_it() {
    let (_relay, network) = relay().await;
    let laptop = device_on("laptop", Arc::new(MemoryStore::default()), &network).await;
    laptop.node.create("laptop").await.unwrap();
    let (session, shown) = ticket(&laptop).await;
    let code = code::generate();
    let host = Shown::start(&code, shown.encode().unwrap(), &network).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), host.endpoint().online()).await.unwrap();

    let phone = device_on("phone", Arc::new(MemoryStore::default()), &network).await;
    let typed = code.to_lowercase().replace('-', " ");
    let found = code::resolve(phone.node.endpoint(), relay_url(&network), &typed).await.unwrap();
    assert_eq!(found, shown);

    let joining = ceremony::redeem(phone.node.endpoint(), &found, Ask::Device { name: "phone".into() }).await.unwrap();
    assert_eq!(laptop.pairings.get(session).unwrap().fingerprint.as_deref(), Some(joining.fingerprint.as_str()));
    laptop.node.append(Op::Add { device: phone.node.id(), name: "phone".into() }).await.unwrap();
    laptop.pairings.approve(session, Approval::Device { snapshot: laptop.node.snapshot().await.unwrap() }).unwrap();
    match joining.wait(Duration::from_secs(10)).await.unwrap() {
        Some(Approval::Device { snapshot }) => phone.node.adopt(snapshot).await.unwrap(),
        other => panic!("expected the list, got {other:?}"),
    }
    assert!(phone.node.list().unwrap().is_member(&phone.node.id()));
    host.close().await;
}

#[tokio::test]
async fn a_code_no_longer_shown_finds_nothing() {
    let (_relay, network) = relay().await;
    let laptop = device_on("laptop", Arc::new(MemoryStore::default()), &network).await;
    laptop.node.create("laptop").await.unwrap();
    let (_, shown) = ticket(&laptop).await;
    let code = code::generate();
    let host = Shown::start(&code, shown.encode().unwrap(), &network).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), host.endpoint().online()).await.unwrap();
    host.close().await;

    let phone = device_on("phone", Arc::new(MemoryStore::default()), &network).await;
    let found =
        tokio::time::timeout(Duration::from_secs(5), code::resolve(phone.node.endpoint(), relay_url(&network), &code))
            .await;
    assert!(!matches!(found, Ok(Ok(_))), "a closed code still answered");
}
