use erisauth::Error;
use erisauth::identity::Identity;
use erisauth::pairing::{Pairings, Status};

fn phone() -> Identity {
    Identity::Iroh("aa".repeat(32))
}

fn grants(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn the_whole_ceremony_runs_pending_requested_approved_collected() {
    let pairings = Pairings::new("test-pair-v1");
    let (id, code) = pairings.open(600);
    assert_eq!(pairings.get(id).unwrap().status, Status::Pending);
    assert!(matches!(pairings.collect(&code, &phone()).unwrap(), Status::Pending));

    let session = pairings.redeem(&code, "Phone", grants(&["files:read", "files:write"]), phone()).unwrap();
    assert_eq!(session.status, Status::Requested);
    assert_eq!(session.client.as_deref(), Some("Phone"));
    let fp = session.fingerprint.clone().unwrap();
    assert_eq!(fp, phone().fingerprint("test-pair-v1", &id, &session.requested));

    assert!(matches!(pairings.collect(&code, &phone()).unwrap(), Status::Requested));
    pairings.approve(id, Some(grants(&["files:read"]))).unwrap();
    match pairings.collect(&code, &phone()).unwrap() {
        Status::Approved { granted } => assert_eq!(granted, grants(&["files:read"])),
        other => panic!("expected approval, got {other:?}"),
    }
    // Collection repeats safely after a lost response.
    assert!(matches!(pairings.collect(&code, &phone()).unwrap(), Status::Approved { .. }));
    assert_eq!(pairings.get(id).unwrap().status, Status::Collected);
}

#[tokio::test]
async fn only_the_redeemer_collects() {
    let pairings = Pairings::new("test-pair-v1");
    let (id, code) = pairings.open(600);
    pairings.redeem(&code, "Phone", grants(&["files:read"]), phone()).unwrap();
    pairings.approve(id, None).unwrap();
    let thief = Identity::Iroh("bb".repeat(32));
    assert!(matches!(pairings.collect(&code, &thief), Err(Error::Unauthorized)));
}

#[tokio::test]
async fn a_code_redeems_once() {
    let pairings = Pairings::new("test-pair-v1");
    let (_, code) = pairings.open(600);
    pairings.redeem(&code, "Phone", grants(&["files:read"]), phone()).unwrap();
    let again = pairings.redeem(&code, "Other", grants(&["files:read"]), Identity::Iroh("bb".repeat(32)));
    assert!(matches!(again, Err(Error::Conflict(_))));
}

#[tokio::test]
async fn a_forged_or_unknown_code_is_unauthorized() {
    let pairings = Pairings::new("test-pair-v1");
    let (id, code) = pairings.open(600);
    let forged = format!("{id}.AAAA");
    assert!(matches!(pairings.redeem(&forged, "x", grants(&["a"]), phone()), Err(Error::Unauthorized)));
    assert!(matches!(pairings.redeem("nonsense", "x", grants(&["a"]), phone()), Err(Error::Unauthorized)));
    let other = Pairings::new("test-pair-v1");
    assert!(matches!(other.redeem(&code, "x", grants(&["a"]), phone()), Err(Error::Unauthorized)));
}

#[tokio::test]
async fn approval_cannot_exceed_the_request() {
    let pairings = Pairings::new("test-pair-v1");
    let (id, code) = pairings.open(600);
    assert!(matches!(pairings.approve(id, None), Err(Error::Conflict(_))));
    pairings.redeem(&code, "Phone", grants(&["files:read"]), phone()).unwrap();
    assert!(matches!(pairings.approve(id, Some(grants(&["files:*"]))), Err(Error::Forbidden { .. })));
    assert!(matches!(pairings.approve(id, Some(vec![])), Err(Error::BadRequest(_))));
}

#[tokio::test]
async fn denial_is_final_and_reported_to_the_redeemer() {
    let pairings = Pairings::new("test-pair-v1");
    let (id, code) = pairings.open(600);
    pairings.redeem(&code, "Phone", grants(&["files:read"]), phone()).unwrap();
    pairings.deny(id).unwrap();
    assert!(matches!(pairings.collect(&code, &phone()).unwrap(), Status::Denied));
    assert!(pairings.approve(id, None).is_err());
}

#[tokio::test]
async fn an_expired_code_does_nothing() {
    let pairings = Pairings::new("test-pair-v1");
    let (id, code) = pairings.open(0);
    assert!(matches!(pairings.redeem(&code, "x", grants(&["a"]), phone()), Err(Error::Expired)));
    assert!(pairings.get(id).is_none());
}

#[tokio::test]
async fn requested_sessions_are_listed_and_announced() {
    let pairings = Pairings::new("test-pair-v1");
    let mut changes = pairings.subscribe();
    let (id, code) = pairings.open(600);
    pairings.redeem(&code, "Phone", grants(&["files:read"]), phone()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), changes.changed()).await.unwrap().unwrap();
    let requested: Vec<_> = pairings.list().into_iter().filter(|s| s.status == Status::Requested).collect();
    assert_eq!(requested.len(), 1);
    assert_eq!(requested[0].id, id);
}

#[tokio::test]
async fn a_requested_request_needs_valid_grants_and_a_name() {
    let pairings = Pairings::new("test-pair-v1");
    let (_, code) = pairings.open(600);
    assert!(pairings.redeem(&code, "", grants(&["files:read"]), phone()).is_err());
    assert!(pairings.redeem(&code, "Phone", grants(&["BAD"]), phone()).is_err());
    assert!(pairings.redeem(&code, "Phone\n", grants(&["files:read"]), phone()).is_err());
}
