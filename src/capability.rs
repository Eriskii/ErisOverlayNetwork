//! Capabilities: what an app uses and what it serves, named for what they are.
//!
//! A capability is a kind and a version: `database/1`, `files/1`. The version names a format:
//! everything serving `database/1` takes the same requests and answers them the same way, over
//! the same ALPN, `eris/database/1`. So an app asks for a database, not for a program: whatever
//! serves `database/1` in the mesh will do.
//!
//! An app's [note](crate::note) says what it uses, with the grants it holds in each, and what it
//! serves. Serving says two more things: whether a mesh has a [single](Serves::single) server of
//! it, the most recently approved, and whether apps that use it also need its
//! [passphrase](Serves::passphrase), which the server checks itself
//! ([`enroll`](crate::enroll)).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The most capabilities one app uses, and the most it serves.
pub const MAX_CAPABILITIES: usize = 16;

/// A kind and a version, `database/1`: `[a-z][a-z0-9-]*`, at most 32 characters, then a version
/// from 1 to 9999.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Capability(String);

impl Capability {
    pub fn new(s: &str) -> Result<Self> {
        let bad = || Error::BadRequest(format!("{s:?} is not a capability: a kind and a version, like database/1"));
        let (kind, version) = s.split_once('/').ok_or_else(bad)?;
        let kind_ok = (1..=32).contains(&kind.len())
            && kind.starts_with(|c: char| c.is_ascii_lowercase())
            && kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        let version_ok = (1..=4).contains(&version.len())
            && !version.starts_with('0')
            && version.chars().all(|c| c.is_ascii_digit());
        if kind_ok && version_ok { Ok(Capability(s.to_string())) } else { Err(bad()) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What it is: `database`.
    pub fn kind(&self) -> &str {
        self.0.split_once('/').map_or(&self.0, |(kind, _)| kind)
    }

    /// Which format of it: `1`.
    pub fn version(&self) -> u32 {
        self.0.split_once('/').and_then(|(_, v)| v.parse().ok()).unwrap_or_default()
    }

    /// The ALPN its servers answer on: `eris/database/1`.
    pub fn alpn(&self) -> Vec<u8> {
        format!("eris/{}", self.0).into_bytes()
    }
}

impl std::str::FromStr for Capability {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Capability::new(s)
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::borrow::Borrow<str> for Capability {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Capability::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Serving a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Serves {
    pub capability: Capability,
    /// A mesh has one server of it: the most recently approved. Approving another replaces it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub single: bool,
    /// An app that uses it also needs the server's passphrase, which a device brings the server
    /// along with the app's note and the server checks itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub passphrase: bool,
}

impl Serves {
    pub fn new(capability: Capability) -> Self {
        Serves { capability, single: false, passphrase: false }
    }
}

/// What an app uses: each capability, with the grants it holds there. A capability with no
/// grants is one it may reach and do nothing in that needs one.
pub type Uses = BTreeMap<Capability, Vec<String>>;

/// Check what an app asks for, or what a note approves: at least one capability, no more than
/// [`MAX_CAPABILITIES`] used or served, each served once, and grants that read.
pub fn check(uses: &Uses, serves: &[Serves]) -> Result<()> {
    if uses.is_empty() && serves.is_empty() {
        return Err(Error::BadRequest("an app uses or serves at least one capability".into()));
    }
    if uses.len() > MAX_CAPABILITIES || serves.len() > MAX_CAPABILITIES {
        return Err(Error::BadRequest(format!("an app uses, and serves, at most {MAX_CAPABILITIES} capabilities")));
    }
    for (i, served) in serves.iter().enumerate() {
        if serves[..i].iter().any(|s| s.capability == served.capability) {
            return Err(Error::BadRequest(format!("{} is served twice", served.capability)));
        }
    }
    for grants in uses.values().filter(|g| !g.is_empty()) {
        crate::permission::check_grants(grants)?;
    }
    Ok(())
}

/// Whether `approved` keeps within `asked`: only capabilities asked for, each with only grants
/// asked for, and serving only as asked.
pub fn within(asked: (&Uses, &[Serves]), approved: (&Uses, &[Serves])) -> bool {
    let uses = approved.0.iter().all(|(capability, grants)| {
        asked.0.get(capability).is_some_and(|asked| crate::permission::encloses(asked, grants))
    });
    let serves = approved.1.iter().all(|served| asked.1.contains(served));
    uses && serves
}
