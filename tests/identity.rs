use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use erislogin::identity::{Identity, fingerprint};
use sha2::Digest;

const KEY: &str = "e718b50236b0b98637fbf39cb4040e79800094313dc195e221e8e075304a6a06";

#[test]
fn an_iroh_peer_enrolls_by_its_key() {
    let id = Identity::enrollment(Some(&format!("iroh:{KEY}")), None).unwrap();
    assert_eq!(id, Identity::Iroh(KEY.into()));
    assert!(id.prove(Some(&format!("iroh:{KEY}")), None).is_ok());
    assert!(id.prove(Some("iroh:ffff"), None).is_err());
    assert!(id.prove(Some("127.0.0.1:5000"), None).is_err());
}

#[test]
fn a_browser_enrolls_by_an_s256_commitment_over_loopback() {
    let verifier = "a".repeat(43);
    let challenge = B64.encode(sha2::Sha256::digest(verifier.as_bytes()));
    let id = Identity::enrollment(Some("127.0.0.1:5000"), Some(challenge.clone())).unwrap();
    assert_eq!(id, Identity::Browser(challenge.clone()));
    assert!(id.prove(Some("127.0.0.1:6000"), Some(&verifier)).is_ok());
    assert!(id.prove(Some("127.0.0.1:6000"), Some(&"b".repeat(43))).is_err());
    assert!(id.prove(Some("8.8.8.8:6000"), Some(&verifier)).is_err());
    assert!(Identity::enrollment(Some("8.8.8.8:5000"), Some(challenge)).is_err());
    assert!(Identity::enrollment(Some("127.0.0.1:5000"), Some("short".into())).is_err());
    assert!(Identity::enrollment(Some("127.0.0.1:5000"), None).is_err());
}

#[test]
fn identities_serialize_tagged() {
    let json = serde_json::to_string(&Identity::Iroh("k".into())).unwrap();
    assert_eq!(json, r#"{"kind":"iroh","key":"k"}"#);
}

#[test]
fn a_fingerprint_is_three_hex_pairs_of_the_transcript_digest() {
    let fp = fingerprint(&("domain", 1, "x"));
    assert_eq!(fp.len(), 14);
    let parts: Vec<&str> = fp.split('-').collect();
    assert_eq!(parts.len(), 3);
    assert!(parts.iter().all(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())));
    let digest = sha2::Sha256::digest(serde_json::to_vec(&("domain", 1, "x")).unwrap());
    assert_eq!(&fp[..2], format!("{:02X}", digest[0]));
    assert_ne!(fp, fingerprint(&("domain", 2, "x")));
}

#[test]
fn an_identity_fingerprints_a_session_request() {
    let id = Identity::Iroh(KEY.into());
    let requested = vec!["files:read".to_string()];
    let fp = id.fingerprint("app-pair-v1", &"session", &requested);
    assert_eq!(fp, fingerprint(&("app-pair-v1", "session", &id, &requested)));
}
