use erisoverlaynetwork::capability::{self, Capability, Serves, Uses};

fn uses(pairs: &[(&str, &[&str])]) -> Uses {
    pairs.iter().map(|(c, g)| (c.parse().unwrap(), g.iter().map(|g| g.to_string()).collect())).collect()
}

#[test]
fn a_capability_is_a_kind_and_a_version() {
    let database: Capability = "database/1".parse().unwrap();
    assert_eq!(database.kind(), "database");
    assert_eq!(database.version(), 1);
    assert_eq!(database.alpn(), b"eris/database/1");
    assert_eq!(serde_json::to_string(&database).unwrap(), "\"database/1\"");
    for bad in [
        "database",
        "database/0",
        "database/01",
        "Database/1",
        "1db/1",
        "data base/1",
        "database/1/2",
        "/1",
        "database/12345",
    ] {
        assert!(bad.parse::<Capability>().is_err(), "{bad:?}");
        assert!(serde_json::from_str::<Capability>(&format!("{bad:?}")).is_err(), "{bad:?}");
    }
    assert!("files-2/9999".parse::<Capability>().is_ok());
}

#[test]
fn an_app_uses_or_serves_something_and_says_it_once() {
    let files: Capability = "files/1".parse().unwrap();
    assert!(capability::check(&Uses::new(), &[]).is_err());
    assert!(capability::check(&uses(&[("files/1", &["files:read"])]), &[]).is_ok());
    assert!(capability::check(&Uses::new(), &[Serves::new(files.clone())]).is_ok());
    assert!(capability::check(&Uses::new(), &[Serves::new(files.clone()), Serves::new(files)]).is_err());
    assert!(capability::check(&uses(&[("files/1", &["NOT A GRANT"])]), &[]).is_err());
}

#[test]
fn what_is_approved_keeps_within_what_was_asked() {
    let asked = uses(&[("database/1", &["tasks:*"]), ("files/1", &["files:read"])]);
    let serves = vec![Serves::new("files/1".parse().unwrap())];
    assert!(capability::within((&asked, &serves), (&uses(&[("database/1", &["tasks:read"])]), &[])));
    assert!(capability::within((&asked, &serves), (&asked, &serves)));
    assert!(!capability::within((&asked, &serves), (&uses(&[("database/1", &["*"])]), &[])));
    assert!(!capability::within((&asked, &serves), (&uses(&[("notes/1", &[])]), &[])));
    let single = vec![Serves { single: true, ..serves[0].clone() }];
    assert!(!capability::within((&asked, &serves), (&asked, &single)));
}
