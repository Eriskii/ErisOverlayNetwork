//! Pairing sessions held in memory, for hosts without a database.
//!
//! ```text
//! pending ── redeem ──▶ requested ── approve ──▶ approved ── collect ──▶ collected
//!                                 ── deny ─────▶ denied
//! ```
//!
//! [`Pairings::open`] mints a session and its code, `<session>.<secret>`,
//! which goes in a [ticket](crate::ticket). The newcomer redeems the code
//! once, naming itself and the grants it would like; its [`Identity`] is
//! bound to the session. A person approves some or all of the request, or
//! denies it, and only that identity can collect the answer. Sessions end
//! at their expiry, whatever state they are in.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::{now, permission};

/// The longest a display name may be.
pub const MAX_CLIENT_LEN: usize = 128;

/// Where a session stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// Waiting for a newcomer to redeem the code.
    Pending,
    /// Redeemed; waiting for a person to answer.
    Requested,
    /// Approved with these grants; the redeemer collects them.
    Approved {
        granted: Vec<String>,
    },
    Denied,
    /// The redeemer has collected these grants. Collecting again returns them again.
    Collected {
        granted: Vec<String>,
    },
}

/// One pairing session as a host's UI sees it. The code is never part of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    #[serde(flatten)]
    pub status: Status,
    /// Unix seconds when the session ends.
    pub expires: i64,
    /// The redeemer's name for itself. Shown to the person, trusted for nothing.
    pub client: Option<String>,
    pub requested: Vec<String>,
    pub identity: Option<Identity>,
    /// The comparison code both screens show once the code is redeemed.
    pub fingerprint: Option<String>,
}

struct Entry {
    digest: [u8; 32],
    session: Session,
}

/// Every live pairing session of one host.
pub struct Pairings {
    domain: &'static str,
    sessions: Mutex<HashMap<Uuid, Entry>>,
    changes: tokio::sync::watch::Sender<u64>,
}

impl Pairings {
    /// `domain` separates this app's fingerprints from every other's, e.g.
    /// `"erisfiles-pair-v1"`.
    pub fn new(domain: &'static str) -> Self {
        Pairings { domain, sessions: Mutex::new(HashMap::new()), changes: tokio::sync::watch::channel(0).0 }
    }

    /// A receiver that changes whenever any session does.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn changed(&self) {
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Entry>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let t = now();
        sessions.retain(|_, entry| entry.session.expires > t);
        sessions
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
            client: None,
            requested: Vec::new(),
            identity: None,
            fingerprint: None,
        };
        self.live().insert(id, Entry { digest: Sha256::digest(secret).into(), session });
        self.changed();
        (id, code)
    }

    /// Every session, expired ones included, so that a code can be told it
    /// expired rather than that it is unknown.
    fn all(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The session a genuine code names; [`Error::Expired`] once it has ended.
    fn by_code<'a>(sessions: &'a mut HashMap<Uuid, Entry>, code: &str) -> Result<&'a mut Session> {
        let (id, secret) = code.split_once('.').ok_or(Error::Unauthorized)?;
        let id: Uuid = id.parse().map_err(|_| Error::Unauthorized)?;
        let secret = B64.decode(secret).map_err(|_| Error::Unauthorized)?;
        let entry = sessions.get_mut(&id).ok_or(Error::Unauthorized)?;
        let digest: [u8; 32] = Sha256::digest(&secret).into();
        if !bool::from(digest.ct_eq(&entry.digest)) {
            return Err(Error::Unauthorized);
        }
        if entry.session.expires <= now() {
            sessions.remove(&id);
            return Err(Error::Expired);
        }
        Ok(&mut sessions.get_mut(&id).expect("checked above").session)
    }

    /// Redeem a code: `client` asks for `requested`, as `identity`.
    pub fn redeem(&self, code: &str, client: &str, requested: Vec<String>, identity: Identity) -> Result<Session> {
        if client.trim().is_empty() || client.len() > MAX_CLIENT_LEN || client.chars().any(char::is_control) {
            return Err(Error::BadRequest(format!("client must be 1..={MAX_CLIENT_LEN} printable characters")));
        }
        permission::check_grants(&requested)?;
        let mut all = self.all();
        let session = Self::by_code(&mut all, code)?;
        if session.status != Status::Pending {
            return Err(Error::Conflict(format!("this pairing code is already {}", status_name(&session.status))));
        }
        session.fingerprint = Some(identity.fingerprint(self.domain, &session.id, &requested));
        session.status = Status::Requested;
        session.client = Some(client.to_string());
        session.requested = requested;
        session.identity = Some(identity);
        let out = session.clone();
        drop(all);
        self.changed();
        Ok(out)
    }

    /// A live session by id.
    pub fn get(&self, id: Uuid) -> Option<Session> {
        self.live().get(&id).map(|e| e.session.clone())
    }

    /// Every live session, oldest expiry first.
    pub fn list(&self) -> Vec<Session> {
        let mut sessions: Vec<Session> = self.live().values().map(|e| e.session.clone()).collect();
        sessions.sort_by_key(|s| s.expires);
        sessions
    }

    /// Approve a requested session with `granted`, or with everything it
    /// asked for when `None`. The grants must fit inside the request.
    pub fn approve(&self, id: Uuid, granted: Option<Vec<String>>) -> Result<Session> {
        let mut sessions = self.live();
        let session = &mut sessions.get_mut(&id).ok_or(Error::Expired)?.session;
        if session.status != Status::Requested {
            return Err(Error::Conflict("only a redeemed pairing code can be approved".into()));
        }
        let granted = granted.unwrap_or_else(|| session.requested.clone());
        permission::check_grants(&granted)?;
        if !permission::encloses(&session.requested, &granted) {
            return Err(Error::Forbidden { permission: granted.join(",") });
        }
        session.status = Status::Approved { granted };
        let out = session.clone();
        drop(sessions);
        self.changed();
        Ok(out)
    }

    /// Deny a session that has not been collected.
    pub fn deny(&self, id: Uuid) -> Result<Session> {
        let mut sessions = self.live();
        let session = &mut sessions.get_mut(&id).ok_or(Error::Expired)?.session;
        if matches!(session.status, Status::Collected { .. }) {
            return Err(Error::Conflict("this pairing was already collected".into()));
        }
        session.status = Status::Denied;
        let out = session.clone();
        drop(sessions);
        self.changed();
        Ok(out)
    }

    /// The redeemer asks for its answer. `Pending` and `Requested` mean
    /// wait; `Approved` carries the grants, and repeats safely; `Denied` is
    /// final. Only the identity that redeemed the code may ask once it has.
    pub fn collect(&self, code: &str, identity: &Identity) -> Result<Status> {
        let mut sessions = self.all();
        let session = Self::by_code(&mut sessions, code)?;
        if session.status == Status::Pending {
            return Ok(Status::Pending);
        }
        if session.identity.as_ref() != Some(identity) {
            return Err(Error::Unauthorized);
        }
        let answer = match &session.status {
            Status::Approved { granted } => {
                let granted = granted.clone();
                session.status = Status::Collected { granted: granted.clone() };
                Status::Approved { granted }
            }
            Status::Collected { granted } => Status::Approved { granted: granted.clone() },
            other => other.clone(),
        };
        let collected = matches!(answer, Status::Approved { .. });
        drop(sessions);
        if collected {
            self.changed();
        }
        Ok(answer)
    }
}

/// The session a code belongs to. A redeemer uses it to compute the
/// request's fingerprint on its own screen, from its own identity, rather
/// than showing whatever the host says.
pub fn code_session(code: &str) -> Option<Uuid> {
    code.split_once('.')?.0.parse().ok()
}

fn status_name(status: &Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Requested => "requested",
        Status::Approved { .. } => "approved",
        Status::Denied => "denied",
        Status::Collected { .. } => "collected",
    }
}
