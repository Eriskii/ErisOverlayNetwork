use erisoverlaynetwork::key;

#[test]
fn a_derived_key_is_stable_per_secret_and_tag() {
    let a = key::derive(b"secret", b"app/iroh/0");
    let b = key::derive(b"secret", b"app/iroh/0");
    let c = key::derive(b"secret", b"other/iroh/0");
    let d = key::derive(b"other", b"app/iroh/0");
    assert_eq!(a.public(), b.public());
    assert_ne!(a.public(), c.public());
    assert_ne!(a.public(), d.public());
}

#[test]
fn a_key_file_is_created_once_then_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("key");
    let first = key::load_or_create(&path).unwrap();
    let second = key::load_or_create(&path).unwrap();
    assert_eq!(first.public(), second.public());
    assert_eq!(std::fs::read(&path).unwrap().len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn a_damaged_key_file_is_an_error_not_a_new_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key");
    std::fs::write(&path, b"short").unwrap();
    assert!(key::load_or_create(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"short");
}
