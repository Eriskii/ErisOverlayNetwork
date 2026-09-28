//! Who is asking. Native peers prove possession of their Iroh key through
//! the QUIC handshake. Browsers hold no key, so they commit to a random
//! 256-bit secret with its S256 digest, and later prove it by revealing the
//! secret over a loopback hop. Display names never authenticate anything.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::{Error, Result};

/// The prefix a transport puts on an authenticated Iroh peer's address:
/// `iroh:<endpoint id>`. Anything else is a socket address.
pub const IROH_PEER: &str = "iroh:";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "key", rename_all = "snake_case")]
pub enum Identity {
    /// An Iroh endpoint id, as hex.
    Iroh(String),
    /// The S256 digest of a browser's secret, as unpadded base64url.
    Browser(String),
}

impl Identity {
    /// The identity a request enrolls: the Iroh key it arrived under, or,
    /// over loopback, the browser commitment it carries.
    pub fn enrollment(peer: Option<&str>, challenge: Option<String>) -> Result<Self> {
        if let Some(key) = peer.and_then(|p| p.strip_prefix(IROH_PEER)) {
            return Ok(Self::Iroh(key.to_string()));
        }
        private_hop(peer)?;
        let challenge = challenge.ok_or_else(|| {
            Error::BadRequest("browser pairing requires an S256 challenge from a random installation secret".into())
        })?;
        if !B64.decode(&challenge).is_ok_and(|bytes| bytes.len() == 32 && B64.encode(bytes) == challenge) {
            return Err(Error::BadRequest("challenge must encode a SHA-256 digest as unpadded base64url".into()));
        }
        Ok(Self::Browser(challenge))
    }

    /// Check that a request comes from this identity: the same Iroh key, or
    /// a loopback hop revealing the committed secret.
    pub fn prove(&self, peer: Option<&str>, verifier: Option<&str>) -> Result<()> {
        let valid = match self {
            Self::Iroh(key) => peer.and_then(|p| p.strip_prefix(IROH_PEER)) == Some(key),
            Self::Browser(expected) => {
                private_hop(peer).is_ok()
                    && verifier
                        .filter(|v| {
                            (43..=128).contains(&v.len())
                                && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
                        })
                        .map(|v| B64.encode(Sha256::digest(v.as_bytes())))
                        .is_some_and(|actual| actual.as_bytes().ct_eq(expected.as_bytes()).into())
            }
        };
        if valid { Ok(()) } else { Err(Error::Unauthorized) }
    }

    /// Check that an access token is used where it may be: by the same Iroh
    /// key, or for a browser, over loopback.
    pub fn bind_access(&self, peer: Option<&str>) -> Result<()> {
        match self {
            Self::Iroh(_) => self.prove(peer, None),
            Self::Browser(_) => private_hop(peer),
        }
    }

    /// The comparison code both screens show for this identity asking for
    /// `requested` in `session`, under the app's `domain` tag.
    pub fn fingerprint<S: Serialize + ?Sized>(&self, domain: &str, session: &S, requested: &[String]) -> String {
        fingerprint(&(domain, session, self, requested))
    }
}

/// Six bytes of the SHA-256 of a JSON transcript, as `12AB-34CD-56EF`.
/// Enough for two people to compare at a glance; far too many for a racing
/// redeemer to hit by chance.
pub fn fingerprint<T: Serialize + ?Sized>(transcript: &T) -> String {
    let digest = Sha256::digest(serde_json::to_vec(transcript).expect("serializable transcript"));
    format!("{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}", digest[0], digest[1], digest[2], digest[3], digest[4], digest[5])
}

/// TLS terminates at a local reverse proxy. Trust the actual TCP peer,
/// never forwarded headers that a direct caller can forge.
fn private_hop(peer: Option<&str>) -> Result<()> {
    if peer.and_then(|p| p.parse::<std::net::SocketAddr>().ok()).is_some_and(|p| p.ip().is_loopback()) {
        Ok(())
    } else {
        Err(Error::Unauthorized)
    }
}
