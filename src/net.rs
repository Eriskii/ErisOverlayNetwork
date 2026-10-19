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
    /// `ERIS_NETWORK`: `loopback`, a relay URL, or anything else (or nothing) for the internet.
    pub fn from_env() -> Self {
        match std::env::var("ERIS_NETWORK").ok().as_deref() {
            Some("loopback") => Network::Loopback,
            Some(url) if url.starts_with("http") => match url.parse() {
                Ok(url) => Network::Relay(url),
                Err(_) => Network::Internet,
            },
            _ => Network::Internet,
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
