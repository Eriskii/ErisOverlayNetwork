//! Pairing tickets: where the host is and what to say, as one scannable
//! string.
//!
//! `<app>://pair/<base64url-nopad(JSON)>`. The payload carries a one-time
//! code and at least one address — an Iroh endpoint id, a plain HTTP URL, or
//! both, so one ticket serves a browser on the LAN and a phone anywhere.
//! Direct socket addresses, when present, let a device on the same network
//! dial without waiting on discovery.
//!
//! There is no pairing service. A ticket is self-contained, and worth
//! exactly the code inside it: a request that a person still has to approve.

use std::net::SocketAddr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The only ticket version there is. A client that meets a version it does
/// not know refuses the ticket rather than guessing at the fields.
pub const VERSION: u32 = 1;

/// The literal prefix of `app`'s tickets. Everything after it is the payload.
pub fn scheme(app: &str) -> String {
    format!("{app}://pair/")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub v: u32,
    /// A label for the human — *paired with my-laptop*. Never trusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Iroh endpoint id, 64 lowercase hex characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eid: Option<String>,
    /// Plain HTTP base URL, for clients that cannot speak QUIC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Direct socket addresses of the endpoint, for dialing without discovery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addrs: Vec<SocketAddr>,
    pub token: String,
}

impl Ticket {
    pub fn new(token: String, eid: Option<String>, url: Option<String>, name: Option<String>) -> Result<Self> {
        let ticket = Ticket { v: VERSION, name, eid, url, addrs: Vec::new(), token };
        ticket.check()?;
        Ok(ticket)
    }

    /// The same ticket, also naming direct addresses of its endpoint.
    pub fn with_addrs(mut self, addrs: Vec<SocketAddr>) -> Self {
        self.addrs = addrs;
        self
    }

    /// Everything a client is required to reject, checked in one place so
    /// building and parsing cannot disagree about what a valid ticket is.
    fn check(&self) -> Result<()> {
        if self.v != VERSION {
            return Err(Error::BadRequest(format!(
                "ticket version {} is not supported; this side speaks v{VERSION}",
                self.v
            )));
        }
        if self.token.is_empty() {
            return Err(Error::BadRequest("ticket carries no token".into()));
        }
        if self.eid.is_none() && self.url.is_none() {
            return Err(Error::BadRequest("ticket names no address: it needs an endpoint id, a url, or both".into()));
        }
        if let Some(eid) = &self.eid
            && !(eid.len() == 64 && eid.chars().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(Error::BadRequest("endpoint id must be 64 hex characters".into()));
        }
        Ok(())
    }

    /// The scannable string for `app`.
    pub fn encode(&self, app: &str) -> Result<String> {
        let json = serde_json::to_vec(self).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(format!("{}{}", scheme(app), B64.encode(json)))
    }

    /// Read one of `app`'s tickets.
    pub fn parse(app: &str, s: &str) -> Result<Self> {
        let prefix = scheme(app);
        let payload = s
            .trim()
            .strip_prefix(prefix.as_str())
            .ok_or_else(|| Error::BadRequest(format!("a pairing ticket starts with {prefix}")))?;
        let json = B64.decode(payload).map_err(|_| Error::BadRequest("ticket payload is not base64url".into()))?;
        let ticket: Ticket = serde_json::from_slice(&json)
            .map_err(|e| Error::BadRequest(format!("ticket payload is not a ticket: {e}")))?;
        ticket.check()?;
        Ok(ticket)
    }

    /// The Iroh address to dial: the endpoint id and any direct addresses.
    #[cfg(feature = "iroh")]
    pub fn endpoint_addr(&self) -> Option<iroh::EndpointAddr> {
        let id: iroh::EndpointId = self.eid.as_deref()?.parse().ok()?;
        Some(iroh::EndpointAddr::from_parts(id, self.addrs.iter().copied().map(iroh::TransportAddr::Ip)))
    }
}
