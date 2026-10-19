//! erisoverlaynetwork: the Eris mesh.
//!
//! A person's devices form one mesh. Its membership is the [device list](devices): signed
//! entries — this device added that one, this one removed those — that every participant keeps
//! a copy of and checks for itself. Nothing in the mesh is the authority over it; any member
//! device may add or remove another, and an entry counts only when it is signed by a device the
//! list already held.
//!
//! Apps are not members. A device approves an app with a [note]: its key, the service it takes
//! part in, and the grants it holds there, signed by the device. A service checks a caller by
//! its note and the list, and refuses anything else.
//!
//! [`mesh::Node`] is what keeps a participant's copy current. Every device and every app that
//! serves something runs one: it syncs with whoever it talks to, pushes what it learns to
//! everyone it knows, and tells the others where it is ([`directory`]). On a device, ErisAuth
//! runs the node that signs; it lets other devices in through the [ceremony] — a [ticket], a
//! [fingerprint] on both screens, one approval — and approves the apps that ask over its
//! [agent] socket.
//!
//! The ceremony, notes, the list and the directory work in a browser too; the node, the agent,
//! QR codes and the terminal prompt are `native`.

#![allow(clippy::large_enum_variant)]

#[cfg(all(feature = "native", unix))]
pub mod agent;
pub mod ceremony;
pub mod devices;
pub mod directory;
pub mod error;
pub mod fingerprint;
#[cfg(feature = "native")]
pub mod key;
#[cfg(feature = "native")]
pub mod mesh;
#[cfg(feature = "native")]
pub mod net;
pub mod note;
pub mod pairing;
pub mod permission;
#[cfg(feature = "native")]
pub mod prompt;
#[cfg(feature = "native")]
pub mod qr;
pub mod signed;
pub mod ticket;
pub mod token;
pub mod wire;

pub use devices::{DeviceId, DeviceList, EntryId, MeshId, Op};
pub use directory::{Presence, Role, Snapshot};
pub use error::{Error, Result};
pub use note::{Note, NoteId};

/// Unix seconds now.
pub fn now() -> i64 {
    n0_future::time::SystemTime::now()
        .duration_since(n0_future::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A name a person gave something: 1 to 64 printable characters, no slashes.
pub fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.contains('/') || name.chars().any(char::is_control) {
        return Err(Error::BadRequest("a name is 1 to 64 characters, without slashes".into()));
    }
    Ok(name.to_string())
}
