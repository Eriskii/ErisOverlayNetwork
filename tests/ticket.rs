use erisoverlaynetwork::ticket::{JOIN_SCHEME, Kind, SCHEME, Ticket, VERSION};

fn eid() -> iroh::EndpointId {
    iroh::SecretKey::generate().public()
}

#[test]
fn a_ticket_round_trips_through_its_uri() {
    let id = eid();
    let addr = iroh::EndpointAddr::from_parts(
        id,
        [
            iroh::TransportAddr::Ip("192.168.1.4:4919".parse().unwrap()),
            iroh::TransportAddr::Relay("https://relay.example.com".parse().unwrap()),
        ],
    );
    let ticket = Ticket::new("code".into(), id, Some("laptop".into())).unwrap().at(&addr);
    let encoded = ticket.encode().unwrap();
    assert!(encoded.starts_with(SCHEME));
    assert!(!encoded.contains('='));
    assert_eq!(Ticket::parse(&encoded).unwrap(), ticket);
    assert_eq!(ticket.v, VERSION);
    assert_eq!(ticket.endpoint_addr(), addr);
}

#[test]
fn a_ticket_needs_a_code() {
    assert!(Ticket::new("".into(), eid(), None).is_err());
}

#[test]
fn damaged_foreign_or_unknown_version_tickets_are_refused() {
    assert!(Ticket::parse("eris://pair/!!!").is_err());
    assert!(Ticket::parse("eris://pair/e30").is_err());
    assert!(Ticket::parse("erisdb://pair/e30").is_err());
    use base64::Engine;
    let v1 =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(r#"{{"v":1,"eid":"{}","token":"t"}}"#, eid()));
    assert!(Ticket::parse(&format!("{SCHEME}{v1}")).is_err());
}

#[test]
fn a_join_ticket_names_its_device_and_says_what_it_is() {
    let id = eid();
    let ticket = Ticket::join("code".into(), id, "mac".into()).unwrap();
    let encoded = ticket.encode().unwrap();
    assert!(encoded.starts_with(JOIN_SCHEME));
    let read = Ticket::parse(&encoded).unwrap();
    assert_eq!(read.kind, Kind::Join);
    assert_eq!(read, ticket);
    assert_eq!(
        Ticket::parse(&Ticket::new("code".into(), id, None).unwrap().encode().unwrap()).unwrap().kind,
        Kind::Pair
    );
    assert!(Ticket::join("code".into(), id, " ".into()).is_err());
    // A join ticket without a name does not read.
    let nameless = Ticket::new("code".into(), id, None).unwrap().encode().unwrap();
    assert!(Ticket::parse(&nameless.replacen(SCHEME, JOIN_SCHEME, 1)).is_err());
}
