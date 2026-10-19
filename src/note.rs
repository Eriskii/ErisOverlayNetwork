//! Notes: a device approving one app.
//!
//! A note names the app's key, the service it takes part in, what it may do there, and
//! whether it serves that service to others, and the device signs it. Anyone holding the
//! mesh's [device list](crate::devices) can check one: the device must be a member, and no
//! member may have revoked the note. A note does not expire; it ends when its device leaves
//! the mesh or a device revokes it.

use iroh::SecretKey;
use serde::{Deserialize, Serialize};

use crate::devices::{DeviceId, DeviceList, MeshId};
use crate::error::{Error, Result};
use crate::signed::{Signed, digest_id};

/// Domain tag for notes.
pub const NOTE_DOMAIN: &str = "eris-mesh/note/0";

/// An app: the public key it holds, which is also its Iroh endpoint id.
pub type AppId = iroh::PublicKey;

digest_id!(
    /// One note.
    NoteId
);

/// What a device signs to approve an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteBody {
    pub mesh: MeshId,
    pub device: DeviceId,
    pub app: AppId,
    /// The app's name for itself, as the person approved it.
    pub name: String,
    /// The service it takes part in: `files`, `erisdb`.
    pub service: String,
    /// What it may do there, as permission patterns. May be empty.
    #[serde(default)]
    pub grants: Vec<String>,
    /// Whether it serves the service to other apps.
    #[serde(default)]
    pub serves: bool,
    /// What the device's operating system said the app is: an executable's path, or an
    /// Android package and the SHA-256 of its signing certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub at: i64,
}

/// A checked note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: NoteId,
    pub body: NoteBody,
    pub signed: Signed,
}

/// A service name: one permission segment, `[a-z0-9][a-z0-9._-]*`.
pub fn check_service(service: &str) -> Result<()> {
    if service.len() <= 64 && crate::permission::is_segment(service, false) {
        Ok(())
    } else {
        Err(Error::BadRequest(format!("{service:?} is not a service name: [a-z0-9][a-z0-9._-]*")))
    }
}

impl Note {
    /// Sign `body` as the device whose key `key` is.
    pub fn issue(key: &SecretKey, body: NoteBody) -> Result<Self> {
        if body.device != key.public() {
            return Err(Error::BadRequest("a note is signed by the device it names".into()));
        }
        Self::open(Signed::sign(key, NOTE_DOMAIN, &body)?)
    }

    /// Check a signed note and read it. This does not consult a device list; see [`Note::check`].
    pub fn open(signed: Signed) -> Result<Self> {
        let body: NoteBody = signed.open(NOTE_DOMAIN)?;
        if body.device != signed.by {
            return Err(Error::Unauthorized);
        }
        check_service(&body.service)?;
        crate::check_name(&body.name)?;
        if !body.grants.is_empty() {
            crate::permission::check_grants(&body.grants)?;
        }
        Ok(Note { id: NoteId(signed.digest(NOTE_DOMAIN)), body, signed })
    }

    /// Whether this note counts in `list`: same mesh, its device a member, and not revoked.
    pub fn check(&self, list: &DeviceList) -> Result<()> {
        if self.body.mesh != list.mesh() {
            return Err(Error::Unauthorized);
        }
        if !list.is_member(&self.body.device) {
            return Err(Error::Forbidden { permission: "a device in the mesh".into() });
        }
        if list.is_revoked(&self.id) {
            return Err(Error::Forbidden { permission: "an approval that has not been revoked".into() });
        }
        Ok(())
    }

    /// True when this note lets its app do `required` in `service`.
    pub fn grants(&self, service: &str, required: &str) -> bool {
        self.body.service == service && crate::permission::granted(&self.body.grants, required)
    }

    pub fn app(&self) -> AppId {
        self.body.app
    }

    pub fn device(&self) -> DeviceId {
        self.body.device
    }
}

impl Serialize for Note {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.signed.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Note {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Note::open(Signed::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
