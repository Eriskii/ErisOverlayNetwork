use erisoverlaynetwork::Error;
use erisoverlaynetwork::ceremony::{Approval, Ask};
use erisoverlaynetwork::devices::DeviceList;
use erisoverlaynetwork::directory::Snapshot;
use erisoverlaynetwork::pairing::{Collected, Pairings, Status, code_session, fingerprint};

fn phone() -> iroh::EndpointId {
    iroh::SecretKey::generate().public()
}

fn ask() -> Ask {
    Ask::Device { name: "phone".into() }
}

fn approval() -> Approval {
    let (list, _) = DeviceList::create(&iroh::SecretKey::generate(), "laptop").unwrap();
    Approval::Device { snapshot: Snapshot { mesh: list.mesh(), entries: list.entries(), directory: vec![] } }
}

#[test]
fn the_whole_ceremony_runs_pending_requested_approved_collected() {
    let pairings = Pairings::new();
    let me = phone();
    let (id, code) = pairings.open(600);
    assert_eq!(code_session(&code), Some(id));
    assert_eq!(pairings.get(id).unwrap().status, Status::Pending);
    assert!(matches!(pairings.collect(&code, &me).unwrap(), Collected::Waiting));

    let session = pairings.redeem(&code, ask(), me).unwrap();
    assert_eq!(session.status, Status::Requested);
    assert_eq!(session.ask, Some(ask()));
    assert_eq!(session.fingerprint.as_deref(), Some(fingerprint(&id, &me, &ask()).as_str()));
    assert!(matches!(pairings.collect(&code, &me).unwrap(), Collected::Waiting));

    let answer = approval();
    pairings.approve(id, answer.clone()).unwrap();
    assert!(matches!(pairings.collect(&code, &me).unwrap(), Collected::Approved(a) if a == answer));
    // Collection repeats safely after a lost response.
    assert!(matches!(pairings.collect(&code, &me).unwrap(), Collected::Approved(_)));
    assert_eq!(pairings.get(id).unwrap().status, Status::Collected);
}

#[test]
fn only_the_redeemer_collects() {
    let pairings = Pairings::new();
    let (id, code) = pairings.open(600);
    pairings.redeem(&code, ask(), phone()).unwrap();
    pairings.approve(id, approval()).unwrap();
    assert!(matches!(pairings.collect(&code, &phone()), Err(Error::Unauthorized)));
}

#[test]
fn a_code_redeems_once() {
    let pairings = Pairings::new();
    let (_, code) = pairings.open(600);
    pairings.redeem(&code, ask(), phone()).unwrap();
    assert!(matches!(pairings.redeem(&code, ask(), phone()), Err(Error::Conflict(_))));
}

#[test]
fn a_wrong_or_expired_code_is_refused() {
    let pairings = Pairings::new();
    let (id, code) = pairings.open(600);
    let wrong = format!("{id}.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert!(matches!(pairings.redeem(&wrong, ask(), phone()), Err(Error::Unauthorized)));
    let (_, expired) = pairings.open(-1);
    assert!(matches!(pairings.redeem(&expired, ask(), phone()), Err(Error::Expired | Error::Unauthorized)));
    assert!(pairings.redeem(&code, ask(), phone()).is_ok());
}

#[test]
fn a_denial_is_final_and_a_collected_session_cannot_be_denied() {
    let pairings = Pairings::new();
    let me = phone();
    let (id, code) = pairings.open(600);
    pairings.redeem(&code, ask(), me).unwrap();
    pairings.deny(id).unwrap();
    assert!(matches!(pairings.collect(&code, &me).unwrap(), Collected::Denied));
    assert!(pairings.approve(id, approval()).is_err());

    let (id, code) = pairings.open(600);
    pairings.redeem(&code, ask(), me).unwrap();
    pairings.approve(id, approval()).unwrap();
    pairings.collect(&code, &me).unwrap();
    assert!(pairings.deny(id).is_err());
}

#[test]
fn what_is_asked_is_checked() {
    let pairings = Pairings::new();
    let (_, code) = pairings.open(600);
    let nothing = Ask::App { name: "Notes".into(), uses: Default::default(), serves: vec![] };
    assert!(pairings.redeem(&code, nothing, phone()).is_err());
    let uses = [("notes/1".parse().unwrap(), vec!["NOPE".to_string()])].into();
    let bad = Ask::App { name: "Notes".into(), uses, serves: vec![] };
    assert!(pairings.redeem(&code, bad, phone()).is_err());
}

#[test]
fn only_the_redeemer_withdraws() {
    let pairings = Pairings::new();
    let me = phone();
    let (id, code) = pairings.open(600);
    pairings.redeem(&code, ask(), me).unwrap();
    assert!(matches!(pairings.withdraw(&code, &phone()), Err(Error::Unauthorized)));
    pairings.withdraw(&code, &me).unwrap();
    assert!(pairings.get(id).is_none());
}
