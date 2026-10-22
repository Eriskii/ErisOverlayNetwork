//! Letting an app in where its server asks for a passphrase.
//!
//! An app that serves a capability with [`passphrase`](crate::capability::Serves::passphrase)
//! keeps a passphrase of its own, and lets in only the apps a device brings it along with that
//! passphrase. A device approves the app, the person types the passphrase on the device, and the
//! device calls the server over [`ALPN`], as itself: here is the app's note, here is the
//! passphrase. The server checks the passphrase itself and counts wrong guesses per device, so a
//! stolen phone guessing away locks out only itself. A device also changes the passphrase by
//! knowing it.
//!
//! ```text
//! device → {"op":"enroll","note":{…},"passphrase":"…"}     server → {"answer":"done"}
//! device → {"op":"change","current":"…","new":"…"}         server → {"answer":"wrong","retry_after":60}
//! ```
//!
//! [`EnrollService`] is the server's side: it lets in only member devices, bringing notes of
//! their own that count and use what this server serves, while this server's own note counts. A
//! [`Gate`] does the rest: the passphrase, and what letting the app in means to the server.

use std::sync::Arc;

use iroh::EndpointId;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use n0_future::boxed::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::devices::DeviceId;
use crate::error::{Error, Result};
use crate::mesh::{Local, Node};
use crate::note::Note;

/// The ALPN servers that take a passphrase answer on.
pub const ALPN: &[u8] = b"eris-enroll/0";

/// What a device asks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Let in the app this device's `note` approves; the person typed `passphrase`.
    Enroll { note: Note, passphrase: String },
    /// Make `new` the passphrase; the person typed the `current` one.
    Change { current: String, new: String },
}

/// What a server answers, when it takes the request at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Outcome {
    Done,
    /// Not the passphrase. When this guess locked the device out, how many seconds it waits.
    Wrong {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after: Option<u64>,
    },
    /// The device guessed wrong too often, and waits this many seconds more.
    Locked {
        retry_after: u64,
    },
    /// The server has no passphrase yet; it is set where the server runs.
    NoPassphrase,
}

/// What a server does with a device's request, once [`EnrollService`] has checked who is asking
/// and what it brings.
pub trait Gate: Send + Sync + 'static {
    /// Let in the app `note` approves, if `passphrase` is right; `device` is asking.
    fn enroll(&self, device: DeviceId, note: Note, passphrase: String) -> BoxFuture<Result<Outcome>>;
    /// Make `new` the passphrase, if `current` is right; `device` is asking.
    fn change(&self, device: DeviceId, current: String, new: String) -> BoxFuture<Result<Outcome>>;
}

/// A server's side of [`ALPN`], for its router.
#[derive(Clone)]
pub struct EnrollService {
    node: Node,
    gate: Arc<dyn Gate>,
}

impl std::fmt::Debug for EnrollService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EnrollService({:?})", self.node)
    }
}

impl EnrollService {
    /// `node` runs under this server's own note.
    pub fn new(node: Node, gate: Arc<dyn Gate>) -> Self {
        EnrollService { node, gate }
    }

    /// Serve one connection: what a host without an iroh router calls for each it accepts.
    pub async fn serve(&self, conn: Connection) {
        let remote = conn.remote_id();
        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
            let this = self.clone();
            n0_future::task::spawn(async move {
                let answer = match crate::wire::request::<Request>(&mut recv).await {
                    Ok(request) => this.handle(remote, request).await,
                    Err(e) => Err(e),
                };
                let _ = crate::wire::reply(&mut send, answer).await;
            });
        }
    }

    async fn handle(&self, remote: EndpointId, request: Request) -> Result<Outcome> {
        let Local::App(mine) = self.node.local() else {
            return Err(Error::Internal("a server runs under a note of its own".into()));
        };
        let gated: Vec<&str> =
            mine.body.serves.iter().filter(|s| s.passphrase).map(|s| s.capability.as_str()).collect();
        if gated.is_empty() {
            return Err(Error::Forbidden { permission: "a server that takes a passphrase".into() });
        }
        if !self.node.counts(mine) {
            return Err(Error::Forbidden {
                permission: "the server the mesh approved, which this one no longer is".into(),
            });
        }
        if !self.node.with_list(|list| list.is_some_and(|l| l.is_member(&remote))) {
            return Err(Error::Forbidden { permission: "a device in the mesh, calling as itself".into() });
        }
        match request {
            Request::Enroll { note, passphrase } => {
                if note.device() != remote {
                    return Err(Error::Forbidden { permission: "a device bringing its own note".into() });
                }
                self.node.with_list(|list| match list {
                    Some(list) => note.check(list),
                    None => Err(Error::Conflict("this server is in no mesh".into())),
                })?;
                if !gated.iter().any(|capability| note.uses(capability).is_some()) {
                    return Err(Error::BadRequest(format!("that note approves using none of {}", gated.join(", "))));
                }
                self.gate.enroll(remote, note, passphrase).await
            }
            Request::Change { current, new } => self.gate.change(remote, current, new).await,
        }
    }
}

impl ProtocolHandler for EnrollService {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
        self.serve(conn).await;
        Ok(())
    }
}

/// Bring `server` the app `note` approves, with the `passphrase` the person typed, as this
/// device.
pub async fn enroll(node: &Node, server: EndpointId, note: &Note, passphrase: &str) -> Result<Outcome> {
    ask(node, server, &Request::Enroll { note: note.clone(), passphrase: passphrase.to_string() }).await
}

/// Change `server`'s passphrase from `current` to `new`, as this device.
pub async fn change(node: &Node, server: EndpointId, current: &str, new: &str) -> Result<Outcome> {
    ask(node, server, &Request::Change { current: current.to_string(), new: new.to_string() }).await
}

async fn ask(node: &Node, server: EndpointId, request: &Request) -> Result<Outcome> {
    let conn = node.connect(server, ALPN).await?;
    let answer = crate::wire::ask(&conn, request).await;
    conn.close(0u32.into(), b"done");
    answer
}
