//! Adding a device that shows its own ticket: a device already in scans it, the newcomer's
//! person accepts, and the newcomer is in.

mod support;

use std::time::Duration;

use erisoverlaynetwork::Error;
use erisoverlaynetwork::ceremony::{self, Ask};
use erisoverlaynetwork::devices::Op;
use erisoverlaynetwork::join::{self, Status};
use erisoverlaynetwork::ticket::{Kind, Ticket};
use support::{Device, device, eventually, join, join_ticket, ticket};

/// `member` adds `newcomer`, as `eon` does once the newcomer's person accepts.
async fn add(member: &Device, offering: &join::Offering) {
    assert!(offering.wait(Duration::from_secs(10)).await.unwrap(), "the newcomer accepted");
    member.node.append(Op::Add { device: offering.newcomer(), name: offering.name().to_string() }).await.unwrap();
    offering.welcome(member.node.snapshot().await.unwrap()).await.unwrap();
}

#[tokio::test]
async fn a_device_already_in_adds_one_that_shows_its_own_ticket() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let desktop = device("desktop").await;
    join(&desktop, &laptop).await;
    let mac = device("mac").await;

    let shown = join_ticket(&mac).await;
    assert_eq!(shown.kind, Kind::Join);
    let scanned = Ticket::parse(&shown.encode().unwrap()).unwrap();
    assert!(scanned.encode().unwrap().starts_with("eris://join/"));
    assert_eq!(scanned.name.as_deref(), Some("mac"));

    let offering = join::offer(laptop.node.endpoint(), &scanned, laptop.node.snapshot().await.unwrap()).await.unwrap();
    // The newcomer's screen says who offers, and which devices it would join.
    let (status, _) = mac.offers.status().unwrap();
    assert_eq!(status, Status::Offered { by: "laptop".into(), devices: vec!["desktop".into(), "laptop".into()] });

    mac.offers.accept().unwrap();
    add(&laptop, &offering).await;
    assert!(mac.node.list().unwrap().is_member(&mac.node.id()));
    assert_eq!(mac.offers.status().unwrap().0, Status::Joined { by: "laptop".into() });
    eventually("the desktop lists the mac", || desktop.node.list().unwrap().is_member(&mac.node.id())).await;
    eventually("the mac finds the desktop", || mac.node.directory().iter().any(|p| p.id == desktop.node.id())).await;
}

#[tokio::test]
async fn a_newcomer_turns_an_offer_down_and_another_device_adds_it() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let desktop = device("desktop").await;
    join(&desktop, &laptop).await;
    let mac = device("mac").await;
    let shown = join_ticket(&mac).await;

    let turned_down = join::offer(laptop.node.endpoint(), &shown, laptop.node.snapshot().await.unwrap()).await.unwrap();
    mac.offers.decline().unwrap();
    assert!(!turned_down.wait(Duration::from_secs(10)).await.unwrap());
    // The ticket stays shown; the device turned down cannot offer again on it, and another can.
    assert_eq!(mac.offers.status().unwrap().0, Status::Showing);
    let again = join::offer(laptop.node.endpoint(), &shown, laptop.node.snapshot().await.unwrap()).await;
    assert!(matches!(again, Err(Error::Forbidden { .. })));
    let offering = join::offer(desktop.node.endpoint(), &shown, desktop.node.snapshot().await.unwrap()).await.unwrap();
    mac.offers.accept().unwrap();
    add(&desktop, &offering).await;
    assert!(mac.node.list().unwrap().is_member(&mac.node.id()));
}

#[tokio::test]
async fn an_offer_without_the_code_changes_nothing() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let mac = device("mac").await;
    let shown = join_ticket(&mac).await;
    let forged = Ticket { token: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(), ..shown };
    let refused = join::offer(laptop.node.endpoint(), &forged, laptop.node.snapshot().await.unwrap()).await;
    assert!(matches!(refused, Err(Error::Unauthorized)));
    assert_eq!(mac.offers.status().unwrap().0, Status::Showing);
}

#[tokio::test]
async fn nothing_is_handed_over_before_the_newcomer_accepts() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let mac = device("mac").await;
    let shown = join_ticket(&mac).await;
    let offering = join::offer(laptop.node.endpoint(), &shown, laptop.node.snapshot().await.unwrap()).await.unwrap();
    laptop.node.append(Op::Add { device: mac.node.id(), name: "mac".into() }).await.unwrap();
    let early = offering.welcome(laptop.node.snapshot().await.unwrap()).await;
    assert!(matches!(early, Err(Error::Conflict(_))));
    assert!(mac.node.list().is_none());
}

#[tokio::test]
async fn each_kind_of_ticket_goes_only_its_own_way() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let mac = device("mac").await;
    let to_be_added = join_ticket(&mac).await;
    let redeemed = ceremony::redeem(laptop.node.endpoint(), &to_be_added, Ask::Device { name: "laptop".into() }).await;
    assert!(matches!(redeemed, Err(Error::BadRequest(_))));
    let (_, letting_in) = ticket(&laptop).await;
    let offered = join::offer(mac.node.endpoint(), &letting_in, laptop.node.snapshot().await.unwrap()).await;
    assert!(matches!(offered, Err(Error::BadRequest(_))));
}
