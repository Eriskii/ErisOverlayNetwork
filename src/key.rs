//! Iroh identities: derived from a deployment secret, or kept in a file.

use std::io::Write;
use std::path::Path;

use hmac::{Hmac, Mac};

use crate::error::{Error, Result};

/// An ed25519 key derived deterministically from `secret` (HMAC-SHA256 as a
/// KDF), domain-separated by `tag`. Same secret and tag, same endpoint id
/// across restarts, with no key file to lose. Changing the tag changes every
/// deployment's identity.
pub fn derive(secret: &[u8], tag: &[u8]) -> iroh::SecretKey {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(tag);
    let bytes: [u8; 32] = mac.finalize().into_bytes().into();
    iroh::SecretKey::from_bytes(&bytes)
}

/// The key stored at `path`, created (owner-only, atomically) when there is
/// none. A file that is there but is not a key is an error: silently
/// replacing it would change this device's identity under every peer.
pub fn load_or_create(path: &Path) -> Result<iroh::SecretKey> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                Error::BadRequest(format!(
                    "{} is not a 32-byte key; move it aside to make a new identity",
                    path.display()
                ))
            })?;
            Ok(iroh::SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = iroh::SecretKey::generate();
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("tmp");
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options.open(&tmp)?;
            file.write_all(&key.to_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)?;
            Ok(key)
        }
        Err(e) => Err(e.into()),
    }
}
