use erislogin::Error;
use erislogin::token::{open, seal};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Claim {
    who: String,
    n: u32,
}

const SECRET: &[u8] = b"a secret that signs";

#[test]
fn a_sealed_payload_opens_with_the_same_prefix_and_secret() {
    let claim = Claim { who: "phone".into(), n: 7 };
    let token = seal("app1", SECRET, &claim).unwrap();
    assert!(token.starts_with("app1."));
    assert_eq!(token.split('.').count(), 3);
    let back: Claim = open("app1", SECRET, &token).unwrap();
    assert_eq!(back, claim);
}

#[test]
fn a_different_secret_or_prefix_is_unauthorized() {
    let token = seal("app1", SECRET, &Claim { who: "x".into(), n: 1 }).unwrap();
    assert!(matches!(open::<Claim>("app1", b"other", &token), Err(Error::Unauthorized)));
    assert!(matches!(open::<Claim>("app2", SECRET, &token), Err(Error::Unauthorized)));
}

#[test]
fn a_tampered_or_malformed_token_is_unauthorized() {
    let token = seal("app1", SECRET, &Claim { who: "x".into(), n: 1 }).unwrap();
    let mut parts: Vec<&str> = token.split('.').collect();
    let forged = seal("app1", b"forger", &Claim { who: "y".into(), n: 2 }).unwrap();
    let forged_payload = forged.split('.').nth(1).unwrap().to_string();
    parts[1] = &forged_payload;
    assert!(open::<Claim>("app1", SECRET, &parts.join(".")).is_err());
    assert!(open::<Claim>("app1", SECRET, "app1.only-two").is_err());
    assert!(open::<Claim>("app1", SECRET, &format!("{token}.extra")).is_err());
    assert!(open::<Claim>("app1", SECRET, "").is_err());
}
