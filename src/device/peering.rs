//! Making this device an ErisFiles peer of the person's other ErisAuth
//! devices, with no ceremony of its own: approving a device into ErisAuth
//! is the approval.
//!
//! Each device announces itself in ErisDB's `erisauth.devices` facet: its
//! installation, its name, and its ErisFiles device id when ErisFiles runs
//! here. ErisDB stamps every item with the installation that wrote it, so a
//! record counts only when it announces the installation that wrote it.
//!
//! Of two announced devices that are not yet peers, the one with the lower
//! ErisFiles id invites: it opens a pairing on its own ErisFiles and posts
//! the ticket to the other in `erisauth.mail`. The other joins only an
//! invitation that its sender wrote itself, from an announced device, for
//! the ErisFiles that device announced. The inviter approves only the
//! ErisFiles key the invitee announced. ErisFiles grants apply both ways, so
//! joining a stranger's ticket would open this device's files to them;
//! every one of those checks stands between a forged invitation and that.
//!
//! Holding `erisauth.devices:create` is what lets an installation announce
//! a device the others will share files with; only ErisAuth devices ask for it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::Joined;

pub const DEVICES: &str = "erisauth.devices";
pub const MAIL: &str = "erisauth.mail";

/// How often the device looks again, besides whenever either facet changes.
const EVERY: Duration = Duration::from_secs(30);

/// How long an invitation stays open.
const INVITATION_SECS: i64 = 600;

/// Where ErisFiles keeps its state, as it chooses it: `$ERISFILES_HOME`, else
/// `$XDG_DATA_HOME/erisfiles`, else `~/.local/share/erisfiles`.
fn erisfiles_home() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("ERISFILES_HOME").or_else(|| {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share"))).map(|d| d.join("erisfiles"))
    })
}

/// This device's ErisFiles, through its local API.
struct Files {
    url: String,
    token: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct Running {
    url: String,
    token: String,
}

impl Files {
    /// The running ErisFiles here, if there is one.
    async fn find() -> Option<(Files, String)> {
        let running: Running =
            serde_json::from_slice(&std::fs::read(erisfiles_home()?.join("daemon.json")).ok()?).ok()?;
        let files = Files { url: running.url, token: running.token, http: crate::erisdb::http() };
        let status = files.call(reqwest::Method::GET, "/api/status", None).await.ok()?;
        let id = status["id"].as_str()?.to_string();
        Some((files, id))
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self.http.request(method, format!("{}{path}", self.url)).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!("ErisFiles refused {path} ({status}): {body}");
        }
        Ok(body)
    }

    async fn peers(&self) -> Result<HashSet<String>> {
        let devices = self.call(reqwest::Method::GET, "/api/devices", None).await?;
        Ok(devices
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["this"] != true)
            .filter_map(|d| d["id"].as_str().map(str::to_string))
            .collect())
    }
}

/// The installation a token names. The core authenticates it; this reads it
/// only to know which records and invitations are this device's own.
fn installation(token: &str) -> Option<Uuid> {
    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.split('.').nth(1)?).ok()?;
    serde_json::from_slice::<Value>(&payload).ok()?["client"].as_str()?.parse().ok()
}

/// An announced device whose record its own installation wrote.
#[derive(Debug, Clone)]
struct Announced {
    item: Uuid,
    revision: i64,
    installation: Uuid,
    name: String,
    erisfiles: Option<String>,
}

pub struct Peering {
    db: Arc<erisdb_client::Client>,
    joined: Joined,
    me: Uuid,
    /// ErisFiles ids this device has an invitation open for.
    inviting: std::sync::Mutex<HashSet<String>>,
    /// Whether both facets are known to be registered, which is asked once.
    registered: std::sync::atomic::AtomicBool,
}

impl Peering {
    pub fn new(db: Arc<erisdb_client::Client>, joined: Joined) -> Option<Arc<Self>> {
        let me = installation(&joined.token)?;
        Some(Arc::new(Peering {
            db,
            joined,
            me,
            inviting: std::sync::Mutex::new(HashSet::new()),
            registered: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    /// Keep this device's announcement current and its peers paired, until
    /// the process ends. Failures are logged and tried again.
    pub async fn run(self: Arc<Self>) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
        for facet in [DEVICES, MAIL] {
            let (db, tx) = (self.db.clone(), tx.clone());
            tokio::spawn(async move { watch(db, facet, tx).await });
        }
        let mut quiet = false;
        loop {
            match self.round().await {
                Ok(()) => quiet = false,
                Err(e) if !quiet => {
                    eprintln!("erisauth: pairing ErisFiles with other devices: {e:#}");
                    quiet = true;
                }
                Err(_) => {}
            }
            let _ = tokio::time::timeout(EVERY, rx.recv()).await;
            // A burst of changes is one more round.
            tokio::time::sleep(Duration::from_millis(200)).await;
            while rx.try_recv().is_ok() {}
        }
    }

    async fn round(self: &Arc<Self>) -> Result<()> {
        if !self.registered.load(std::sync::atomic::Ordering::Relaxed) {
            register(&self.db).await?;
            self.registered.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let files = Files::find().await;
        let mine = files.as_ref().map(|(_, id)| id.clone());
        let devices = self.announced().await?;
        self.announce(&devices, mine.as_deref()).await?;
        let Some((files, mine)) = files else { return Ok(()) };
        let peers = files.peers().await?;
        self.answer_mail(&files, &devices).await?;
        let files = Arc::new(files);
        for device in devices.iter().filter(|d| d.installation != self.me) {
            let Some(theirs) = &device.erisfiles else { continue };
            if *theirs == mine || peers.contains(theirs) || mine.as_str() > theirs.as_str() {
                continue;
            }
            if !self.inviting.lock().unwrap_or_else(|e| e.into_inner()).insert(theirs.clone()) {
                continue;
            }
            let (this, files, device) = (self.clone(), files.clone(), device.clone());
            tokio::spawn(async move {
                if let Err(e) = this.invite(&files, &device).await {
                    eprintln!("erisauth: inviting {} to ErisFiles: {e:#}", device.name);
                }
                let theirs = device.erisfiles.unwrap_or_default();
                this.inviting.lock().unwrap_or_else(|e| e.into_inner()).remove(&theirs);
            });
        }
        Ok(())
    }

    async fn list(&self, facet: &str) -> Result<Vec<Value>> {
        let (status, body) = self.db.request("GET", &format!("/v1/items?facet={facet}&limit=1000"), None).await?;
        if status != 200 {
            bail!("reading {facet} ({status}): {body}");
        }
        Ok(body["items"].as_array().cloned().unwrap_or_default())
    }

    /// Every device announced by its own installation.
    async fn announced(&self) -> Result<Vec<Announced>> {
        Ok(self
            .list(DEVICES)
            .await?
            .into_iter()
            .filter_map(|item| {
                let installation: Uuid = item["body"]["installation"].as_str()?.parse().ok()?;
                let author: Uuid = item["source"]["installation"].as_str()?.parse().ok()?;
                if installation != author {
                    return None;
                }
                Some(Announced {
                    item: item["id"].as_str()?.parse().ok()?,
                    revision: item["revision"].as_i64()?,
                    installation,
                    name: item["body"]["name"].as_str().unwrap_or("a device").to_string(),
                    erisfiles: item["body"]["erisfiles"].as_str().map(str::to_string),
                })
            })
            .collect())
    }

    /// Write or correct this device's own announcement.
    async fn announce(&self, devices: &[Announced], erisfiles: Option<&str>) -> Result<()> {
        let body = json!({"installation": self.me, "name": self.joined.name, "erisfiles": erisfiles});
        let (status, answer) = match devices.iter().find(|d| d.installation == self.me) {
            Some(d) if d.name == self.joined.name && d.erisfiles.as_deref() == erisfiles => return Ok(()),
            Some(d) => {
                self.db
                    .request(
                        "PUT",
                        &format!("/v1/items/{}", d.item),
                        Some(json!({"body": body, "revision": d.revision})),
                    )
                    .await?
            }
            None => self.db.request("POST", "/v1/items", Some(json!({"facet": DEVICES, "body": body}))).await?,
        };
        if !(200..300).contains(&status) {
            bail!("announcing this device ({status}): {answer}");
        }
        Ok(())
    }

    /// Join the invitations addressed here that pass every check; drop the rest.
    async fn answer_mail(&self, files: &Files, devices: &[Announced]) -> Result<()> {
        for letter in self.list(MAIL).await? {
            let body = &letter["body"];
            if body["to"].as_str().and_then(|to| to.parse::<Uuid>().ok()) != Some(self.me) {
                continue;
            }
            let Some(id) = letter["id"].as_str() else { continue };
            match self.genuine(&letter, devices) {
                Ok(ticket) => {
                    if let Err(e) =
                        files.call(reqwest::Method::POST, "/api/join", Some(json!({"ticket": ticket}))).await
                    {
                        eprintln!("erisauth: joining an ErisFiles invitation: {e:#}");
                    }
                }
                Err(why) => eprintln!("erisauth: dropping an ErisFiles invitation: {why:#}"),
            }
            let _ = self.db.request("DELETE", &format!("/v1/items/{id}"), None).await;
        }
        Ok(())
    }

    /// The ticket in `letter`, if it comes from an announced ErisAuth device,
    /// written by that device, for the ErisFiles that device announced.
    fn genuine(&self, letter: &Value, devices: &[Announced]) -> Result<String> {
        let body = &letter["body"];
        let from: Uuid = body["from"].as_str().and_then(|f| f.parse().ok()).context("no sender")?;
        let author: Option<Uuid> = letter["source"]["installation"].as_str().and_then(|a| a.parse().ok());
        if author != Some(from) {
            bail!("it claims to be from {from} but {author:?} wrote it");
        }
        let sender = devices.iter().find(|d| d.installation == from).context("its sender is no announced device")?;
        let ticket = body["ticket"].as_str().context("no ticket")?;
        let named = erislogin::ticket::Ticket::parse("erisfiles", ticket)?.eid;
        if named.is_none() || named != sender.erisfiles {
            bail!("its ticket is not for the ErisFiles {} announced", sender.name);
        }
        Ok(ticket.to_string())
    }

    /// Open a pairing on this device's ErisFiles for `device`, post it the
    /// ticket, and approve only `device`'s announced ErisFiles key.
    async fn invite(&self, files: &Files, device: &Announced) -> Result<()> {
        let theirs = device.erisfiles.clone().context("that device has no ErisFiles")?;
        let opened =
            files.call(reqwest::Method::POST, "/api/pairings", Some(json!({"ttl_secs": INVITATION_SECS}))).await?;
        let pairing = opened["id"].as_str().context("ErisFiles sent no pairing id")?.to_string();
        let letter =
            json!({"to": device.installation, "from": self.me, "kind": "erisfiles", "ticket": opened["ticket"]});
        let (status, posted) =
            self.db.request("POST", "/v1/items", Some(json!({"facet": MAIL, "body": letter}))).await?;
        if status != 201 {
            bail!("posting the invitation ({status}): {posted}");
        }
        let deadline = erislogin::now() + INVITATION_SECS;
        let outcome = loop {
            if erislogin::now() >= deadline {
                break Ok(());
            }
            let session = files.call(reqwest::Method::GET, &format!("/api/pairings/{pairing}"), None).await?;
            match session["status"].as_str() {
                Some("requested") => {
                    let expected = json!({"kind": "iroh", "key": theirs});
                    let verdict = if session["identity"] == expected { "approve" } else { "deny" };
                    files
                        .call(reqwest::Method::POST, &format!("/api/pairings/{pairing}/{verdict}"), Some(json!({})))
                        .await?;
                    break if verdict == "approve" {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!("a different device than {} redeemed the invitation", device.name))
                    };
                }
                Some("pending") => tokio::time::sleep(Duration::from_millis(300)).await,
                _ => break Ok(()),
            }
        };
        // The invitee deletes what it answers; this clears what it never did.
        if let Some(id) = posted["id"].as_str() {
            let _ = self.db.request("DELETE", &format!("/v1/items/{id}"), None).await;
        }
        outcome
    }
}

/// Register both facets, unless they are.
async fn register(db: &erisdb_client::Client) -> Result<()> {
    let uuid = json!({"type": "string", "format": "uuid"});
    let schemas = [
        (
            DEVICES,
            json!({
                "type": "object",
                "required": ["installation", "name"],
                "properties": {
                    "installation": uuid,
                    "name": {"type": "string", "minLength": 1, "maxLength": 128},
                    "erisfiles": {"type": ["string", "null"], "pattern": "^[0-9a-f]{64}$"}
                },
                "additionalProperties": false
            }),
        ),
        (
            MAIL,
            json!({
                "type": "object",
                "required": ["to", "from", "kind", "ticket"],
                "properties": {
                    "to": uuid,
                    "from": uuid,
                    "kind": {"enum": ["erisfiles"]},
                    "ticket": {"type": "string", "maxLength": 4096}
                },
                "additionalProperties": false
            }),
        ),
    ];
    for (name, schema) in schemas {
        let definition =
            json!({"facet": "facet", "body": {"name": name, "version": 1, "strict": true, "schema": schema}});
        let (status, body) = db.request("POST", "/v1/items", Some(definition)).await?;
        if !matches!(status, 201 | 409) {
            bail!("registering {name} ({status}): {body}");
        }
    }
    Ok(())
}

/// Wake the round whenever `facet` changes; resubscribe when the stream ends.
async fn watch(db: Arc<erisdb_client::Client>, facet: &'static str, wake: tokio::sync::mpsc::Sender<()>) {
    let mut since = 0;
    loop {
        if let Ok(mut changes) = db.subscribe_changes(since, Some(facet)).await {
            while let Ok(Some(change)) = changes.next().await {
                since = since.max(change.seq);
                let _ = wake.try_send(());
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
