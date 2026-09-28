//! Namespaced permissions.
//!
//! A required permission is concrete: `files:read`, `meta:pairing:approve`.
//! A grant is what a holder has and may wildcard: `files:*`, `meta:*`, `*`.
//! There is no registry of valid permissions; a grant naming something that
//! does not exist is legal and matches nothing until it does.

use crate::error::{Error, Result};

/// Caps on caller-supplied grant sets, which get signed and matched on every
/// request.
pub const MAX_GRANTS: usize = 64;
pub const MAX_GRANT_LEN: usize = 128;

/// True when `grant` covers `required`.
///
/// Segment by segment. `*` matches exactly one segment, and as a grant's
/// final segment it matches every remaining segment — which is what makes
/// `*` mean everything and `meta:*` reach `meta:pairing:approve`, while
/// `*:read` stops at two segments and cannot.
///
/// The same function decides whether one grant encloses another: pass the
/// child as `required`. `*` is wild on the grant side and an ordinary token
/// on the required side, which is exactly subsumption — `tasks:read` does
/// not cover `tasks:*`, because `read` is not `*`.
pub fn covers(grant: &str, required: &str) -> bool {
    let mut grants = grant.split(':').peekable();
    let mut required = required.split(':');
    while let Some(segment) = grants.next() {
        let Some(other) = required.next() else { return false };
        if segment == "*" && grants.peek().is_none() {
            return true;
        }
        if segment != "*" && segment != other {
            return false;
        }
    }
    required.next().is_none()
}

/// True when any grant in the set covers `required`.
pub fn granted(grants: &[String], required: &str) -> bool {
    grants.iter().any(|g| covers(g, required))
}

/// True when every grant in `child` is subsumed by some grant in `parent`.
pub fn encloses(parent: &[String], child: &[String]) -> bool {
    child.iter().all(|c| granted(parent, c))
}

/// The permissions both authorities grant, including crossing wildcards
/// such as `tasks:*` and `*:read`, whose intersection is `tasks:read`.
pub fn intersection(left: &[String], right: &[String]) -> Vec<String> {
    fn pattern(left: &str, right: &str) -> Option<String> {
        let a: Vec<_> = left.split(':').collect();
        let b: Vec<_> = right.split(':').collect();
        let mut result = Vec::new();
        for i in 0..a.len().max(b.len()) {
            let (&x, &y) = (a.get(i)?, b.get(i)?);
            if x == "*" && i + 1 == a.len() {
                result.extend_from_slice(&b[i..]);
                return Some(result.join(":"));
            }
            if y == "*" && i + 1 == b.len() {
                result.extend_from_slice(&a[i..]);
                return Some(result.join(":"));
            }
            result.push(match (x, y) {
                ("*", _) => y,
                (_, "*") => x,
                _ if x == y => x,
                _ => return None,
            });
        }
        Some(result.join(":"))
    }
    left.iter().flat_map(|a| right.iter().filter_map(move |b| pattern(a, b))).fold(Vec::new(), |mut grants, grant| {
        if !grants.contains(&grant) {
            grants.push(grant);
        }
        grants
    })
}

/// One permission segment: `[a-z0-9][a-z0-9._-]*`, or `*` where allowed.
pub fn is_segment(seg: &str, allow_star: bool) -> bool {
    if seg == "*" {
        return allow_star;
    }
    let mut chars = seg.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-')
}

/// A grant is segments of `[a-z0-9][a-z0-9._-]*`, or `*`, joined by `:`.
pub fn check_grant(grant: &str) -> Result<()> {
    if grant.is_empty() {
        return Err(Error::BadRequest("a grant cannot be empty".into()));
    }
    if grant.split(':').all(|s| is_segment(s, true)) {
        Ok(())
    } else {
        Err(Error::BadRequest(format!(
            "{grant:?} is not a permission: segments are [a-z0-9][a-z0-9._-]* or *, joined by :"
        )))
    }
}

/// A grant set is nonempty, at most [`MAX_GRANTS`] long, and every grant is
/// well-formed and at most [`MAX_GRANT_LEN`] bytes.
pub fn check_grants(grants: &[String]) -> Result<()> {
    if grants.is_empty() {
        return Err(Error::BadRequest("a capability needs at least one grant".into()));
    }
    if grants.len() > MAX_GRANTS {
        return Err(Error::BadRequest(format!("at most {MAX_GRANTS} grants")));
    }
    if let Some(g) = grants.iter().find(|g| g.len() > MAX_GRANT_LEN) {
        return Err(Error::BadRequest(format!("grant is longer than {MAX_GRANT_LEN}: {g:?}")));
    }
    grants.iter().try_for_each(|g| check_grant(g))
}
