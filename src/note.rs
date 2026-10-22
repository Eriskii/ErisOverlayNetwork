//! Notes: a device approving one app.
//!
//! A note names the app's key, the [capabilities](crate::capability) it uses with the grants it
//! holds in each, and those it serves, and the device signs it. Anyone holding the mesh's
//! [device list](crate::devices) can check one: the device must be a member, and no member may
//! have revoked the note. A note does not expire; it ends when its device leaves the mesh or a
//! device revokes it.

use iroh::SecretKey;
use serde::{Deserialize, Serialize};

use crate::capability::{Serves, Uses};
use crate::devices::{DeviceId, DeviceList, MeshId};
use crate::error::{Error, Result};
use crate::signed::{Signed, digest_id};

/// Domain tag for notes.
pub const NOTE_DOMAIN: &str = "eris-mesh/note/1";

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
    /// What it uses, and the grants it holds in each.
    #[serde(default, skip_serializing_if = "Uses::is_empty")]
    pub uses: Uses,
    /// What it serves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub serves: Vec<Serves>,
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
        crate::check_name(&body.name)?;
        crate::capability::check(&body.uses, &body.serves)?;
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

    /// The grants this note's app holds in `capability`, when it uses it.
    pub fn uses(&self, capability: &str) -> Option<&[String]> {
        self.body.uses.get(capability).map(Vec::as_slice)
    }

    /// True when this note lets its app do `required` in `capability`.
    pub fn grants(&self, capability: &str, required: &str) -> bool {
        self.uses(capability).is_some_and(|grants| crate::permission::granted(grants, required))
    }

    /// How this note's app serves `capability`, when it does.
    pub fn serving(&self, capability: &str) -> Option<&Serves> {
        self.body.serves.iter().find(|s| s.capability.as_str() == capability)
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
