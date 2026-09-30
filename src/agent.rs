//! Asking the device's ErisAuth to approve an app, instead of showing a
//! person a ticket.
//!
//! ErisAuth listens on `$XDG_RUNTIME_DIR/erisauth/agent.sock`. An app
//! connects and sends one line naming the service, itself, the grants it
//! wants and the [identity](Identity) it will redeem with. ErisAuth answers
//! with a ticket; the app redeems it at the service exactly as it would a
//! scanned one, with that identity. ErisAuth puts the request to the person
//! and answers `approved` or `denied`, and the app collects its credential
//! from the service. Closing the connection withdraws the request.
//!
//! ```text
//! app   → {"v":1,"op":"pair","service":"erisdb","client":"Notes","requested":["notes:read"],"identity":{…}}
//! agent → {"ticket":"erisdb://pair/…"}
//! agent → {"approved":["notes:read"]}     or  {"denied":"why"}
//! ```
//!
//! Any line may instead be `{"error":"why"}`, after which the agent hangs
//! up. With no agent listening, [`connect`] returns `None` and the app pairs
//! by ticket as it always can.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::error::{Error, Result};
use crate::identity::Identity;

/// The protocol version an app sends.
pub const VERSION: u32 = 1;

/// The longest line either side reads.
pub const MAX_LINE: u64 = 64 * 1024;

/// What an app asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    /// The service to pair with, as its tickets name it: `erisdb`.
    pub service: String,
    /// The app's name for itself. Shown to the person, trusted for nothing.
    pub client: String,
    pub requested: Vec<String>,
    /// The identity the app will redeem the ticket with.
    pub identity: Identity,
}

/// The first line an app sends: an [`Ask`] with the version and operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub v: u32,
    pub op: String,
    #[serde(flatten)]
    pub ask: Ask,
}

/// A line the agent sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// Redeem this ticket with the identity you named.
    Ticket(String),
    /// The person approved these grants; collect the credential from the service.
    Approved(Vec<String>),
    /// Nothing was approved, and why.
    Denied(String),
    /// The agent could not take the request.
    Error(String),
}

/// How a request ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Approved(Vec<String>),
    Denied(String),
}

/// Where the agent listens under a runtime directory.
pub fn socket_in(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("erisauth").join("agent.sock")
}

/// Where this user's agent listens, when the session has a runtime directory.
pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()).map(|dir| socket_in(Path::new(&dir)))
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

/// One request to the agent. Dropping it withdraws the request.
pub struct Agent {
    read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
}

impl Agent {
    /// Send `ask`; returns the ticket to redeem.
    pub async fn ask(&mut self, ask: &Ask) -> Result<String> {
        let request = Request { v: VERSION, op: "pair".into(), ask: ask.clone() };
        let mut line = serde_json::to_vec(&request).map_err(|e| Error::Internal(e.to_string()))?;
        line.push(b'\n');
        self.write.write_all(&line).await?;
        match self.reply().await? {
            Reply::Ticket(ticket) => Ok(ticket),
            other => Err(unexpected(other)),
        }
    }

    /// Wait for the person's answer.
    pub async fn answer(&mut self) -> Result<Answer> {
        match self.reply().await? {
            Reply::Approved(granted) => Ok(Answer::Approved(granted)),
            Reply::Denied(why) => Ok(Answer::Denied(why)),
            other => Err(unexpected(other)),
        }
    }

    async fn reply(&mut self) -> Result<Reply> {
        let mut line = String::new();
        (&mut self.read).take(MAX_LINE).read_line(&mut line).await?;
        if line.is_empty() {
            return Err(Error::Internal("ErisAuth hung up without answering".into()));
        }
        serde_json::from_str(&line).map_err(|e| Error::Internal(format!("ErisAuth sent something unreadable: {e}")))
    }
}

fn unexpected(reply: Reply) -> Error {
    match reply {
        Reply::Error(why) => Error::Internal(format!("ErisAuth could not take the request: {why}")),
        other => Error::Internal(format!("ErisAuth answered out of turn: {other:?}")),
    }
}
