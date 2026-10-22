//! Asking the device's ErisOverlayNetwork to approve an app.
//!
//! ErisOverlayNetwork listens on `eon/agent.sock` in the session's runtime directory:
//! `$XDG_RUNTIME_DIR`, or on macOS `$TMPDIR`. An app connects and sends one line: approve me, by
//! this key, to use these [capabilities](crate::capability) with these grants and to serve
//! those. ErisOverlayNetwork puts the request to the person, with the program the kernel says is
//! asking, and answers with the app's [note](crate::note) and a
//! [snapshot](crate::directory::Snapshot) of the mesh, or a denial. Closing the connection
//! withdraws the request.
//!
//! ```text
//! app   → {"v":3,"op":"approve","name":"ErisFiles","uses":{"files/1":["files:read"]},"serves":[{"capability":"files/1"}],"app":"…"}
//! agent → {"approved":{"note":{…},"snapshot":{…}}}     or  {"denied":"why"}
//! app   → {"v":3,"op":"snapshot"}
//! agent → {"snapshot":{…}}
//! ```
//!
//! Any answer may instead be `{"error":"why"}`, after which the agent hangs up.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::capability::{Serves, Uses};
use crate::directory::Snapshot;
use crate::error::{Error, Result};
use crate::note::Note;

/// The protocol version an app sends.
pub const VERSION: u32 = 3;

/// The longest line either side reads.
pub const MAX_LINE: u64 = 4 * 1024 * 1024;

/// What an app asks to be approved for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    /// The app's name for itself. Shown to the person, trusted for nothing.
    pub name: String,
    /// What it uses, with the grants it asks for in each.
    #[serde(default)]
    pub uses: Uses,
    /// What it serves.
    #[serde(default)]
    pub serves: Vec<Serves>,
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
    runtime_dir.join("eon").join("agent.sock")
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
    /// returned: it names this app's key, approves no more than was asked, and counts in the
    /// snapshot's mesh. The person may have approved fewer grants than were asked for.
    pub async fn approve(&mut self, ask: &Ask) -> Result<Answer> {
        self.send(&Request::Approve { v: VERSION, ask: ask.clone() }).await?;
        match self.reply().await? {
            Reply::Approved { note, snapshot } => {
                let list = snapshot.list()?;
                note.check(&list)?;
                let within = crate::capability::within((&ask.uses, &ask.serves), (&note.body.uses, &note.body.serves));
                if note.app() != ask.app || !within {
                    return Err(Error::Unreachable(
                        "ErisOverlayNetwork approved something other than what was asked".into(),
                    ));
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
            return Err(Error::Unreachable("ErisOverlayNetwork hung up without answering".into()));
        }
        serde_json::from_str(&line)
            .map_err(|e| Error::Unreachable(format!("ErisOverlayNetwork sent something unreadable: {e}")))
    }
}

fn unexpected(reply: Reply) -> Error {
    match reply {
        Reply::Error(why) => Error::Unreachable(format!("ErisOverlayNetwork could not take the request: {why}")),
        other => Error::Unreachable(format!("ErisOverlayNetwork answered out of turn: {other:?}")),
    }
}
