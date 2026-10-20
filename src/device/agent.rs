//! What apps and newcomers ask a device, put to the person, and the person's answer carried out.
//!
//! Requests come two ways. An app beside ErisAuth asks over the [agent socket](super::socket)
//! (on Android, through the app's bound service), and the operating system says which program
//! is asking: the kernel's `/proc/<pid>/exe` for the socket's peer, or the calling package and
//! its signing certificate on Android. A newcomer with a ticket this device showed asks over
//! the ceremony: a new device, or an app with no ErisAuth beside it, such as one in a browser.
//! The person compares the fingerprint both screens show.
//!
//! The person approves an app as asked, approves part of its grants, or denies it:
//!
//! - an app for a mesh service gets its note at once;
//! - an app asking to serve `erisdb` asks to be the mesh's ErisDB. There is one: approving it
//!   also revokes every other approval to serve `erisdb` this device knows of;
//! - an app for `erisdb` that does not serve it takes the ErisDB passphrase as well. The device
//!   enrolls the app's note at the mesh's ErisDB with it, and the app hears it is approved only
//!   once ErisDB has accepted. A wrong passphrase lets the person try again; once ErisDB locks
//!   this device out, the person hears how long to wait.
//!
//! Approving a new device adds it to the list. An app that hangs up withdraws its request.

use std::collections::btree_map::Entry;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use erisoverlaynetwork::agent::{self, Reply};
use erisoverlaynetwork::ceremony::{self, Approval};
use erisoverlaynetwork::devices::Op;
use erisoverlaynetwork::pairing::{Session, Status};
use erisoverlaynetwork::{Note, permission};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::{Daemon, Purpose, TICKET_SECS};

/// The service a mesh has one server of, whose clients take the passphrase.
pub const ERISDB: &str = "erisdb";

/// Who is asking, as the operating system says: the kernel on Linux, the package manager on
/// Android.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Caller {
    #[serde(default)]
    pub pid: Option<i32>,
    #[serde(default)]
    pub executable: Option<PathBuf>,
    /// The Android package asking.
    #[serde(default)]
    pub package: Option<String>,
    /// The SHA-256 of that package's signing certificate, in hex.
    #[serde(default)]
    pub certificate: Option<String>,
}

impl Caller {
    /// The socket peer `pid`, as the kernel names its executable.
    pub fn process(pid: Option<i32>) -> Self {
        let executable = pid.and_then(erisoverlaynetwork::sys::executable);
        Caller { pid, executable, ..Caller::default() }
    }

    /// What goes in the note's `program`: the executable's path, or the Android package and the
    /// SHA-256 of its signing certificate, as `dev.example.notes sha256:<hex>`.
    pub fn program(&self) -> Option<String> {
        match (&self.package, &self.certificate) {
            (Some(package), Some(certificate)) => Some(format!("{package} sha256:{certificate}")),
            (Some(package), None) => Some(package.clone()),
            (None, _) => self.executable.as_ref().map(|path| path.display().to_string()),
        }
    }
}

/// What a request asks to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    App,
    Device,
}

/// How a request came.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    /// Over the agent socket, or Android's bound service: an app on this device.
    Agent,
    /// By a ticket this device showed.
    Ticket,
}

/// A request as the person's screens show it.
#[derive(Debug, Clone, Serialize)]
pub struct Prompt {
    pub id: Uuid,
    pub kind: Kind,
    pub via: Via,
    /// The app's or the new device's name for itself. Shown, trusted for nothing.
    pub name: String,
    /// The key asking: the app's, or the new device's.
    pub key: EndpointId,
    pub service: Option<String>,
    pub grants: Vec<String>,
    pub serves: bool,
    /// What the operating system says is asking, as the note will record it.
    pub program: Option<String>,
    pub pid: Option<i32>,
    pub package: Option<String>,
    pub certificate: Option<String>,
    /// For a request by ticket: the code both screens show, to compare.
    pub fingerprint: Option<String>,
    /// Approving takes the ErisDB passphrase.
    pub passphrase: bool,
    /// Approving makes the app this mesh's ErisDB, in place of the one there is now.
    pub becomes_erisdb: bool,
    /// Unix seconds past which the request is gone.
    pub expires: i64,
    /// The person's approval is being carried out.
    pub busy: bool,
}

/// Where the answer goes.
pub(crate) enum Answer {
    /// To the app waiting on the socket.
    Agent(oneshot::Sender<Reply>),
    /// To the ceremony session, for the newcomer to collect.
    Session,
}

pub(crate) struct Pending {
    pub prompt: Prompt,
    pub answer: Answer,
}

/// Why something the person asked for did not happen: `{"error": code, "message": why}`.
pub fn failed(code: &str, message: impl Into<String>) -> Value {
    json!({"error": code, "message": message.into()})
}

fn ok() -> Value {
    json!({"ok": true})
}

/// Takes a socket request back when its future is dropped: the app hung up.
struct Withdraw<'a> {
    daemon: &'a Daemon,
    id: Uuid,
}

impl Drop for Withdraw<'_> {
    fn drop(&mut self) {
        self.daemon.withdraw(self.id);
    }
}

impl Daemon {
    pub(crate) fn requests(&self) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<Uuid, Pending>> {
        self.requests.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What waits for the person, oldest first.
    pub fn prompts(&self) -> Vec<Prompt> {
        let mut prompts: Vec<Prompt> = self.requests().values().map(|p| p.prompt.clone()).collect();
        prompts.sort_by_key(|p| p.expires);
        prompts
    }

    /// Why an app may not ask for `service` under `app` at all, before anyone is asked.
    fn refuse(&self, name: &str, service: &str, grants: &[String], app: &EndpointId) -> Result<(), String> {
        erisoverlaynetwork::check_name(name).map_err(|e| e.to_string())?;
        erisoverlaynetwork::note::check_service(service).map_err(|e| e.to_string())?;
        if !grants.is_empty() {
            permission::check_grants(grants).map_err(|e| e.to_string())?;
        }
        let list = self.node.list().ok_or("this device is in no mesh yet")?;
        if !list.is_member(&self.id()) {
            return Err("this device is no longer in its mesh".into());
        }
        if list.is_member(app) || list.state().removed.contains(app) {
            return Err("an app holds a key of its own, not a device's".into());
        }
        Ok(())
    }

    fn app_prompt(&self, id: Uuid, via: Via, ask: &agent::Ask, caller: &Caller, expires: i64) -> Prompt {
        Prompt {
            id,
            kind: Kind::App,
            via,
            name: ask.name.trim().to_string(),
            key: ask.app,
            service: Some(ask.service.clone()),
            grants: ask.grants.clone(),
            serves: ask.serves,
            program: caller.program(),
            pid: caller.pid,
            package: caller.package.clone(),
            certificate: caller.certificate.clone(),
            fingerprint: None,
            passphrase: ask.service == ERISDB && !ask.serves,
            becomes_erisdb: ask.service == ERISDB && ask.serves,
            expires,
            busy: false,
        }
    }

    /// An app on this device asks, as `caller`; the reply once the person answers. Dropping the
    /// future withdraws the request.
    pub async fn ask(&self, ask: agent::Ask, caller: Caller) -> Reply {
        if let Err(why) = self.refuse(&ask.name, &ask.service, &ask.grants, &ask.app) {
            return Reply::Error(if self.node.mesh().is_none() {
                format!("{why}; start one with `erisauth init` or join one with `erisauth join`")
            } else {
                why
            });
        }
        let id = Uuid::new_v4();
        let expires = erisoverlaynetwork::now() + TICKET_SECS;
        let prompt = self.app_prompt(id, Via::Agent, &ask, &caller, expires);
        let (send, answered) = oneshot::channel();
        self.requests().insert(id, Pending { prompt: prompt.clone(), answer: Answer::Agent(send) });
        self.changed();
        if self.notifies() {
            super::notify::asked(&prompt, self.page());
        }
        let _withdraw = Withdraw { daemon: self, id };
        match tokio::time::timeout(Duration::from_secs(TICKET_SECS as u64), answered).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Reply::Error("ErisAuth dropped the request".into()),
            Err(_) => Reply::Denied("nobody answered in time".into()),
        }
    }

    /// Take a request back, unless the person's approval is being carried out.
    pub(crate) fn withdraw(&self, id: Uuid) {
        let gone = match self.requests().entry(id) {
            Entry::Occupied(entry) if !entry.get().prompt.busy => {
                entry.remove();
                true
            }
            _ => false,
        };
        if gone {
            self.changed();
        }
    }

    /// Bring the ceremony's sessions into what waits for the person: a newcomer redeemed one of
    /// this device's tickets, or a session ended.
    pub(crate) fn take_sessions(&self) {
        let live: Vec<Session> = self.pairings.list();
        let mut changed = false;
        let mut opened = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        opened.retain(|id, _| live.iter().any(|s| s.id == *id));
        for session in &live {
            let Some(purpose) = opened.get(&session.id).copied() else { continue };
            if session.status != Status::Requested || self.requests().contains_key(&session.id) {
                continue;
            }
            match self.session_prompt(session, purpose) {
                Ok(prompt) => {
                    if self.notifies() {
                        super::notify::asked(&prompt, self.page());
                    }
                    self.requests().insert(session.id, Pending { prompt, answer: Answer::Session });
                }
                // What could never be approved is turned away before the person sees it.
                Err(why) => {
                    eprintln!("erisauth: turning away a redeemed ticket: {why}");
                    let _ = self.pairings.deny(session.id);
                }
            }
            changed = true;
        }
        drop(opened);
        // A request by ticket lasts as long as its session waits for an answer; an app's, as long
        // as the app waits.
        self.requests().retain(|id, pending| {
            let keep = pending.prompt.busy
                || match &pending.answer {
                    Answer::Session => live.iter().any(|s| s.id == *id && s.status == Status::Requested),
                    Answer::Agent(send) => !send.is_closed(),
                };
            changed |= !keep;
            keep
        });
        if changed {
            self.changed();
        }
    }

    fn session_prompt(&self, session: &Session, purpose: Purpose) -> Result<Prompt, String> {
        let (Some(ask), Some(key)) = (&session.ask, session.identity) else {
            return Err("the session holds no request".into());
        };
        match (ask, purpose) {
            (ceremony::Ask::Device { name }, Purpose::Device) => {
                let list = self.node.list().ok_or("this device is in no mesh")?;
                if list.is_member(&key) || list.state().removed.contains(&key) {
                    return Err("that key is, or was, a device in this mesh".into());
                }
                Ok(Prompt {
                    id: session.id,
                    kind: Kind::Device,
                    via: Via::Ticket,
                    name: name.trim().to_string(),
                    key,
                    service: None,
                    grants: Vec::new(),
                    serves: false,
                    program: None,
                    pid: None,
                    package: None,
                    certificate: None,
                    fingerprint: session.fingerprint.clone(),
                    passphrase: false,
                    becomes_erisdb: false,
                    expires: session.expires,
                    busy: false,
                })
            }
            (ceremony::Ask::App { name, service, grants, serves }, Purpose::App) => {
                self.refuse(name, service, grants, &key)?;
                let ask = agent::Ask {
                    service: service.clone(),
                    name: name.clone(),
                    grants: grants.clone(),
                    serves: *serves,
                    app: key,
                };
                let mut prompt = self.app_prompt(session.id, Via::Ticket, &ask, &Caller::default(), session.expires);
                prompt.fingerprint = session.fingerprint.clone();
                Ok(prompt)
            }
            (ceremony::Ask::Device { .. }, Purpose::App) => Err("a device redeemed a ticket for an app".into()),
            (ceremony::Ask::App { .. }, Purpose::Device) => Err("an app redeemed a ticket for a device".into()),
        }
    }

    /// The person approves request `id`: an app with `grants` (as asked when `None`) and, for an
    /// ErisDB client, the `passphrase`; a device as it asked.
    pub async fn decide(&self, id: Uuid, grants: Option<Vec<String>>, passphrase: Option<String>) -> Value {
        let prompt = {
            let mut requests = self.requests();
            let Some(pending) = requests.get_mut(&id) else {
                return failed("no_such_request", "nothing is waiting for an answer under that id");
            };
            if pending.prompt.busy {
                return failed("busy", "this request is being approved already");
            }
            pending.prompt.busy = true;
            pending.prompt.clone()
        };
        self.changed();
        let outcome = match prompt.kind {
            Kind::Device => self.add_device(&prompt).await,
            Kind::App => self.approve_app(&prompt, grants, passphrase).await,
        };
        let refusal = match outcome {
            Ok(answer) => {
                let pending = self.requests().remove(&id);
                self.changed();
                return match (pending.map(|p| p.answer), answer) {
                    (Some(Answer::Agent(send)), Approved::App { note, snapshot }) => {
                        let _ = send.send(Reply::Approved { note, snapshot });
                        ok()
                    }
                    (Some(Answer::Session), approval) => match self.pairings.approve(id, approval.into()) {
                        Ok(_) => ok(),
                        Err(e) => failed("expired", format!("the newcomer's ticket ended first: {e}")),
                    },
                    (None, _) => failed("withdrawn", "the app withdrew its request"),
                    (Some(Answer::Agent(_)), Approved::Device { .. }) => failed("internal", "a device asked as an app"),
                };
            }
            Err(refusal) => refusal,
        };
        // The request stays for the person to try again, unless its app has gone meanwhile.
        {
            let mut requests = self.requests();
            match requests.get_mut(&id) {
                Some(pending) if matches!(&pending.answer, Answer::Agent(send) if send.is_closed()) => {
                    requests.remove(&id);
                }
                Some(pending) => pending.prompt.busy = false,
                None => {}
            }
        }
        self.changed();
        refusal
    }

    /// The person says no to request `id`.
    pub fn deny(&self, id: Uuid) -> Value {
        let pending = match self.requests().entry(id) {
            Entry::Occupied(entry) if entry.get().prompt.busy => {
                return failed("busy", "this request is being approved already");
            }
            Entry::Occupied(entry) => entry.remove(),
            Entry::Vacant(_) => return failed("no_such_request", "nothing is waiting for an answer under that id"),
        };
        match pending.answer {
            Answer::Agent(send) => {
                let _ = send.send(Reply::Denied("the person said no".into()));
            }
            Answer::Session => {
                let _ = self.pairings.deny(id);
            }
        }
        self.changed();
        ok()
    }

    async fn add_device(&self, prompt: &Prompt) -> Result<Approved, Value> {
        self.node
            .append(Op::Add { device: prompt.key, name: prompt.name.clone() })
            .await
            .map_err(|e| failed("refused", e.to_string()))?;
        let snapshot = self.node.snapshot().await.ok_or_else(|| failed("internal", "the mesh went away"))?;
        Ok(Approved::Device { snapshot })
    }

    async fn approve_app(
        &self,
        prompt: &Prompt,
        grants: Option<Vec<String>>,
        passphrase: Option<String>,
    ) -> Result<Approved, Value> {
        let service = prompt.service.clone().unwrap_or_default();
        let grants = grants.unwrap_or_else(|| prompt.grants.clone());
        if !grants.is_empty() {
            permission::check_grants(&grants).map_err(|e| failed("bad_request", e.to_string()))?;
        }
        if !permission::encloses(&prompt.grants, &grants) {
            return Err(failed("not_asked", "approve only what was asked for"));
        }
        if grants.is_empty() && !prompt.grants.is_empty() {
            return Err(failed("bad_request", "approve at least one of the grants asked for, or deny"));
        }
        let passphrase = match (prompt.passphrase, passphrase.filter(|p| !p.is_empty())) {
            (true, None) => return Err(failed("needs_passphrase", "approving this takes the ErisDB passphrase")),
            (true, Some(passphrase)) => Some(passphrase),
            (false, _) => None,
        };
        if passphrase.is_some() && self.node.provider(ERISDB).is_none() {
            return Err(self.no_erisdb());
        }

        let note = self
            .node
            .issue(prompt.key, &prompt.name, &service, grants, prompt.serves, prompt.program.clone())
            .map_err(|e| failed("refused", e.to_string()))?;
        if let Some(passphrase) = passphrase {
            let (status, body) =
                super::erisdb::post(&self.node, "/v1/enroll", &json!({"note": note, "passphrase": passphrase}))
                    .await
                    .map_err(|e| failed("unreachable", format!("reaching this mesh's ErisDB: {e:#}")))?;
            if !(200..300).contains(&status) {
                return Err(super::erisdb::refusal(status, &body));
            }
        }
        if let Err(e) = self.record(note.clone()) {
            eprintln!("erisauth: could not keep the note this device issued: {e:#}");
        }
        if prompt.becomes_erisdb {
            for other in self.erisdb_approvals().into_iter().filter(|other| other.id != note.id) {
                self.node.append(Op::Revoke { note: other.id }).await.map_err(|e| failed("refused", e.to_string()))?;
            }
        }
        let snapshot = self.node.snapshot().await.ok_or_else(|| failed("internal", "the mesh went away"))?;
        Ok(Approved::App { note, snapshot })
    }
}

/// What the person's approval comes to.
#[allow(clippy::large_enum_variant)]
enum Approved {
    Device { snapshot: erisoverlaynetwork::Snapshot },
    App { note: Note, snapshot: erisoverlaynetwork::Snapshot },
}

impl From<Approved> for Approval {
    fn from(approved: Approved) -> Self {
        match approved {
            Approved::Device { snapshot } => Approval::Device { snapshot },
            Approved::App { note, snapshot } => Approval::App { note, snapshot },
        }
    }
}

impl Daemon {
    /// Every approval to serve `erisdb` that this device knows of and that still counts: those
    /// of the ErisDBs it has heard from, and those it issued itself.
    pub(crate) fn erisdb_approvals(&self) -> Vec<Note> {
        let heard = self.node.providers(ERISDB).into_iter().filter_map(|p| p.note().cloned());
        let issued = self.issued().into_iter().filter(|n| n.body.service == ERISDB && n.body.serves);
        let mut approvals: Vec<Note> = Vec::new();
        for note in heard.chain(issued) {
            if self.node.counts(&note) && !approvals.iter().any(|known| known.id == note.id) {
                approvals.push(note);
            }
        }
        approvals
    }

    /// Why there is nowhere to check the passphrase.
    pub(crate) fn no_erisdb(&self) -> Value {
        if self.erisdb_approvals().is_empty() {
            failed("no_erisdb", "this mesh has no ErisDB yet; run `erisdb join` where ErisDB runs")
        } else {
            failed("no_erisdb", "this mesh's ErisDB has not been heard from; is it running?")
        }
    }

    /// Show a ticket for `purpose`: `{id, ticket, svg, qr: {side, modules}, expires}`. The QR code
    /// comes drawn as an SVG for the page and laid out for the Android app to draw: `side`
    /// modules a side, `modules` a `1` for each dark one, row by row.
    pub async fn open_ticket(&self, purpose: Purpose) -> Value {
        if !self.is_member() {
            return failed("not_in_a_mesh", "this device is in no mesh; start one or join one first");
        }
        let (id, code) = self.pairings.open(TICKET_SECS);
        let expires = self.pairings.get(id).map(|s| s.expires).unwrap_or_default();
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(id, purpose);
        let addr = erisoverlaynetwork::net::advertised(self.node.endpoint(), &self.network).await;
        let encoded = erisoverlaynetwork::ticket::Ticket::new(code, self.id(), self.name())
            .map(|ticket| ticket.at(&addr))
            .and_then(|ticket| ticket.encode());
        let ticket = match encoded {
            Ok(ticket) => ticket,
            Err(e) => return failed("internal", e.to_string()),
        };
        let (svg, raster) = match (erisoverlaynetwork::qr::svg(&ticket), erisoverlaynetwork::qr::raster(&ticket, 1)) {
            (Ok(svg), Ok(raster)) => (svg, raster),
            (Err(e), _) | (_, Err(e)) => return failed("internal", e.to_string()),
        };
        let modules: String = raster.1.iter().map(|&p| if p == 0 { '1' } else { '0' }).collect();
        self.changed();
        json!({
            "id": id,
            "purpose": purpose,
            "ticket": ticket,
            "svg": svg,
            "qr": {"side": raster.0, "modules": modules},
            "expires": expires,
        })
    }

    /// Close a ticket nobody has redeemed yet.
    pub fn close_ticket(&self, id: Uuid) -> Value {
        let opened = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&id).is_some();
        if !opened {
            return failed("no_such_ticket", "no ticket of this device's is open under that id");
        }
        if self.pairings.get(id).is_some_and(|s| s.status == Status::Pending) {
            let _ = self.pairings.deny(id);
        }
        self.changed();
        ok()
    }

    /// The tickets this device shows whose newcomer has not been answered yet: `pending` until
    /// one redeems it, `requested` once one has.
    pub fn tickets(&self) -> Vec<Value> {
        let opened = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.pairings
            .list()
            .into_iter()
            .filter(|s| matches!(s.status, Status::Pending | Status::Requested))
            .filter_map(|s| {
                let purpose = opened.get(&s.id)?;
                Some(json!({"id": s.id, "purpose": purpose, "status": s.status, "expires": s.expires}))
            })
            .collect()
    }

    /// Follow the ceremony's sessions for as long as the daemon runs. Sessions end at their
    /// expiry without a word, so they are looked at every few seconds besides.
    pub(crate) async fn follow_sessions(self: Arc<Self>) {
        let mut changes = self.pairings.subscribe();
        loop {
            self.take_sessions();
            tokio::select! {
                changed = changes.changed() => if changed.is_err() { return },
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}
