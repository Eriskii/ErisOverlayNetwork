//! The requests put to this device, from ticket to answer.
//!
//! An app asks; the device cuts a pairing at ErisDB and hands the app its
//! ticket. When the pairing is redeemed, the device checks that the app
//! that asked is the one that redeemed it: the same key, name and grants.
//! Anything else is denied before the person sees it.
//!
//! A new device joins with a ticket this device shows ([`Agent::add`]).
//! That ticket crosses from one screen to another, so the person compares
//! the fingerprint both devices show instead.
//!
//! Either way the request then waits for the person, who approves all or
//! part of it, or says no. What fits inside this device's own grants the
//! device approves itself; anything more goes to ErisAuth in ErisDB with the passphrase.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use erislogin::agent::{Ask, Reply};
use erislogin::permission;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use uuid::Uuid;

use super::Joined;

/// How long an app's ticket stays redeemable, and its request answerable.
pub const TICKET_SECS: i64 = 600;

/// How often the device looks to see whether a ticket has been redeemed.
const POLL: Duration = Duration::from_millis(250);

/// Who is asking, as the operating system says: the kernel on Linux, the
/// package manager on Android.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct Peer {
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

/// Who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An app on this device, over the socket.
    App,
    /// Another device, with a ticket this device showed.
    Device,
}

/// A request waiting for the person, as the prompt and the page show it.
#[derive(Debug, Clone, Serialize)]
pub struct Approval {
    /// The ErisDB pairing this request is.
    pub id: Uuid,
    pub kind: Kind,
    pub service: String,
    pub client: String,
    pub requested: Vec<String>,
    /// The key asking; a device's is known once it redeems the ticket.
    pub identity: Option<erislogin::identity::Identity>,
    /// For a device: the code both screens show, to compare.
    pub fingerprint: Option<String>,
    /// For a device: what it asks for to be an ErisAuth device at all, which
    /// the person grants or refuses whole; the rest of `requested` is its ceiling.
    pub essential: Vec<String>,
    pub pid: Option<i32>,
    pub executable: Option<PathBuf>,
    pub package: Option<String>,
    pub certificate: Option<String>,
    /// True when the request asks for more than this device may approve by itself.
    pub needs_passphrase: bool,
    pub expires: i64,
}

#[derive(Debug, Clone)]
enum Decision {
    Approved(Vec<String>),
    Denied(String),
}

struct Pending {
    approval: Approval,
    /// False until the ticket is redeemed by the app that asked.
    asking: bool,
    decided: Option<Decision>,
}

/// This device's standing with ErisDB, once it has joined.
#[derive(Clone)]
pub struct Standing {
    pub db: Arc<erisdb_client::Client>,
    pub joined: Joined,
}

/// A join the person started on the page, until it ends.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Joining {
    /// The code to compare with the approving screen, once ErisDB has it.
    pub fingerprint: Option<String>,
    /// Why the join failed, if it did.
    pub error: Option<String>,
}

pub struct Agent {
    paths: super::Paths,
    standing: std::sync::RwLock<Option<Standing>>,
    joining: Mutex<Option<Joining>>,
    notify: bool,
    /// The approval page's link, once it is listening.
    page: std::sync::OnceLock<String>,
    /// This device's grants and when they were read; the page asks every second.
    held: tokio::sync::Mutex<Option<(std::time::Instant, Vec<String>)>>,
    pending: Mutex<BTreeMap<Uuid, Pending>>,
    changed: watch::Sender<u64>,
}

/// The answer to a decision: `{"ok": true}` or `{"error": …}`.
pub type Outcome = Value;

fn failed(error: &str, message: impl Into<String>) -> Outcome {
    json!({"error": error, "message": message.into()})
}

impl Agent {
    pub fn new(paths: super::Paths, notify: bool) -> Arc<Self> {
        Arc::new(Agent {
            paths,
            standing: std::sync::RwLock::new(None),
            joining: Mutex::new(None),
            notify,
            page: std::sync::OnceLock::new(),
            held: tokio::sync::Mutex::new(None),
            pending: Mutex::new(BTreeMap::new()),
            changed: watch::channel(0).0,
        })
    }

    pub fn set_page(&self, link: String) {
        let _ = self.page.set(link);
    }

    pub fn standing(&self) -> Option<Standing> {
        self.standing.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Take up this device's standing with ErisDB: dial it, and start
    /// pairing this device's ErisFiles with the other devices.
    pub async fn stand(&self, joined: Joined) -> anyhow::Result<()> {
        let db = Arc::new(joined.dial(&self.paths).await?);
        if let Some(peering) = super::peering::Peering::new(db.clone(), joined.clone()) {
            tokio::spawn(peering.run());
        }
        *self.standing.write().unwrap_or_else(|e| e.into_inner()) = Some(Standing { db, joined });
        *self.held.lock().await = None;
        self.changed();
        Ok(())
    }

    /// The join the person started on the page, while it lasts.
    pub fn joining(&self) -> Option<Joining> {
        self.joining.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_joining(&self, joining: Option<Joining>) {
        *self.joining.lock().unwrap_or_else(|e| e.into_inner()) = joining;
        self.changed();
    }

    /// Join ErisDB from the page with a ticket, as `erisauth join` does:
    /// `{"ok": true}` once it is asking, then [`Agent::joining`] shows the
    /// fingerprint until someone approves and the device stands joined.
    pub async fn join(self: &Arc<Self>, ticket: String, name: String, ceiling: Vec<String>) -> Outcome {
        if self.standing().is_some() {
            return failed("already_joined", "this device has joined ErisDB already");
        }
        if self.joining().is_some_and(|j| j.error.is_none()) {
            return failed("joining", "this device is already joining");
        }
        let name = name.trim().to_string();
        if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
            return failed("bad_name", "name this device in 1 to 64 printable characters");
        }
        let ceiling = if ceiling.is_empty() { vec!["*".to_string()] } else { ceiling };
        if let Err(e) = permission::check_grants(&ceiling) {
            return failed("bad_grants", e.to_string());
        }
        let asking = match super::begin_join(&self.paths, &ticket, &name, &ceiling).await {
            Ok(asking) => asking,
            Err(e) => return failed("cannot_join", format!("{e:#}")),
        };
        self.set_joining(Some(Joining { fingerprint: Some(asking.fingerprint.clone()), error: None }));
        let agent = self.clone();
        tokio::spawn(async move {
            let outcome = match asking.finish(&agent.paths).await {
                Ok((joined, _)) => agent.stand(joined).await,
                Err(e) => Err(e),
            };
            match outcome {
                Ok(()) => agent.set_joining(None),
                Err(e) => agent.set_joining(Some(Joining { fingerprint: None, error: Some(format!("{e:#}")) })),
            }
        });
        json!({"ok": true})
    }

    /// This device, as the page shows it: its name, whether it has joined
    /// ErisDB, and the grants it holds.
    pub async fn device(&self) -> Value {
        let standing = self.standing();
        let grants = match standing.as_ref().map(|s| &s.db) {
            Some(db) => {
                let mut held = self.held.lock().await;
                match &*held {
                    Some((read, grants)) if read.elapsed() < Duration::from_secs(10) => grants.clone(),
                    _ => match db.permissions().await {
                        Ok(permissions) => {
                            *held = Some((std::time::Instant::now(), permissions.grants.clone()));
                            permissions.grants
                        }
                        Err(_) => held.as_ref().map(|(_, grants)| grants.clone()).unwrap_or_default(),
                    },
                }
            }
            None => Vec::new(),
        };
        let joined = standing.as_ref().map(|s| &s.joined);
        json!({"name": joined.map(|j| &j.name), "joined": joined.is_some(), "grants": grants})
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, BTreeMap<Uuid, Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        self.changed.send_modify(|n| *n = n.wrapping_add(1));
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// The requests waiting for the person, oldest first.
    pub fn approvals(&self) -> Vec<Approval> {
        let mut approvals: Vec<Approval> =
            self.pending().values().filter(|p| p.asking && p.decided.is_none()).map(|p| p.approval.clone()).collect();
        approvals.sort_by_key(|a| a.expires);
        approvals
    }

    /// Cut a pairing at ErisDB: its id, a ticket naming ErisDB, and when it expires.
    async fn cut(&self) -> Result<(Uuid, String, i64), String> {
        let Some(Standing { db, joined }) = self.standing() else {
            return Err(
                "this device has not joined ErisDB; join it with `erisauth join` or on its ErisAuth page".into()
            );
        };
        let (status, cut) = db
            .request("POST", "/v1/pairings", Some(json!({"ttl_secs": TICKET_SECS})))
            .await
            .map_err(|e| format!("reaching ErisDB: {e:#}"))?;
        if status != 201 {
            return Err(format!("ErisDB would not open a pairing ({status}): {cut}"));
        }
        let id: Uuid = cut["id"].as_str().and_then(|id| id.parse().ok()).ok_or("ErisDB sent no pairing id")?;
        let code = cut["secret"].as_str().ok_or("ErisDB sent no code")?.to_string();
        let expires = cut["expires"].as_i64().unwrap_or(erislogin::now() + TICKET_SECS);
        let ticket = erislogin::ticket::Ticket::new(code, Some(joined.eid.clone()), None, Some(joined.name.clone()))
            .map_err(|e| e.to_string())?
            .with_addrs(joined.addrs.clone())
            .encode("erisdb")
            .map_err(|e| e.to_string())?;
        Ok((id, ticket, expires))
    }

    /// Show a ticket a new device joins with: `{id, ticket, svg, expires}`,
    /// the svg being the ticket as a QR code. Its request comes to the
    /// person like an app's, with the fingerprint to compare.
    pub async fn add(self: &Arc<Self>) -> Value {
        let (id, ticket, expires) = match self.cut().await {
            Ok(cut) => cut,
            Err(why) => return failed("cannot_add", why),
        };
        let svg = match erislogin::qr::svg(&ticket) {
            Ok(svg) => svg,
            Err(e) => return failed("cannot_add", e.to_string()),
        };
        let approval = Approval {
            id,
            kind: Kind::Device,
            service: "erisdb".into(),
            client: String::new(),
            requested: Vec::new(),
            identity: None,
            fingerprint: None,
            essential: Vec::new(),
            pid: None,
            executable: None,
            package: None,
            certificate: None,
            needs_passphrase: false,
            expires,
        };
        self.pending().insert(id, Pending { approval, asking: false, decided: None });
        let agent = self.clone();
        tokio::spawn(async move {
            if let Decision::Denied(_) = agent.settle(id).await {
                agent.deny_at_erisdb(id).await;
            }
            agent.pending().remove(&id);
            agent.changed();
        });
        json!({"id": id, "ticket": ticket, "svg": svg, "expires": expires})
    }

    /// Start an app's request: cut a pairing and return the ticket for the app.
    pub async fn open(&self, ask: &Ask, peer: Peer) -> Result<(Uuid, String), String> {
        if ask.service != "erisdb" {
            return Err(format!("ErisAuth signs apps in to erisdb, not {:?}", ask.service));
        }
        if ask.client.trim().is_empty() || ask.client.len() > 128 || ask.client.chars().any(char::is_control) {
            return Err("an app names itself in 1 to 128 printable characters".into());
        }
        permission::check_grants(&ask.requested).map_err(|e| e.to_string())?;
        let (id, ticket, expires) = self.cut().await?;
        let approval = Approval {
            id,
            kind: Kind::App,
            service: ask.service.clone(),
            client: ask.client.clone(),
            requested: ask.requested.clone(),
            identity: Some(ask.identity.clone()),
            fingerprint: None,
            essential: Vec::new(),
            pid: peer.pid,
            executable: peer.executable,
            package: peer.package,
            certificate: peer.certificate,
            needs_passphrase: false,
            expires,
        };
        self.pending().insert(id, Pending { approval, asking: false, decided: None });
        Ok((id, ticket))
    }

    /// Follow request `id` to its answer. Dropping this future withdraws the request.
    pub async fn follow(self: &Arc<Self>, id: Uuid) -> Reply {
        let decision = self.settle(id).await;
        let reply = match &decision {
            Decision::Approved(granted) => Reply::Approved(granted.clone()),
            Decision::Denied(why) => {
                self.deny_at_erisdb(id).await;
                Reply::Denied(why.clone())
            }
        };
        self.pending().remove(&id);
        self.changed();
        reply
    }

    /// Take back request `id`: the app hung up. ErisDB hears first, so that
    /// once the request is gone from the list it is denied everywhere.
    pub async fn withdraw(&self, id: Uuid) {
        if self.pending().contains_key(&id) {
            self.deny_at_erisdb(id).await;
            self.pending().remove(&id);
            self.changed();
        }
    }

    async fn deny_at_erisdb(&self, id: Uuid) {
        if let Some(Standing { db, .. }) = self.standing() {
            let _ = db.request("POST", &format!("/v1/pairings/{id}/deny"), Some(json!({}))).await;
        }
    }

    async fn settle(self: &Arc<Self>, id: Uuid) -> Decision {
        let Some(Standing { db, .. }) = self.standing() else {
            return Decision::Denied("this device has not joined ErisDB".into());
        };
        let Some(mut ask) = self.pending().get(&id).map(|p| p.approval.clone()) else {
            return Decision::Denied("the request is gone".into());
        };
        // Wait for the ticket to be redeemed.
        loop {
            if erislogin::now() >= ask.expires {
                return Decision::Denied("the ticket expired before it was redeemed".into());
            }
            if !self.pending().contains_key(&id) {
                return Decision::Denied("the app withdrew its request".into());
            }
            let session = match db.request("GET", &format!("/v1/pairings/{id}"), None).await {
                Ok((200, session)) => session,
                Ok((status, body)) => return Decision::Denied(format!("ErisDB lost the pairing ({status}): {body}")),
                Err(_) => {
                    tokio::time::sleep(POLL).await;
                    continue;
                }
            };
            let body = &session["body"];
            match body["status"].as_str() {
                Some("pending") => tokio::time::sleep(POLL).await,
                Some("requested") => match ask.kind {
                    Kind::App => {
                        let same = body["identity"] == json!(ask.identity)
                            && body["client"] == json!(ask.client)
                            && body["requested"] == json!(ask.requested);
                        if !same {
                            return Decision::Denied(
                                "a different app redeemed the ticket than the one that asked; nothing was approved"
                                    .into(),
                            );
                        }
                        break;
                    }
                    // The person checks a device by the fingerprint both screens show.
                    Kind::Device => {
                        ask.client = body["client"].as_str().unwrap_or("a device").to_string();
                        ask.requested = serde_json::from_value(body["requested"].clone()).unwrap_or_default();
                        ask.identity = serde_json::from_value(body["identity"].clone()).ok();
                        ask.fingerprint = body["fingerprint"].as_str().map(str::to_string);
                        ask.essential =
                            ask.requested.iter().filter(|g| super::BASE.contains(&g.as_str())).cloned().collect();
                        break;
                    }
                },
                Some(other) => return Decision::Denied(format!("the pairing is {other}")),
                None => return Decision::Denied("ErisDB sent a pairing with no status".into()),
            }
        }

        // Put it to the person.
        let needs_passphrase = match db.permissions().await {
            Ok(held) => !permission::encloses(&held.grants, &ask.requested),
            Err(_) => true,
        };
        let mut changes = self.subscribe();
        let shown = {
            let mut pending = self.pending();
            let Some(entry) = pending.get_mut(&id) else { return Decision::Denied("the request is gone".into()) };
            ask.needs_passphrase = needs_passphrase;
            entry.asking = true;
            entry.approval = ask.clone();
            entry.approval.clone()
        };
        self.changed();
        if self.notify {
            super::notify::asked(&shown, self.page.get().cloned());
        }
        loop {
            match self.pending().get_mut(&id) {
                None => return Decision::Denied("the app withdrew its request".into()),
                Some(pending) => {
                    if let Some(decision) = pending.decided.take() {
                        return decision;
                    }
                }
            }
            let left = ask.expires.saturating_sub(erislogin::now());
            if left <= 0 {
                return Decision::Denied("nobody answered in time".into());
            }
            let _ = tokio::time::timeout(Duration::from_secs(left as u64), changes.changed()).await;
        }
    }

    /// Wait up to `within` for request `id` to reach the person: its
    /// approval once it does, `None` if it ends first or time runs out.
    pub async fn asked(&self, id: Uuid, within: Duration) -> Option<Approval> {
        let mut changes = self.subscribe();
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(waiting) = self.waiting(id) {
                return Some(waiting);
            }
            if !self.pending().contains_key(&id) {
                return None;
            }
            if tokio::time::timeout_at(deadline, changes.changed()).await.is_err() {
                return None;
            }
        }
    }

    fn waiting(&self, id: Uuid) -> Option<Approval> {
        self.pending().get(&id).filter(|p| p.asking && p.decided.is_none()).map(|p| p.approval.clone())
    }

    fn decide_now(&self, id: Uuid, decision: Decision) {
        if let Some(pending) = self.pending().get_mut(&id) {
            pending.decided = Some(decision);
        }
        self.changed();
    }

    /// The person approves `granted` for request `id`, with the passphrase
    /// when it asks for more than this device may approve by itself.
    pub async fn approve(&self, id: Uuid, granted: Vec<String>, passphrase: Option<String>) -> Outcome {
        let Some(Standing { db, .. }) = self.standing() else {
            return failed("not_joined", "this device has not joined ErisDB");
        };
        let Some(approval) = self.waiting(id) else {
            return failed("no_such_request", "nothing is waiting for an answer under that id");
        };
        if let Err(e) = permission::check_grants(&granted) {
            return failed("bad_grants", e.to_string());
        }
        if !permission::encloses(&approval.requested, &granted) {
            return failed("not_requested", "approve only what was asked for");
        }
        let held = match db.permissions().await {
            Ok(held) => held.grants,
            Err(e) => return failed("unreachable", format!("reaching ErisDB: {e:#}")),
        };
        let (status, body) = if permission::encloses(&held, &granted) {
            db.request("POST", &format!("/v1/pairings/{id}/approve"), Some(json!({"granted": granted}))).await
        } else {
            let Some(passphrase) = passphrase else {
                return failed("needs_passphrase", "this asks for more than this device may approve by itself");
            };
            let Some(identity) = approval.identity else {
                return failed("refused", "the request names no key for ErisAuth to check");
            };
            let input = json!({"pairing": id, "granted": granted, "identity": identity, "passphrase": passphrase});
            db.call_plugin("erisauth", "approve", input).await
        }
        .unwrap_or_else(|e| (0, json!({"error": "unreachable", "message": format!("{e:#}")})));
        if status != 200 {
            // The plugin says `message`; ErisDB itself says `detail`.
            let message = body["message"].as_str().or(body["detail"].as_str()).map(str::to_string);
            let message = message.unwrap_or_else(|| format!("ErisDB refused ({status}): {body}"));
            return match body["error"].as_str() {
                Some("plugin_not_found") => failed(
                    "not_set_up",
                    "ErisDB has no ErisAuth plugin to ask; install it and run `erisauth setup` on the ErisDB machine",
                ),
                Some(error) => {
                    let mut refusal = failed(error, message);
                    if let Some(wait) = body["retry_after"].as_i64() {
                        refusal["retry_after"] = json!(wait);
                    }
                    refusal
                }
                None => failed("refused", message),
            };
        }
        self.decide_now(id, Decision::Approved(granted));
        json!({"ok": true})
    }

    /// The person says no to request `id`.
    pub fn deny(&self, id: Uuid) -> Outcome {
        if self.waiting(id).is_none() {
            return failed("no_such_request", "nothing is waiting for an answer under that id");
        }
        self.decide_now(id, Decision::Denied("the person said no".into()));
        json!({"ok": true})
    }
}
