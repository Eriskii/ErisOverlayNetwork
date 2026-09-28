use erisauth::permission::{check_grant, check_grants, covers, encloses, granted, intersection, is_segment};

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_trailing_star_covers_every_remaining_segment() {
    assert!(covers("*", "meta:pairing:approve"));
    assert!(covers("meta:*", "meta:pairing:approve"));
    assert!(covers("files:read", "files:read"));
    assert!(!covers("files:read", "files:write"));
}

#[test]
fn an_inner_star_covers_exactly_one_segment() {
    assert!(covers("*:read", "files:read"));
    assert!(!covers("*:read", "meta:pairing:read"));
}

#[test]
fn a_concrete_grant_does_not_cover_a_wildcard() {
    assert!(!covers("files:read", "files:*"));
    assert!(covers("files:*", "files:*"));
}

#[test]
fn enclosure_is_subsumption_of_every_child_grant() {
    assert!(encloses(&v(&["files:*"]), &v(&["files:read", "files:write"])));
    assert!(!encloses(&v(&["files:read"]), &v(&["files:read", "files:write"])));
    assert!(granted(&v(&["a:b", "files:read"]), "files:read"));
}

#[test]
fn intersection_resolves_crossing_wildcards() {
    assert_eq!(intersection(&v(&["tasks:*"]), &v(&["*:read"])), v(&["tasks:read"]));
    assert_eq!(intersection(&v(&["*"]), &v(&["files:read"])), v(&["files:read"]));
    assert!(intersection(&v(&["files:read"]), &v(&["files:write"])).is_empty());
}

#[test]
fn grants_are_lowercase_segments_or_stars() {
    assert!(check_grant("files:read").is_ok());
    assert!(check_grant("*").is_ok());
    assert!(check_grant("Files:read").is_err());
    assert!(check_grant("").is_err());
    assert!(check_grant("files::read").is_err());
    assert!(is_segment("a.b_c-9", false));
    assert!(!is_segment("*", false));
    assert!(is_segment("*", true));
}

#[test]
fn a_grant_set_is_bounded_and_nonempty() {
    assert!(check_grants(&v(&["files:read"])).is_ok());
    assert!(check_grants(&[]).is_err());
    let many: Vec<String> = (0..65).map(|i| format!("g{i}")).collect();
    assert!(check_grants(&many).is_err());
    assert!(check_grants(&[format!("a{}", "b".repeat(200))]).is_err());
}
