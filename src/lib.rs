//! erislogin: how an Eris app lets one device or app into another.
//!
//! Pairing is a ceremony between two screens and a person. The host shows a
//! [ticket](ticket) — its address and a one-time code — as a [QR code](qr).
//! The newcomer redeems the code, naming itself and the [permissions] it
//! wants, and both screens show a [fingerprint](identity::fingerprint) of the
//! request. The person compares them and approves all, some, or none of it
//! ([prompt] in a terminal, or any UI over [pairing::Pairings]). Only the
//! [identity] that redeemed the code can collect the answer.
//!
//! Identity on the wire is an Iroh key: [key] derives or stores one, and the
//! QUIC handshake proves it. Browsers, which hold no key, commit to a random
//! secret instead. Durable authority can be handed out as HMAC-signed
//! [tokens](token) scoped by permission patterns.

pub mod error;
pub mod identity;
#[cfg(feature = "iroh")]
pub mod key;
pub mod pairing;
pub mod permission;
#[cfg(feature = "prompt")]
pub mod prompt;
#[cfg(feature = "qr")]
pub mod qr;
pub mod ticket;
pub mod token;

pub use error::{Error, Result};

/// Unix seconds now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
