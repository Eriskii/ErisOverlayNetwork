//! Signed tokens: `<prefix>.<b64url(json)>.<b64url(hmac_sha256(secret, json))>`.
//!
//! The prefix names the issuer and version, so one app's tokens never open
//! as another's. Anything that fails to open — wrong prefix, bad encoding,
//! wrong signature, unexpected payload — is [`Error::Unauthorized`], with no
//! detail for a forger to learn from.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hmac::{Hmac, Mac};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::Sha256;

use crate::error::{Error, Result};

fn mac(secret: &[u8], payload: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload);
    mac
}

/// Sign `payload` under `prefix`.
pub fn seal<T: Serialize>(prefix: &str, secret: &[u8], payload: &T) -> Result<String> {
    let json = serde_json::to_vec(payload).map_err(|e| Error::Internal(e.to_string()))?;
    let sig = mac(secret, &json).finalize().into_bytes();
    Ok(format!("{prefix}.{}.{}", B64.encode(&json), B64.encode(sig)))
}

/// Check a token's prefix and signature and return its payload.
pub fn open<T: DeserializeOwned>(prefix: &str, secret: &[u8], token: &str) -> Result<T> {
    let mut parts = token.split('.');
    let (Some(p), Some(payload), Some(sig), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(Error::Unauthorized);
    };
    if p != prefix {
        return Err(Error::Unauthorized);
    }
    let payload = B64.decode(payload).map_err(|_| Error::Unauthorized)?;
    let sig = B64.decode(sig).map_err(|_| Error::Unauthorized)?;
    mac(secret, &payload).verify_slice(&sig).map_err(|_| Error::Unauthorized)?;
    serde_json::from_slice(&payload).map_err(|_| Error::Unauthorized)
}
