//! Tickets: where a device is and what to say to it, as one scannable string.
//!
//! `eris://pair/<base64url-nopad(JSON)>`. The payload carries a one-time code and the
//! device's Iroh endpoint id. Direct socket addresses, when present, let a device on the same
//! network dial without waiting on discovery; a relay URL lets a browser, which can reach
//! nothing but relays, find it.
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

/// The literal prefix of every ticket. Everything after it is the payload.
pub const SCHEME: &str = "eris://pair/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
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
    pub fn new(token: String, eid: iroh::EndpointId, name: Option<String>) -> Result<Self> {
        let ticket = Ticket { v: VERSION, name, eid, addrs: Vec::new(), relay: None, token };
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
        Ok(())
    }

    /// The scannable string.
    pub fn encode(&self) -> Result<String> {
        let json = serde_json::to_vec(self).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(format!("{SCHEME}{}", B64.encode(json)))
    }

    /// Read a ticket.
    pub fn parse(s: &str) -> Result<Self> {
        let payload = s
            .trim()
            .strip_prefix(SCHEME)
            .ok_or_else(|| Error::BadRequest(format!("a ticket starts with {SCHEME}")))?;
        let json = B64.decode(payload).map_err(|_| Error::BadRequest("ticket payload is not base64url".into()))?;
        let ticket: Ticket = serde_json::from_slice(&json)
            .map_err(|e| Error::BadRequest(format!("ticket payload is not a ticket: {e}")))?;
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
