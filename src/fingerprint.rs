//! The comparison code both screens show during the [ceremony](crate::ceremony).

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Six bytes of the SHA-256 of a JSON transcript, as `12AB-34CD-56EF`.
/// Enough for two people to compare at a glance; far too many for a racing
/// redeemer to hit by chance.
pub fn fingerprint<T: Serialize + ?Sized>(transcript: &T) -> String {
    let digest = Sha256::digest(serde_json::to_vec(transcript).expect("serializable transcript"));
    format!("{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}", digest[0], digest[1], digest[2], digest[3], digest[4], digest[5])
}
