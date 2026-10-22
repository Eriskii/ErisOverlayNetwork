//! A participant's Iroh endpoint, and the addresses it gives others.

use std::time::Duration;

use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, RelayMap, RelayMode, SecretKey};

use crate::error::{Error, Result};

/// How long dialing a participant may take before it counts as unreachable.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How a participant reaches the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Network {
    /// n0's relays, hole punching and DNS lookup, so participants find each other anywhere by
    /// endpoint id, and mDNS, so participants on one network find each other with no internet.
    Internet,
    /// One relay, and nothing else: no DNS lookup, no mDNS. For a mesh behind its own relay,
    /// and for tests with browsers in them, which reach nothing but relays.
    Relay(iroh::RelayUrl),
    /// Loopback sockets only: no relays and no lookup. Participants know each other's
    /// addresses from tickets and presences alone. For tests.
    Loopback,
}

impl Network {
    /// `ERIS_NETWORK`, read as [`Network::from_str`] reads; the internet when it is unset or
    /// does not read.
    pub fn from_env() -> Self {
        let value = std::env::var("ERIS_NETWORK").unwrap_or_default();
        value.parse().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ERIS_NETWORK does not read; using the internet");
            Network::Internet
        })
    }
}

impl Network {
    /// The one relay a relay-only network reaches others through.
    pub fn relay(&self) -> Option<iroh::RelayUrl> {
        match self {
            Network::Relay(url) => Some(url.clone()),
            Network::Internet | Network::Loopback => None,
        }
    }
}

impl std::str::FromStr for Network {
    type Err = Error;

    /// `internet` or nothing, `loopback`, or a relay's `http://` or `https://` URL.
    fn from_str(s: &str) -> Result<Self> {
        match s.trim() {
            "" | "internet" => Ok(Network::Internet),
            "loopback" => Ok(Network::Loopback),
            url if url.starts_with("http://") || url.starts_with("https://") => url
                .parse()
                .map(Network::Relay)
                .map_err(|e| Error::BadRequest(format!("{url:?} is not a relay URL: {e}"))),
            other => Err(Error::BadRequest(format!("{other:?} is not a network: internet, loopback, or a relay URL"))),
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::Internet => f.write_str("internet"),
            Network::Loopback => f.write_str("loopback"),
            Network::Relay(url) => write!(f, "{url}"),
        }
    }
}

/// Bind an endpoint as `key`. Its protocols are its router's to register.
pub async fn endpoint(key: SecretKey, network: &Network) -> Result<Endpoint> {
    let bound = match network {
        Network::Internet => {
            let ep = Endpoint::builder(presets::N0).secret_key(key).bind().await.map_err(internal)?;
            match iroh_mdns_address_lookup::MdnsAddressLookup::builder().build(ep.id()) {
                Ok(mdns) => ep.address_lookup().map_err(internal)?.add(mdns),
                Err(e) => tracing::warn!(error = %e, "local-network discovery is off"),
            }
            ep
        }
        Network::Relay(url) => Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .relay_mode(RelayMode::Custom(RelayMap::from(url.clone())))
            .bind()
            .await
            .map_err(internal)?,
        Network::Loopback => Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .relay_mode(RelayMode::Disabled)
            .clear_address_lookup()
            .clear_ip_transports()
            .bind_addr("127.0.0.1:0")
            .map_err(internal)?
            .bind()
            .await
            .map_err(internal)?,
    };
    Ok(bound)
}

/// The address to hand another participant: the endpoint id and whatever reaches it.
pub async fn advertised(ep: &Endpoint, network: &Network) -> EndpointAddr {
    match network {
        Network::Loopback => {
            EndpointAddr::from_parts(ep.id(), ep.bound_sockets().into_iter().map(iroh::TransportAddr::Ip))
        }
        Network::Internet | Network::Relay(_) => {
            let _ = n0_future::time::timeout(Duration::from_secs(3), ep.online()).await;
            ep.addr()
        }
    }
}

fn internal(e: impl std::fmt::Display) -> Error {
    Error::Internal(format!("binding the endpoint: {e}"))
}
