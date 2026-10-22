//! Tickets: where a device is and what to say to it, as one scannable string.
//!
//! Two kinds, by what the device showing it wants:
//!
//! - `eris://pair/<base64url-nopad(JSON)>`: a device in a mesh shows it to let someone in, a new
//!   device or an app ([ceremony](crate::ceremony));
//! - `eris://join/<base64url-nopad(JSON)>`: a device in no mesh yet shows it to be added by a
//!   device that is ([join](crate::join)).
//!
//! The payload carries a one-time code and the device's Iroh endpoint id. Direct socket
//! addresses, when present, let a device on the same network dial without waiting on discovery;
//! a relay URL lets a browser, which can reach nothing but relays, find it.
//!
//! A ticket is worth exactly the code inside it: a request that a person still has to approve.

use std::net::SocketAddr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The only ticket version there is. A client that meets a version it does
/// not know refuses the ticket rather than guessing at the fields.
pub const VERSION: u32 = 2;

/// The prefix of a ticket a device in a mesh shows. Everything after it is the payload.
pub const SCHEME: &str = "eris://pair/";

/// The prefix of a ticket a device in no mesh yet shows.
pub const JOIN_SCHEME: &str = "eris://join/";

/// What the device showing a ticket wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// To let someone in: `eris://pair/`.
    #[default]
    Pair,
    /// To be added: `eris://join/`.
    Join,
}

impl Kind {
    pub fn scheme(self) -> &'static str {
        match self {
            Kind::Pair => SCHEME,
            Kind::Join => JOIN_SCHEME,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    /// Said by the scheme, not the payload.
    #[serde(skip)]
    pub kind: Kind,
    pub v: u32,
    /// The device's name, for the human: *joining laptop*. Never trusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The device's Iroh endpoint id.
    pub eid: iroh::EndpointId,
    /// Direct socket addresses of the endpoint, for dialing without discovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addrs: Vec<SocketAddr>,
    /// The relay the endpoint is reachable through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    pub token: String,
}

impl Ticket {
    /// A ticket a device in a mesh shows, to let someone in.
    pub fn new(token: String, eid: iroh::EndpointId, name: Option<String>) -> Result<Self> {
        let ticket = Ticket { kind: Kind::Pair, v: VERSION, name, eid, addrs: Vec::new(), relay: None, token };
        ticket.check()?;
        Ok(ticket)
    }

    /// A ticket a device in no mesh yet shows, to be added under `name`.
    pub fn join(token: String, eid: iroh::EndpointId, name: String) -> Result<Self> {
        let ticket = Ticket { kind: Kind::Join, ..Ticket::new(token, eid, Some(crate::check_name(&name)?))? };
        ticket.check()?;
        Ok(ticket)
    }

    /// The same ticket, also naming where its endpoint is.
    pub fn at(mut self, addr: &iroh::EndpointAddr) -> Self {
        self.addrs = addr.ip_addrs().copied().collect();
        self.relay = addr.relay_urls().next().map(|r| r.to_string());
        self
    }

    fn check(&self) -> Result<()> {
        if self.v != VERSION {
            return Err(Error::BadRequest(format!(
                "ticket version {} is not supported; this side speaks v{VERSION}",
                self.v
            )));
        }
        if self.token.is_empty() {
            return Err(Error::BadRequest("ticket carries no code".into()));
        }
        if self.kind == Kind::Join && self.name.as_deref().is_none_or(|name| crate::check_name(name).is_err()) {
            return Err(Error::BadRequest("a ticket to be added names the device".into()));
        }
        Ok(())
    }

    /// The scannable string.
    pub fn encode(&self) -> Result<String> {
        let json = serde_json::to_vec(self).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(format!("{}{}", self.kind.scheme(), B64.encode(json)))
    }

    /// Read a ticket of either kind.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let (kind, payload) = [Kind::Pair, Kind::Join]
            .into_iter()
            .find_map(|kind| s.strip_prefix(kind.scheme()).map(|payload| (kind, payload)))
            .ok_or_else(|| Error::BadRequest(format!("a ticket starts with {SCHEME} or {JOIN_SCHEME}")))?;
        let json = B64.decode(payload).map_err(|_| Error::BadRequest("ticket payload is not base64url".into()))?;
        let mut ticket: Ticket = serde_json::from_slice(&json)
            .map_err(|e| Error::BadRequest(format!("ticket payload is not a ticket: {e}")))?;
        ticket.kind = kind;
        ticket.check()?;
        Ok(ticket)
    }

    /// The Iroh address to dial.
    pub fn endpoint_addr(&self) -> iroh::EndpointAddr {
        let relay = self.relay.as_deref().and_then(|r| r.parse::<iroh::RelayUrl>().ok());
        iroh::EndpointAddr::from_parts(
            self.eid,
            self.addrs.iter().copied().map(iroh::TransportAddr::Ip).chain(relay.map(iroh::TransportAddr::Relay)),
        )
    }
}
