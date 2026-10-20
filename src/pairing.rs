//! The device's side of the [ceremony](crate::ceremony): sessions held in memory.
//!
//! ```text
//! pending ── redeem ──▶ requested ── approve ──▶ approved ── collect ──▶ collected
//!                                 ── deny ─────▶ denied
//! ```
//!
//! [`Pairings::open`] mints a session and its code, `<session>.<secret>`, which goes in a
//! [ticket](crate::ticket). The newcomer redeems the code once, saying what it asks to be
//! ([`Ask`]); the Iroh key it redeemed under is bound to the session. A person approves or
//! denies, and only that key can collect the answer. Sessions end at their expiry, whatever
//! state they are in.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::ceremony::{Approval, Ask};
use crate::error::{Error, Result};
use crate::now;

/// The tag in every ceremony fingerprint.
pub const PAIR_DOMAIN: &str = "eris-pair-v1";

/// Where a session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting for a newcomer to redeem the code.
    Pending,
    /// Redeemed; waiting for a person to answer.
    Requested,
    /// Approved; the redeemer collects the answer.
    Approved,
    Denied,
    /// The redeemer has collected its answer. Collecting again returns it again.
    Collected,
}

/// One session as a device's UI sees it. The code is never part of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub status: Status,
    /// Unix seconds when the session ends.
    pub expires: i64,
    /// What the redeemer asked to be.
    pub ask: Option<Ask>,
    /// The key it redeemed under.
    pub identity: Option<iroh::EndpointId>,
    /// The comparison code both screens show once the code is redeemed.
    pub fingerprint: Option<String>,
}

/// A redeemer's answer, as [`Pairings::collect`] gives it.
#[derive(Debug, Clone)]
pub enum Collected {
    Waiting,
    Approved(Approval),
    Denied,
}

struct Slot {
    digest: [u8; 32],
    session: Session,
    approval: Option<Approval>,
}

/// Every live session of one device.
pub struct Pairings {
    sessions: Mutex<HashMap<Uuid, Slot>>,
    changes: tokio::sync::watch::Sender<u64>,
}

impl Default for Pairings {
    fn default() -> Self {
        Self::new()
    }
}

/// The fingerprint of `identity` asking for `ask` in `session`: what both screens show.
pub fn fingerprint(session: &Uuid, identity: &iroh::EndpointId, ask: &Ask) -> String {
    crate::fingerprint::fingerprint(&(PAIR_DOMAIN, session, identity, ask))
}

impl Pairings {
    pub fn new() -> Self {
        Pairings { sessions: Mutex::new(HashMap::new()), changes: tokio::sync::watch::channel(0).0 }
    }

    /// A receiver that changes whenever any session does.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn changed(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Slot>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let t = now();
        sessions.retain(|_, slot| slot.session.expires > t);
        sessions
    }

    fn all(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Slot>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Open a session for `ttl_secs`; returns its id and the code to put in a ticket.
    pub fn open(&self, ttl_secs: i64) -> (Uuid, String) {
        let id = Uuid::new_v4();
        let secret: [u8; 32] = rand::random();
        let code = format!("{id}.{}", B64.encode(secret));
        let session = Session {
            id,
            status: Status::Pending,
            expires: now().saturating_add(ttl_secs),
            ask: None,
            identity: None,
            fingerprint: None,
        };
        self.live().insert(id, Slot { digest: Sha256::digest(secret).into(), session, approval: None });
        self.changed();
        (id, code)
    }

    /// The slot a genuine code names; [`Error::Expired`] once it has ended.
    fn by_code<'a>(sessions: &'a mut HashMap<Uuid, Slot>, code: &str) -> Result<&'a mut Slot> {
        let (id, secret) = code.split_once('.').ok_or(Error::Unauthorized)?;
        let id: Uuid = id.parse().map_err(|_| Error::Unauthorized)?;
        let secret = B64.decode(secret).map_err(|_| Error::Unauthorized)?;
        let slot = sessions.get(&id).ok_or(Error::Unauthorized)?;
        let digest: [u8; 32] = Sha256::digest(&secret).into();
        if !bool::from(digest.ct_eq(&slot.digest)) {
            return Err(Error::Unauthorized);
        }
        if slot.session.expires <= now() {
            sessions.remove(&id);
            return Err(Error::Expired);
        }
        Ok(sessions.get_mut(&id).expect("checked above"))
    }

    /// Redeem a code: `identity` asks for `ask`.
    pub fn redeem(&self, code: &str, ask: Ask, identity: iroh::EndpointId) -> Result<Session> {
        ask.check()?;
        let mut all = self.all();
        let slot = Self::by_code(&mut all, code)?;
        let session = &mut slot.session;
        if session.status != Status::Pending {
            return Err(Error::Conflict(format!("this code is already {:?}", session.status).to_lowercase()));
        }
        session.fingerprint = Some(fingerprint(&session.id, &identity, &ask));
        session.status = Status::Requested;
        session.ask = Some(ask);
        session.identity = Some(identity);
        let out = session.clone();
        drop(all);
        self.changed();
        Ok(out)
    }

    /// A live session by id.
    pub fn get(&self, id: Uuid) -> Option<Session> {
        self.live().get(&id).map(|s| s.session.clone())
    }

    /// Every live session, oldest expiry first.
    pub fn list(&self) -> Vec<Session> {
        let mut sessions: Vec<Session> = self.live().values().map(|s| s.session.clone()).collect();
        sessions.sort_by_key(|s| s.expires);
        sessions
    }

    /// Approve a requested session with the answer the redeemer collects.
    pub fn approve(&self, id: Uuid, approval: Approval) -> Result<Session> {
        let mut sessions = self.live();
        let slot = sessions.get_mut(&id).ok_or(Error::Expired)?;
        if slot.session.status != Status::Requested {
            return Err(Error::Conflict("only a redeemed code can be approved".into()));
        }
        slot.session.status = Status::Approved;
        slot.approval = Some(approval);
        let out = slot.session.clone();
        drop(sessions);
        self.changed();
        Ok(out)
    }

    /// Deny a session that has not been collected.
    pub fn deny(&self, id: Uuid) -> Result<Session> {
        let mut sessions = self.live();
        let slot = sessions.get_mut(&id).ok_or(Error::Expired)?;
        if slot.session.status == Status::Collected {
            return Err(Error::Conflict("this session was already collected".into()));
        }
        slot.session.status = Status::Denied;
        slot.approval = None;
        let out = slot.session.clone();
        drop(sessions);
        self.changed();
        Ok(out)
    }

    /// The redeemer takes its request back; the session is gone.
    pub fn withdraw(&self, code: &str, identity: &iroh::EndpointId) -> Result<()> {
        let mut sessions = self.all();
        let slot = Self::by_code(&mut sessions, code)?;
        if slot.session.identity.as_ref() != Some(identity) {
            return Err(Error::Unauthorized);
        }
        let id = slot.session.id;
        sessions.remove(&id);
        drop(sessions);
        self.changed();
        Ok(())
    }

    /// The redeemer asks for its answer. Only the key that redeemed the code may ask once it
    /// has; an approved answer repeats safely.
    pub fn collect(&self, code: &str, identity: &iroh::EndpointId) -> Result<Collected> {
        let mut sessions = self.all();
        let slot = Self::by_code(&mut sessions, code)?;
        if slot.session.status == Status::Pending {
            return Ok(Collected::Waiting);
        }
        if slot.session.identity.as_ref() != Some(identity) {
            return Err(Error::Unauthorized);
        }
        let answer = match slot.session.status {
            Status::Pending | Status::Requested => Collected::Waiting,
            Status::Denied => Collected::Denied,
            Status::Approved | Status::Collected => {
                slot.session.status = Status::Collected;
                Collected::Approved(slot.approval.clone().expect("an approved session holds its answer"))
            }
        };
        let collected = matches!(answer, Collected::Approved(_));
        drop(sessions);
        if collected {
            self.changed();
        }
        Ok(answer)
    }
}

/// The session a code belongs to. A redeemer computes the fingerprint on its own screen, from
/// its own key, rather than showing whatever the device says.
pub fn code_session(code: &str) -> Option<Uuid> {
    code.split_once('.')?.0.parse().ok()
}
