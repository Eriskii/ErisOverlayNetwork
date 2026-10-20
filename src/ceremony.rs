//! Letting a device or an app in from another screen: a ticket, a fingerprint, one approval.
//!
//! A member device shows a [ticket](crate::ticket). The newcomer dials the device it names
//! over [`ALPN`] and redeems the code, saying what it asks to be: a device joining the mesh,
//! or an app that wants a [note](crate::note) — a browser app, which has no ErisAuth beside
//! it to ask. Both screens show the [fingerprint](crate::pairing::fingerprint) of the request,
//! computed by each side for itself; a person compares them and approves. The newcomer
//! collects the answer, which only the key that redeemed can do: for a device, the list with
//! it added; for an app, its note. Both come with a [`Snapshot`] of the mesh.

use serde::{Deserialize, Serialize};

use crate::directory::Snapshot;
use crate::error::{Error, Result};
use crate::note::Note;
use crate::pairing::code_session;
use crate::ticket::Ticket;

/// The ceremony's ALPN.
pub const ALPN: &[u8] = b"eris-pair/0";

/// How long dialing the device may take.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// How often a newcomer asks for its answer.
pub const POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// What a newcomer asks to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ask {
    /// A device joining the mesh, under this name.
    Device { name: String },
    /// An app that wants a note.
    App {
        name: String,
        service: String,
        #[serde(default)]
        grants: Vec<String>,
        #[serde(default)]
        serves: bool,
    },
}

impl Ask {
    pub fn check(&self) -> Result<()> {
        match self {
            Ask::Device { name } => crate::check_name(name).map(drop),
            Ask::App { name, service, grants, .. } => {
                crate::check_name(name)?;
                crate::note::check_service(service)?;
                if !grants.is_empty() {
                    crate::permission::check_grants(grants)?;
                }
                Ok(())
            }
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Ask::Device { name } | Ask::App { name, .. } => name,
        }
    }
}

/// What a newcomer collects once a person approves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Approval {
    /// The mesh, with the device added.
    Device { snapshot: Snapshot },
    /// The app's note, and the mesh it counts in.
    App { note: Note, snapshot: Snapshot },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Redeem {
        code: String,
        ask: Ask,
    },
    Collect {
        code: String,
    },
    /// The redeemer takes its request back.
    Withdraw {
        code: String,
    },
}

/// A device's answer to a redemption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Redeemed {
    pub fingerprint: String,
    /// The device's name.
    pub name: String,
}

/// A device's answer to collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Reply {
    Waiting,
    Approved { approval: Approval },
    Denied,
}

/// A redemption under way, from the newcomer's side.
pub struct Joining {
    endpoint: iroh::Endpoint,
    ticket: Ticket,
    ask: Ask,
    conn: std::sync::Mutex<Option<iroh::endpoint::Connection>>,
    /// The code to compare with the device's screen, computed on this one.
    pub fingerprint: String,
    /// The device's name for itself.
    pub host: String,
}

/// Redeem `ticket` from `endpoint`, asking for `ask`.
pub async fn redeem(endpoint: &iroh::Endpoint, ticket: &Ticket, ask: Ask) -> Result<Joining> {
    ask.check()?;
    if ticket.eid == endpoint.id() {
        return Err(Error::BadRequest("this is this device's own ticket".into()));
    }
    let session =
        code_session(&ticket.token).ok_or_else(|| Error::BadRequest("the ticket's code is damaged".into()))?;
    let fingerprint = crate::pairing::fingerprint(&session, &endpoint.id(), &ask);
    let conn = dial(endpoint, ticket).await?;
    let redeemed: Redeemed =
        crate::wire::ask(&conn, &Request::Redeem { code: ticket.token.clone(), ask: ask.clone() }).await?;
    if redeemed.fingerprint != fingerprint {
        return Err(Error::Unreachable("the device saw a different request than this one sent".into()));
    }
    Ok(Joining {
        endpoint: endpoint.clone(),
        ticket: ticket.clone(),
        ask,
        conn: std::sync::Mutex::new(Some(conn)),
        fingerprint,
        host: redeemed.name,
    })
}

/// Take up again a redemption this endpoint made earlier — from a page that has since gone
/// away — to collect its answer or withdraw it. Only the key that redeemed can collect, so
/// this works for nobody else; `host` is the device's name, as the redemption gave it.
pub fn resume(endpoint: &iroh::Endpoint, ticket: &Ticket, ask: Ask, host: String) -> Result<Joining> {
    ask.check()?;
    let session =
        code_session(&ticket.token).ok_or_else(|| Error::BadRequest("the ticket's code is damaged".into()))?;
    let fingerprint = crate::pairing::fingerprint(&session, &endpoint.id(), &ask);
    Ok(Joining {
        endpoint: endpoint.clone(),
        ticket: ticket.clone(),
        ask,
        conn: std::sync::Mutex::new(None),
        fingerprint,
        host,
    })
}

async fn dial(endpoint: &iroh::Endpoint, ticket: &Ticket) -> Result<iroh::endpoint::Connection> {
    n0_future::time::timeout(CONNECT_TIMEOUT, endpoint.connect(ticket.endpoint_addr(), ALPN))
        .await
        .map_err(|_| Error::Unreachable("the device did not answer in time".into()))?
        .map_err(|e| Error::Unreachable(format!("could not reach the device: {e}")))
}

impl Joining {
    /// Take the request back: the device forgets it.
    pub async fn withdraw(&self) -> Result<()> {
        let conn = self.connection().await?;
        let _: serde_json::Value =
            crate::wire::ask(&conn, &Request::Withdraw { code: self.ticket.token.clone() }).await?;
        Ok(())
    }

    async fn connection(&self) -> Result<iroh::endpoint::Connection> {
        if let Some(conn) = self.conn.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(conn);
        }
        let conn = dial(&self.endpoint, &self.ticket).await?;
        *self.conn.lock().unwrap_or_else(|e| e.into_inner()) = Some(conn.clone());
        Ok(conn)
    }

    /// Ask once for the answer. An approval is checked before it is returned: the list must
    /// hold the device that approved, and the answer must be about this newcomer.
    pub async fn collect(&self) -> Result<Reply> {
        let conn = self.connection().await?;
        let reply: Reply = match crate::wire::ask(&conn, &Request::Collect { code: self.ticket.token.clone() }).await {
            Ok(reply) => reply,
            Err(e @ Error::Unreachable(_)) => {
                self.conn.lock().unwrap_or_else(|e| e.into_inner()).take();
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        if let Reply::Approved { approval } = &reply {
            self.check(approval)?;
        }
        Ok(reply)
    }

    fn check(&self, approval: &Approval) -> Result<()> {
        let me = self.endpoint.id();
        let bad = |why: &str| Err(Error::Unreachable(format!("the device's answer {why}")));
        match (approval, &self.ask) {
            (Approval::Device { snapshot }, Ask::Device { .. }) => {
                let list = snapshot.list()?;
                if !list.is_member(&self.ticket.eid) {
                    return bad("comes from outside its own mesh");
                }
                if !list.is_member(&me) {
                    return bad("does not add this device");
                }
                Ok(())
            }
            (Approval::App { note, snapshot }, Ask::App { service, .. }) => {
                let list = snapshot.list()?;
                note.check(&list)?;
                if note.app() != me || note.device() != self.ticket.eid || note.body.service != *service {
                    return bad("approves something other than what this app asked for");
                }
                Ok(())
            }
            _ => bad("is for another kind of request"),
        }
    }

    /// Ask until there is an answer or `patience` runs out: the approval, or `None` when the
    /// person denied the request.
    pub async fn wait(&self, patience: std::time::Duration) -> Result<Option<Approval>> {
        let deadline = n0_future::time::Instant::now() + patience;
        loop {
            match self.collect().await {
                Ok(Reply::Approved { approval }) => return Ok(Some(approval)),
                Ok(Reply::Denied) => return Ok(None),
                Ok(Reply::Waiting) | Err(Error::Unreachable(_)) => {}
                Err(e) => return Err(e),
            }
            if n0_future::time::Instant::now() >= deadline {
                return Err(Error::Expired);
            }
            n0_future::time::sleep(POLL).await;
        }
    }
}

#[cfg(feature = "native")]
pub use service::PairService;

#[cfg(feature = "native")]
mod service {
    use std::sync::Arc;

    use iroh::endpoint::Connection;
    use iroh::protocol::{AcceptError, ProtocolHandler};

    use super::{Redeemed, Reply, Request};
    use crate::pairing::{Collected, Pairings};

    /// A device's side of the ceremony: answers redemptions and collections from its
    /// [`Pairings`]. Its UI opens sessions and approves or denies them there.
    #[derive(Clone)]
    pub struct PairService {
        pub pairings: Arc<Pairings>,
        name: Arc<dyn Fn() -> String + Send + Sync>,
    }

    impl std::fmt::Debug for PairService {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("PairService")
        }
    }

    impl PairService {
        /// `name` tells redeemers what this device is called now.
        pub fn new(pairings: Arc<Pairings>, name: impl Fn() -> String + Send + Sync + 'static) -> Self {
            PairService { pairings, name: Arc::new(name) }
        }
    }

    impl ProtocolHandler for PairService {
        async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
            let remote = conn.remote_id();
            while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                let this = self.clone();
                n0_future::task::spawn(async move {
                    let answer = match crate::wire::request::<Request>(&mut recv).await {
                        Ok(Request::Redeem { code, ask }) => match this.pairings.redeem(&code, ask, remote) {
                            Ok(session) => {
                                tracing::info!(session = %session.id, "redeemed");
                                serde_json::to_value(Redeemed {
                                    fingerprint: session.fingerprint.unwrap_or_default(),
                                    name: (this.name)(),
                                })
                                .map_err(|e| crate::Error::Internal(e.to_string()))
                            }
                            Err(e) => Err(e),
                        },
                        Ok(Request::Withdraw { code }) => {
                            this.pairings.withdraw(&code, &remote).map(|()| serde_json::Value::Null)
                        }
                        Ok(Request::Collect { code }) => this.pairings.collect(&code, &remote).and_then(|c| {
                            serde_json::to_value(match c {
                                Collected::Waiting => Reply::Waiting,
                                Collected::Denied => Reply::Denied,
                                Collected::Approved(approval) => Reply::Approved { approval },
                            })
                            .map_err(|e| crate::Error::Internal(e.to_string()))
                        }),
                        Err(e) => Err(e),
                    };
                    let _ = crate::wire::reply(&mut send, answer).await;
                });
            }
            Ok(())
        }
    }
}
