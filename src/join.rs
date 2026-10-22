//! Being added from another screen: the device that is not in yet shows a ticket, and a device
//! already in scans it.
//!
//! A device in no mesh shows a [join ticket](crate::ticket::Kind::Join): its endpoint id, its
//! name, and a one-time code. A member device that reads it, with a camera most likely, asks its
//! person "add laptop?", and on a yes dials the newcomer over [`ALPN`] and offers it the mesh:
//! the code, and the mesh as it stands. The newcomer's screen says which device is offering and
//! which devices it would be joining, and its person accepts there. The member then adds the
//! newcomer and hands over the mesh with it added.
//!
//! The member took the newcomer's key straight off the newcomer's screen, so there is no
//! fingerprint to compare. The newcomer's person sees the mesh's devices by name before
//! accepting, and the code keeps out anyone who never saw the screen.
//!
//! ```text
//! member → {"op":"offer","code","snapshot"}     newcomer → null
//! member → {"op":"ask","code"}                  newcomer → {"answer":"waiting" | "accepted" | "declined"}
//! member → {"op":"welcome","code","snapshot"}   newcomer → null, once it holds the mesh
//! member → {"op":"withdraw","code"}             newcomer → null
//! ```

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointId};
use n0_future::boxed::BoxFuture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::devices::{DeviceList, MeshId};
use crate::directory::Snapshot;
use crate::error::{Error, Result};
use crate::now;
use crate::ticket::{Kind, Ticket};

/// The ALPN a newcomer showing a join ticket answers on.
pub const ALPN: &[u8] = b"eris-join/0";

/// How often a member asks whether its offer was accepted.
pub const POLL: Duration = Duration::from_millis(500);

/// What a member says to a newcomer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Join the mesh `snapshot` holds.
    Offer { code: String, snapshot: Snapshot },
    /// Has the person accepted?
    Ask { code: String },
    /// The mesh, with the newcomer added.
    Welcome { code: String, snapshot: Snapshot },
    /// The member takes its offer back.
    Withdraw { code: String },
}

/// Where an offer stands, as the member hears it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Reply {
    Waiting,
    Accepted,
    Declined,
}

/// Where the newcomer's ticket stands, as its screens show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// Showing the ticket; nobody has offered yet.
    Showing,
    /// A device offers to add this one: its name, and the names of every device in the mesh.
    Offered { by: String, devices: Vec<String> },
    /// The person accepted; the device is adding this one.
    Accepted { by: String, devices: Vec<String> },
    /// This device is in the mesh.
    Joined { by: String },
}

struct Slot {
    digest: [u8; 32],
    expires: i64,
    status: Status,
    /// The member whose offer stands, and its mesh.
    by: Option<(EndpointId, MeshId)>,
    /// Members whose offers the person turned down.
    declined: HashSet<EndpointId>,
}

/// A newcomer's side: the one ticket it shows and the offer made on it.
pub struct Offers {
    me: EndpointId,
    slot: Mutex<Option<Slot>>,
    changes: tokio::sync::watch::Sender<u64>,
}

impl Offers {
    /// The offers made to the device whose endpoint id is `me`.
    pub fn new(me: EndpointId) -> Self {
        Offers { me, slot: Mutex::new(None), changes: tokio::sync::watch::channel(0).0 }
    }

    /// A receiver that changes whenever the offer does.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn changed(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    fn live(&self) -> std::sync::MutexGuard<'_, Option<Slot>> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|s| s.expires <= now() && !matches!(s.status, Status::Joined { .. })) {
            *slot = None;
        }
        slot
    }

    /// Show a new code for `ttl_secs`, in place of any shown before; returns it, to put in a
    /// [join ticket](Ticket::join).
    pub fn open(&self, ttl_secs: i64) -> String {
        let secret: [u8; 32] = rand::random();
        *self.live() = Some(Slot {
            digest: Sha256::digest(secret).into(),
            expires: now().saturating_add(ttl_secs),
            status: Status::Showing,
            by: None,
            declined: HashSet::new(),
        });
        self.changed();
        B64.encode(secret)
    }

    /// Stop showing the ticket.
    pub fn close(&self) {
        *self.live() = None;
        self.changed();
    }

    /// Where the ticket stands and when it ends, while there is one.
    pub fn status(&self) -> Option<(Status, i64)> {
        self.live().as_ref().map(|s| (s.status.clone(), s.expires))
    }

    /// The person accepts the offer made.
    pub fn accept(&self) -> Result<()> {
        let mut live = self.live();
        let slot = live.as_mut().ok_or(Error::Expired)?;
        let Status::Offered { by, devices } = &slot.status else {
            return Err(Error::Conflict("no device is offering to add this one".into()));
        };
        slot.status = Status::Accepted { by: by.clone(), devices: devices.clone() };
        drop(live);
        self.changed();
        Ok(())
    }

    /// The person turns the offer down. The ticket stays shown for another device to scan.
    pub fn decline(&self) -> Result<()> {
        let mut live = self.live();
        let slot = live.as_mut().ok_or(Error::Expired)?;
        if !matches!(slot.status, Status::Offered { .. }) {
            return Err(Error::Conflict("no device is offering to add this one".into()));
        }
        if let Some((by, _)) = slot.by.take() {
            slot.declined.insert(by);
        }
        slot.status = Status::Showing;
        drop(live);
        self.changed();
        Ok(())
    }

    /// The slot a genuine code names.
    fn by_code<'a>(live: &'a mut Option<Slot>, code: &str) -> Result<&'a mut Slot> {
        let slot = live.as_mut().ok_or(Error::Expired)?;
        let secret = B64.decode(code).map_err(|_| Error::Unauthorized)?;
        let digest: [u8; 32] = Sha256::digest(&secret).into();
        if !bool::from(digest.ct_eq(&slot.digest)) {
            return Err(Error::Unauthorized);
        }
        Ok(slot)
    }

    fn offer(&self, code: &str, from: EndpointId, snapshot: &Snapshot) -> Result<()> {
        let list = snapshot.list()?;
        if !list.is_member(&from) {
            return Err(Error::Forbidden { permission: "a device offering its own mesh".into() });
        }
        if list.is_member(&self.me) || list.state().removed.contains(&self.me) {
            return Err(Error::Conflict("this device is, or was, in that mesh already".into()));
        }
        let mut live = self.live();
        let slot = Self::by_code(&mut live, code)?;
        if slot.declined.contains(&from) {
            return Err(Error::Forbidden { permission: "a device whose offer was not turned down".into() });
        }
        match (&slot.status, slot.by) {
            (Status::Showing, _) => {}
            // The same offer again, as after a lost answer.
            (Status::Offered { .. } | Status::Accepted { .. }, Some(by)) if by == (from, list.mesh()) => return Ok(()),
            _ => return Err(Error::Conflict("another device is offering to add this one".into())),
        }
        let (by, devices) = names(&list, &from);
        slot.status = Status::Offered { by, devices };
        slot.by = Some((from, list.mesh()));
        drop(live);
        self.changed();
        Ok(())
    }

    fn ask(&self, code: &str, from: EndpointId) -> Result<Reply> {
        let mut live = self.live();
        let slot = Self::by_code(&mut live, code)?;
        if slot.declined.contains(&from) {
            return Ok(Reply::Declined);
        }
        if slot.by.map(|(by, _)| by) != Some(from) {
            return Err(Error::Unauthorized);
        }
        Ok(match slot.status {
            Status::Showing | Status::Offered { .. } => Reply::Waiting,
            Status::Accepted { .. } | Status::Joined { .. } => Reply::Accepted,
        })
    }

    /// The mesh the member hands over, once it checks out: the accepted member's mesh, with this
    /// device in it.
    fn welcome(&self, code: &str, from: EndpointId, snapshot: &Snapshot) -> Result<()> {
        let mut live = self.live();
        let slot = Self::by_code(&mut live, code)?;
        let Some((by, mesh)) = slot.by.filter(|(by, _)| *by == from) else { return Err(Error::Unauthorized) };
        if !matches!(slot.status, Status::Accepted { .. }) {
            return Err(Error::Conflict("the person has not accepted the offer".into()));
        }
        let list = snapshot.list()?;
        if list.mesh() != mesh || !list.is_member(&by) || !list.is_member(&self.me) {
            return Err(Error::BadRequest("that mesh is not the one offered, with this device in it".into()));
        }
        Ok(())
    }

    fn joined(&self, code: &str, from: EndpointId) {
        let mut live = self.live();
        if let Ok(slot) = Self::by_code(&mut live, code)
            && let Status::Accepted { by, .. } = &slot.status
            && slot.by.is_some_and(|(by, _)| by == from)
        {
            slot.status = Status::Joined { by: by.clone() };
            drop(live);
            self.changed();
        }
    }

    fn withdraw(&self, code: &str, from: EndpointId) -> Result<()> {
        let mut live = self.live();
        let slot = Self::by_code(&mut live, code)?;
        if slot.by.map(|(by, _)| by) != Some(from) {
            return Err(Error::Unauthorized);
        }
        if !matches!(slot.status, Status::Joined { .. }) {
            slot.status = Status::Showing;
            slot.by = None;
        }
        drop(live);
        self.changed();
        Ok(())
    }
}

/// The name of `by` in `list`, and the names of every member, sorted.
fn names(list: &DeviceList, by: &EndpointId) -> (String, Vec<String>) {
    let members = &list.state().members;
    let by = members.get(by).map(|m| m.name.clone()).unwrap_or_else(|| by.fmt_short().to_string());
    let mut devices: Vec<String> = members.values().map(|m| m.name.clone()).collect();
    devices.sort_by_key(|name| name.to_lowercase());
    (by, devices)
}

/// Takes in the mesh a newcomer was added to: the newcomer's node adopting it.
pub type Adopt = Arc<dyn Fn(Snapshot) -> BoxFuture<Result<()>> + Send + Sync>;

/// A newcomer's side of [`ALPN`], for its router.
#[derive(Clone)]
pub struct JoinService {
    offers: Arc<Offers>,
    adopt: Adopt,
}

impl std::fmt::Debug for JoinService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JoinService")
    }
}

impl JoinService {
    /// Answer members offering on `offers`; `adopt` takes in the mesh once this device is added.
    pub fn new(offers: Arc<Offers>, adopt: Adopt) -> Self {
        JoinService { offers, adopt }
    }

    async fn handle(&self, remote: EndpointId, request: Request) -> Result<serde_json::Value> {
        let null = |()| serde_json::Value::Null;
        match request {
            Request::Offer { code, snapshot } => self.offers.offer(&code, remote, &snapshot).map(null),
            Request::Ask { code } => self
                .offers
                .ask(&code, remote)
                .and_then(|reply| serde_json::to_value(reply).map_err(|e| Error::Internal(e.to_string()))),
            Request::Welcome { code, snapshot } => {
                self.offers.welcome(&code, remote, &snapshot)?;
                (self.adopt)(snapshot).await?;
                self.offers.joined(&code, remote);
                Ok(serde_json::Value::Null)
            }
            Request::Withdraw { code } => self.offers.withdraw(&code, remote).map(null),
        }
    }
}

impl ProtocolHandler for JoinService {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
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
        Ok(())
    }
}

/// An offer under way, from the member's side.
pub struct Offering {
    endpoint: Endpoint,
    ticket: Ticket,
    conn: Mutex<Option<Connection>>,
}

/// Offer the mesh `snapshot` holds to the newcomer whose join ticket is `ticket`.
pub async fn offer(endpoint: &Endpoint, ticket: &Ticket, snapshot: Snapshot) -> Result<Offering> {
    if ticket.kind != Kind::Join {
        return Err(Error::BadRequest("that ticket is a device letting others in, not one asking to be added".into()));
    }
    if ticket.eid == endpoint.id() {
        return Err(Error::BadRequest("this is this device's own ticket".into()));
    }
    let offering = Offering { endpoint: endpoint.clone(), ticket: ticket.clone(), conn: Mutex::new(None) };
    offering.ask::<serde_json::Value>(&Request::Offer { code: ticket.token.clone(), snapshot }).await?;
    Ok(offering)
}

impl Offering {
    /// The newcomer's name for itself, as its ticket gives it.
    pub fn name(&self) -> &str {
        self.ticket.name.as_deref().unwrap_or_default()
    }

    /// The newcomer's key.
    pub fn newcomer(&self) -> EndpointId {
        self.ticket.eid
    }

    /// Ask until the newcomer's person answers or `patience` runs out: whether they accepted.
    pub async fn wait(&self, patience: Duration) -> Result<bool> {
        let deadline = n0_future::time::Instant::now() + patience;
        loop {
            match self.ask::<Reply>(&Request::Ask { code: self.ticket.token.clone() }).await {
                Ok(Reply::Accepted) => return Ok(true),
                Ok(Reply::Declined) => return Ok(false),
                Ok(Reply::Waiting) | Err(Error::Unreachable(_)) => {}
                Err(e) => return Err(e),
            }
            if n0_future::time::Instant::now() >= deadline {
                return Err(Error::Expired);
            }
            n0_future::time::sleep(POLL).await;
        }
    }

    /// Hand over the mesh with the newcomer added.
    pub async fn welcome(&self, snapshot: Snapshot) -> Result<()> {
        self.ask::<serde_json::Value>(&Request::Welcome { code: self.ticket.token.clone(), snapshot }).await.map(drop)
    }

    /// Take the offer back.
    pub async fn withdraw(&self) -> Result<()> {
        self.ask::<serde_json::Value>(&Request::Withdraw { code: self.ticket.token.clone() }).await.map(drop)
    }

    async fn ask<A: serde::de::DeserializeOwned>(&self, request: &Request) -> Result<A> {
        let conn = self.connection().await?;
        let answer = crate::wire::ask(&conn, request).await;
        if matches!(answer, Err(Error::Unreachable(_))) {
            self.conn.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
        answer
    }

    async fn connection(&self) -> Result<Connection> {
        if let Some(conn) = self.conn.lock().unwrap_or_else(|e| e.into_inner()).clone()
            && conn.close_reason().is_none()
        {
            return Ok(conn);
        }
        let conn = n0_future::time::timeout(
            crate::ceremony::CONNECT_TIMEOUT,
            self.endpoint.connect(self.ticket.endpoint_addr(), ALPN),
        )
        .await
        .map_err(|_| Error::Unreachable("the new device did not answer in time".into()))?
        .map_err(|e| Error::Unreachable(format!("could not reach the new device: {e}")))?;
        *self.conn.lock().unwrap_or_else(|e| e.into_inner()) = Some(conn.clone());
        Ok(conn)
    }
}
