use erislogin::ticket::{Ticket, VERSION, scheme};

const EID: &str = "e718b50236b0b98637fbf39cb4040e79800094313dc195e221e8e075304a6a06";

#[test]
fn the_scheme_names_the_app() {
    assert_eq!(scheme("erisfiles"), "erisfiles://pair/");
}

#[test]
fn a_ticket_round_trips_through_its_uri() {
    let ticket = Ticket::new("code".into(), Some(EID.into()), None, Some("laptop".into()))
        .unwrap()
        .with_addrs(vec!["192.168.1.4:4919".parse().unwrap()]);
    let encoded = ticket.encode("erisfiles").unwrap();
    assert!(encoded.starts_with("erisfiles://pair/"));
    assert!(!encoded.contains('='));
    assert_eq!(Ticket::parse("erisfiles", &encoded).unwrap(), ticket);
    assert_eq!(ticket.v, VERSION);
}

#[test]
fn a_ticket_for_another_app_is_refused() {
    let encoded = Ticket::new("code".into(), Some(EID.into()), None, None).unwrap().encode("erisdb").unwrap();
    assert!(Ticket::parse("erisfiles", &encoded).is_err());
}

#[test]
fn a_ticket_needs_a_token_and_an_address() {
    assert!(Ticket::new("".into(), Some(EID.into()), None, None).is_err());
    assert!(Ticket::new("code".into(), None, None, None).is_err());
    assert!(Ticket::new("code".into(), None, Some("http://127.0.0.1:7700".into()), None).is_ok());
    assert!(Ticket::new("code".into(), Some("abc".into()), None, None).is_err());
}

#[test]
fn damaged_or_unknown_version_tickets_are_refused() {
    assert!(Ticket::parse("erisfiles", "erisfiles://pair/!!!").is_err());
    assert!(Ticket::parse("erisfiles", "erisfiles://pair/e30").is_err());
    use base64::Engine;
    let v2 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(r#"{{"v":2,"eid":"{EID}","token":"t"}}"#));
    assert!(Ticket::parse("erisfiles", &format!("erisfiles://pair/{v2}")).is_err());
}

#[test]
fn a_ticket_without_addrs_encodes_as_it_always_has() {
    let ticket = Ticket::new("code".into(), Some(EID.into()), None, Some("core".into())).unwrap();
    let encoded = ticket.encode("erisdb").unwrap();
    use base64::Engine;
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded.strip_prefix("erisdb://pair/").unwrap())
        .unwrap();
    assert_eq!(String::from_utf8(json).unwrap(), format!(r#"{{"v":1,"name":"core","eid":"{EID}","token":"code"}}"#));
}

#[test]
fn a_ticket_gives_the_address_to_dial() {
    let ticket = Ticket::new("code".into(), Some(EID.into()), None, None)
        .unwrap()
        .with_addrs(vec!["10.0.0.2:5000".parse().unwrap()]);
    let addr = ticket.endpoint_addr().unwrap();
    assert_eq!(addr.id.to_string(), EID);
    assert_eq!(addr.ip_addrs().count(), 1);
}
