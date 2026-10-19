//! Bytes a key signed, kept exactly as signed.
//!
//! A [`Signed`] carries the signer, the body as the bytes that were signed, and the
//! signature. Nothing is re-serialized before checking, so a body reads back exactly as its
//! signer wrote it. Every kind of signed thing has its own domain tag, mixed into what is
//! signed, so that a signature over one kind never verifies as another.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use iroh::{PublicKey, SecretKey, Signature};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// The most a signed body may hold.
pub const MAX_BODY: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signed {
    pub by: PublicKey,
    /// The signed bytes, unpadded base64url.
    pub body: String,
    /// The ed25519 signature over the domain tag, a zero byte, and the body.
    pub sig: String,
}

fn message(domain: &str, body: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + 1 + body.len());
    message.extend_from_slice(domain.as_bytes());
    message.push(0);
    message.extend_from_slice(body);
    message
}

impl Signed {
    /// Sign `body` as `key`, under `domain`.
    pub fn sign<T: Serialize>(key: &SecretKey, domain: &str, body: &T) -> Result<Self> {
        let bytes = serde_json::to_vec(body).map_err(|e| Error::Internal(e.to_string()))?;
        if bytes.len() > MAX_BODY {
            return Err(Error::BadRequest(format!("a signed body holds at most {MAX_BODY} bytes")));
        }
        let sig = key.sign(&message(domain, &bytes));
        Ok(Signed { by: key.public(), body: B64.encode(&bytes), sig: B64.encode(sig.to_bytes()) })
    }

    /// The body, once the signature checks out under `domain`.
    pub fn open<T: DeserializeOwned>(&self, domain: &str) -> Result<T> {
        let bytes = self.verify(domain)?;
        serde_json::from_slice(&bytes).map_err(|e| Error::BadRequest(format!("a signed body does not read: {e}")))
    }

    /// The signed bytes, once the signature checks out under `domain`.
    pub fn verify(&self, domain: &str) -> Result<Vec<u8>> {
        if self.body.len() > MAX_BODY * 4 / 3 + 4 {
            return Err(Error::BadRequest(format!("a signed body holds at most {MAX_BODY} bytes")));
        }
        let bytes = B64.decode(&self.body).map_err(|_| Error::Unauthorized)?;
        let sig: [u8; Signature::LENGTH] =
            B64.decode(&self.sig).map_err(|_| Error::Unauthorized)?.try_into().map_err(|_| Error::Unauthorized)?;
        self.by.verify(&message(domain, &bytes), &Signature::from_bytes(&sig)).map_err(|_| Error::Unauthorized)?;
        Ok(bytes)
    }

    /// A digest naming this signed thing: the signer and the bytes, under `domain`.
    pub fn digest(&self, domain: &str) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(domain.as_bytes());
        hash.update([0]);
        hash.update(self.by.as_bytes());
        hash.update(self.body.as_bytes());
        hash.finalize().into()
    }
}

/// 32 bytes as lowercase hex.
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 64 hex characters as 32 bytes.
pub fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A 32-byte digest named by its hex, for ids.
macro_rules! digest_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub [u8; 32]);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&$crate::signed::hex(&self.0))
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), &$crate::signed::hex(&self.0)[..12])
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::error::Error;
            fn from_str(s: &str) -> $crate::error::Result<Self> {
                $crate::signed::unhex(s)
                    .map($name)
                    .ok_or_else(|| $crate::error::Error::BadRequest(format!("{} is 64 hex characters", stringify!($name))))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}
pub(crate) use digest_id;
