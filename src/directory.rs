//! Where participants are, and what they are.
//!
//! A participant says where it can be reached in a [`Presence`]: its endpoint id, direct
//! addresses, relay, and its [`Role`] — a member device, or an app with its note. Presences
//! travel from participant to participant. Only the role is checked, against the device list;
//! addresses are hints, since dialing an endpoint id proves who answers.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::devices::{DeviceList, MeshId};
use crate::error::{Error, Result};
use crate::note::Note;
use crate::signed::Signed;

/// The most addresses one presence carries.
pub const MAX_ADDRS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Role {
    /// A member device: ErisAuth on it.
    Device,
    /// An app a member device approved.
    App { note: Note },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presence {
    pub id: iroh::EndpointId,
    #[serde(flatten)]
    pub role: Role,
    #[serde(default)]
    pub addrs: Vec<SocketAddr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    /// Unix seconds when the participant said this, by its clock. The newest wins.
    pub at: i64,
}

impl Presence {
    /// Whether this presence describes a participant of `list`'s mesh.
    pub fn check(&self, list: &DeviceList) -> Result<()> {
        if self.addrs.len() > MAX_ADDRS {
            return Err(Error::BadRequest(format!("a presence carries at most {MAX_ADDRS} addresses")));
        }
        match &self.role {
            Role::Device if list.is_member(&self.id) => Ok(()),
            Role::Device => Err(Error::Forbidden { permission: "a device in the mesh".into() }),
            Role::App { note } if note.app() == self.id => note.check(list),
            Role::App { .. } => Err(Error::Unauthorized),
        }
    }

    pub fn note(&self) -> Option<&Note> {
        match &self.role {
            Role::App { note } => Some(note),
            Role::Device => None,
        }
    }

    /// The Iroh address these hints give.
    pub fn endpoint_addr(&self) -> iroh::EndpointAddr {
        let relay = self.relay.as_deref().and_then(|r| r.parse::<iroh::RelayUrl>().ok());
        iroh::EndpointAddr::from_parts(
            self.id,
            self.addrs.iter().copied().map(iroh::TransportAddr::Ip).chain(relay.map(iroh::TransportAddr::Relay)),
        )
    }
}

/// Everything a newcomer needs to take part: the list's entries and who is where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub mesh: MeshId,
    pub entries: Vec<Signed>,
    #[serde(default)]
    pub directory: Vec<Presence>,
}

impl Snapshot {
    /// The list the entries make, once it is checked to be `mesh`'s.
    pub fn list(&self) -> Result<DeviceList> {
        let list = DeviceList::from_entries(self.entries.iter().cloned())?;
        if list.mesh() != self.mesh {
            return Err(Error::BadRequest("these entries are another mesh's".into()));
        }
        Ok(list)
    }

    /// The one app providing `service`, for a service a mesh has one of, such as ErisDB: of
    /// the apps approved to serve it, the most recently approved.
    pub fn provider(&self, service: &str) -> Option<&Presence> {
        self.providers(service).into_iter().max_by_key(|p| p.note().map(|n| (n.body.at, n.id)))
    }

    /// The participants serving `service`, as far as this snapshot knows.
    pub fn providers(&self, service: &str) -> Vec<&Presence> {
        let Ok(list) = self.list() else { return Vec::new() };
        self.directory
            .iter()
            .filter(|p| p.check(&list).is_ok())
            .filter(|p| p.note().is_some_and(|n| n.body.serves && n.body.service == service))
            .collect()
    }
}
