//! Short codes: a ticket a person can type, `K7QX-M2PA`.
//!
//! A [ticket](crate::ticket) is long: fine for a camera, no fun to type. A code stands in for
//! one: eight characters of Crockford's base32, forty random bits. The device showing the ticket
//! also runs a small endpoint whose key the code derives, and hands the ticket to whoever dials
//! it. Whoever types the code derives the same endpoint id and dials it, found the way Iroh finds
//! any endpoint: through n0's DNS, by mDNS on the local network, or through the one relay a
//! relay-only mesh has. The ticket it gets back lets in only what a person approves once both
//! screens show the same fingerprint, as any ticket does.

use iroh::{EndpointAddr, EndpointId, RelayUrl, SecretKey, TransportAddr};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::ticket::Ticket;

/// The ALPN a code's endpoint answers on.
pub const ALPN: &[u8] = b"eris-code/0";

/// Crockford's base32: no I, L, O or U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The tag in every code's key.
const DOMAIN: &str = "eris-code/0";

/// How many characters a code has.
pub const LEN: usize = 8;

/// The one request a code's endpoint takes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Ticket,
}

/// A new code, as a person reads it: `K7QX-M2PA`.
pub fn generate() -> String {
    let symbols: String = (0..LEN).map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char).collect();
    format!("{}-{}", &symbols[..4], &symbols[4..])
}

/// A code as typed, read the way it was meant: any case, with or without the dash or spaces,
/// an O for a zero and an I or L for a one. The answer is the code as [`generate`] writes it.
pub fn normalize(typed: &str) -> Result<String> {
    let symbols: String = typed
        .chars()
        .filter(|c| !matches!(c, '-' | ' ' | '_'))
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    if symbols.len() != LEN || !symbols.bytes().all(|b| ALPHABET.contains(&b)) {
        return Err(Error::BadRequest(format!("{typed:?} is not a code: eight letters and digits, like K7QX-M2PA")));
    }
    Ok(format!("{}-{}", &symbols[..4], &symbols[4..]))
}

/// The key of the endpoint `code` names.
pub fn key(code: &str) -> Result<SecretKey> {
    let code = normalize(code)?;
    let digest: [u8; 32] = Sha256::digest(format!("{DOMAIN}\n{code}")).into();
    Ok(SecretKey::from_bytes(&digest))
}

/// The endpoint `code` names.
pub fn id(code: &str) -> Result<EndpointId> {
    key(code).map(|key| key.public())
}

/// Dial the endpoint `code` names and take the ticket it hands over. `relay` is the one relay to
/// look for it through, on a network that has nothing else; without one, the endpoint's own
/// address lookup finds it.
pub async fn resolve(endpoint: &iroh::Endpoint, relay: Option<RelayUrl>, code: &str) -> Result<Ticket> {
    let id = id(code)?;
    let addr = EndpointAddr::from_parts(id, relay.map(TransportAddr::Relay));
    let conn = n0_future::time::timeout(crate::ceremony::CONNECT_TIMEOUT, endpoint.connect(addr, ALPN))
        .await
        .map_err(|_| Error::Unreachable("nothing answered for that code; is it still shown?".into()))?
        .map_err(|e| Error::Unreachable(format!("nothing answered for that code ({e}); is it still shown?")))?;
    let ticket: Result<String> = crate::wire::ask(&conn, &Request::Ticket).await;
    conn.close(0u32.into(), b"done");
    Ticket::parse(&ticket?)
}

#[cfg(feature = "native")]
pub use shown::Shown;

#[cfg(feature = "native")]
mod shown {
    use iroh::endpoint::Connection;
    use iroh::protocol::{AcceptError, ProtocolHandler, Router};

    use super::{ALPN, Request};
    use crate::error::Result;
    use crate::net::Network;

    /// A code's endpoint, handing over its ticket until it is closed.
    #[derive(Debug)]
    pub struct Shown {
        router: Router,
    }

    #[derive(Debug, Clone)]
    struct Hand(String);

    impl ProtocolHandler for Hand {
        async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
            while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                let answer = crate::wire::request::<Request>(&mut recv).await.map(|Request::Ticket| self.0.clone());
                let _ = crate::wire::reply(&mut send, answer).await;
            }
            Ok(())
        }
    }

    impl Shown {
        /// Run the endpoint `code` names on `network`, handing over `ticket`.
        pub async fn start(code: &str, ticket: String, network: &Network) -> Result<Self> {
            let endpoint = crate::net::endpoint(super::key(code)?, network).await?;
            let router = Router::builder(endpoint).accept(ALPN, Hand(ticket)).spawn();
            Ok(Shown { router })
        }

        /// The code's endpoint, to say where it is.
        pub fn endpoint(&self) -> &iroh::Endpoint {
            self.router.endpoint()
        }

        /// Stop answering for the code.
        pub async fn close(&self) {
            let _ = self.router.shutdown().await;
        }
    }
}
