//! Asking the device's ErisAuth to approve an app.
//!
//! ErisAuth listens on `erisauth/agent.sock` in the session's runtime directory:
//! `$XDG_RUNTIME_DIR`, or on macOS `$TMPDIR`. An app connects and sends one
//! line: approve me, by this key, for this service, with these grants. ErisAuth puts the
//! request to the person, with the program the kernel says is asking, and answers with the
//! app's [note](crate::note) and a [snapshot](crate::directory::Snapshot) of the mesh, or a
//! denial. Closing the connection withdraws the request.
//!
//! ```text
//! app   → {"v":2,"op":"approve","service":"files","name":"ErisFiles","grants":["files:read"],"serves":true,"app":"…"}
//! agent → {"approved":{"note":{…},"snapshot":{…}}}     or  {"denied":"why"}
//! app   → {"v":2,"op":"snapshot"}
//! agent → {"snapshot":{…}}
//! ```
//!
//! Any answer may instead be `{"error":"why"}`, after which the agent hangs up.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::directory::Snapshot;
use crate::error::{Error, Result};
use crate::note::Note;

/// The protocol version an app sends.
pub const VERSION: u32 = 2;

/// The longest line either side reads.
pub const MAX_LINE: u64 = 4 * 1024 * 1024;

/// What an app asks to be approved for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub service: String,
    /// The app's name for itself. Shown to the person, trusted for nothing.
    pub name: String,
    #[serde(default)]
    pub grants: Vec<String>,
    /// Whether it serves the service to other apps.
    #[serde(default)]
    pub serves: bool,
    /// The key the app holds: the note will name it, and only its holder can use the note.
    pub app: iroh::EndpointId,
}

/// A line an app sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Approve {
        v: u32,
        #[serde(flatten)]
        ask: Ask,
    },
    Snapshot {
        v: u32,
    },
}

/// A line the agent sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Approved { note: Note, snapshot: Snapshot },
    Denied(String),
    Snapshot(Snapshot),
    Error(String),
}

/// How a request ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Approved { note: Note, snapshot: Snapshot },
    Denied(String),
}

/// Where the agent listens under a runtime directory.
pub fn socket_in(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("erisauth").join("agent.sock")
}

/// Where this user's agent listens, when the session has a runtime directory
/// ([`crate::sys::runtime_dir`]).
pub fn socket_path() -> Option<PathBuf> {
    crate::sys::runtime_dir().map(|dir| socket_in(&dir))
}

/// Connect to this user's agent; `None` when there is none.
pub async fn connect() -> Result<Option<Agent>> {
    match socket_path() {
        Some(path) => connect_to(&path).await,
        None => Ok(None),
    }
}

/// Connect to the agent at `path`; `None` when nothing listens there.
pub async fn connect_to(path: &Path) -> Result<Option<Agent>> {
    match UnixStream::connect(path).await {
        Ok(stream) => {
            let (read, write) = stream.into_split();
            Ok(Some(Agent { read: BufReader::new(read), write }))
        }
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// One conversation with the agent. Dropping it withdraws an open request.
pub struct Agent {
    read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
}

impl Agent {
    /// Ask to be approved, and wait for the person's answer. The note is checked before it is
    /// returned: it names this app's key and service and counts in the snapshot's mesh.
    pub async fn approve(&mut self, ask: &Ask) -> Result<Answer> {
        self.send(&Request::Approve { v: VERSION, ask: ask.clone() }).await?;
        match self.reply().await? {
            Reply::Approved { note, snapshot } => {
                let list = snapshot.list()?;
                note.check(&list)?;
                if note.app() != ask.app || note.body.service != ask.service {
                    return Err(Error::Unreachable("ErisAuth approved something other than what was asked".into()));
                }
                Ok(Answer::Approved { note, snapshot })
            }
            Reply::Denied(why) => Ok(Answer::Denied(why)),
            other => Err(unexpected(other)),
        }
    }

    /// The mesh as the device has it now.
    pub async fn snapshot(&mut self) -> Result<Snapshot> {
        self.send(&Request::Snapshot { v: VERSION }).await?;
        match self.reply().await? {
            Reply::Snapshot(snapshot) => Ok(snapshot),
            other => Err(unexpected(other)),
        }
    }

    async fn send(&mut self, request: &Request) -> Result<()> {
        let mut line = serde_json::to_vec(request).map_err(|e| Error::Internal(e.to_string()))?;
        line.push(b'\n');
        self.write.write_all(&line).await?;
        Ok(())
    }

    async fn reply(&mut self) -> Result<Reply> {
        let mut line = String::new();
        (&mut self.read).take(MAX_LINE).read_line(&mut line).await?;
        if line.is_empty() {
            return Err(Error::Unreachable("ErisAuth hung up without answering".into()));
        }
        serde_json::from_str(&line).map_err(|e| Error::Unreachable(format!("ErisAuth sent something unreadable: {e}")))
    }
}

fn unexpected(reply: Reply) -> Error {
    match reply {
        Reply::Error(why) => Error::Unreachable(format!("ErisAuth could not take the request: {why}")),
        other => Error::Unreachable(format!("ErisAuth answered out of turn: {other:?}")),
    }
}
