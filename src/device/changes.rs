//! Changing the mesh, and hearing of the changes other devices make.
//!
//! From any device in the mesh the person renames a device, removes devices, or revokes an
//! app's note; each is one signed entry the node sends to everyone at once. Each device tells
//! its person what the others did: "laptop added phone", "laptop removed phone, tablet",
//! "server approved ErisDB as your ErisDB". It also asks the other devices, and the mesh's
//! ErisDB, now and then whether they answer, so the person sees which are reachable.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use erisoverlaynetwork::ceremony::{self, Approval};
use erisoverlaynetwork::devices::{DeviceList, Entry, Op};
use erisoverlaynetwork::{DeviceId, EntryId, Error, NoteId};
use serde_json::{Value, json};

use super::agent::{ERISDB, failed};
use super::{Daemon, Joining, TICKET_SECS};

/// How often a device that answered is asked again.
const REACHABLE: Duration = Duration::from_secs(30);

/// How long a device has to answer.
const ANSWER: Duration = Duration::from_secs(5);

/// What the library refused, as the person's screens show it.
pub(crate) fn refused(e: &Error) -> Value {
    let code = match e {
        Error::BadRequest(_) => "bad_request",
        Error::Conflict(_) => "conflict",
        Error::Forbidden { .. } | Error::Unauthorized => "forbidden",
        Error::Unreachable(_) => "unreachable",
        Error::Expired => "expired",
        Error::Cancelled(_) => "cancelled",
        Error::Io(_) | Error::Internal(_) => "internal",
    };
    failed(code, e.to_string())
}

fn ok() -> Value {
    json!({"ok": true})
}

fn device_id(id: &str) -> Result<DeviceId, Value> {
    id.parse().map_err(|_| failed("bad_request", format!("{id:?} is not a device's id")))
}

/// Every device the entries ever named, by its latest name, the removed ones included.
pub(crate) struct Names(HashMap<DeviceId, String>);

impl Names {
    pub(crate) fn of(list: &DeviceList) -> Self {
        let mut names = HashMap::new();
        for entry in list.entries().into_iter().filter_map(|signed| Entry::open(signed).ok()) {
            match entry.body.op {
                Op::Genesis { name } => {
                    names.insert(entry.author, name);
                }
                Op::Add { device, name } | Op::Name { device, name } => {
                    names.insert(device, name);
                }
                _ => {}
            }
        }
        for (device, member) in &list.state().members {
            names.insert(*device, member.name.clone());
        }
        Names(names)
    }

    pub(crate) fn name(&self, device: &DeviceId) -> String {
        self.0.get(device).cloned().unwrap_or_else(|| device.fmt_short().to_string())
    }
}

impl Daemon {
    /// Start a mesh with this device as its first member.
    pub async fn init(&self, name: &str) -> Value {
        if self.node.mesh().is_some() {
            return failed("in_a_mesh", "this device is in a mesh already");
        }
        if self.joining().is_some_and(|j| j.error.is_none()) {
            return failed("joining", "this device is joining a mesh");
        }
        match self.node.create(name).await {
            Ok(mesh) => {
                self.changed();
                json!({"ok": true, "mesh": mesh})
            }
            Err(e) => refused(&e),
        }
    }

    /// Redeem a ticket another device shows, asking to join its mesh as `name`: the code to
    /// compare at once, then [`Daemon::joining`] follows the request until the device is in or
    /// the request is refused.
    pub async fn join(self: &Arc<Self>, ticket: &str, name: &str) -> Value {
        if self.node.mesh().is_some() {
            return failed("in_a_mesh", "this device is in a mesh already");
        }
        if self.joining().is_some_and(|j| j.error.is_none()) {
            return failed("joining", "this device is joining a mesh already");
        }
        let ticket = match erisoverlaynetwork::ticket::Ticket::parse(ticket) {
            Ok(ticket) => ticket,
            Err(e) => return refused(&e),
        };
        let ask = ceremony::Ask::Device { name: name.trim().to_string() };
        let joining = match ceremony::redeem(self.node.endpoint(), &ticket, ask).await {
            Ok(joining) => joining,
            Err(e) => return refused(&e),
        };
        let shown = json!({"ok": true, "fingerprint": joining.fingerprint, "host": joining.host});
        let (fingerprint, host) = (joining.fingerprint.clone(), joining.host.clone());
        self.set_joining(Some(Joining { fingerprint: fingerprint.clone(), host: host.clone(), error: None }));
        let daemon = self.clone();
        tokio::spawn(async move {
            let outcome = match joining.wait(Duration::from_secs(TICKET_SECS as u64)).await {
                Ok(Some(Approval::Device { snapshot })) => daemon.node.adopt(snapshot).await.map_err(|e| e.to_string()),
                Ok(Some(Approval::App { .. })) => Err("the device answered as if this were an app".into()),
                Ok(None) => Err(format!("{} denied the request to join", joining.host)),
                Err(Error::Expired) => Err("nobody approved the request to join in time".into()),
                Err(e) => Err(e.to_string()),
            };
            match outcome {
                Ok(()) => daemon.set_joining(None),
                Err(why) => {
                    let mut failed = daemon.joining().unwrap_or(Joining { fingerprint, host, error: None });
                    failed.error = Some(why);
                    daemon.set_joining(Some(failed));
                }
            }
        });
        shown
    }

    /// Call `device` something new.
    pub async fn rename(&self, device: &str, name: &str) -> Value {
        let device = match device_id(device) {
            Ok(device) => device,
            Err(refusal) => return refusal,
        };
        match self.node.append(Op::Name { device, name: name.trim().to_string() }).await {
            Ok(_) => ok(),
            Err(e) => refused(&e),
        }
    }

    /// What removing `device` offers to remove with it: the devices it added.
    pub fn removal(&self, device: &str) -> Value {
        let device = match device_id(device) {
            Ok(device) => device,
            Err(refusal) => return refusal,
        };
        let Some(list) = self.node.list() else { return failed("not_in_a_mesh", "this device is in no mesh") };
        let Some(member) = list.member(&device) else {
            return failed("no_such_device", "no device in the mesh has that id");
        };
        let also: Vec<Value> = list
            .added_by(&device)
            .into_iter()
            .map(|d| json!({"id": d, "name": list.member(&d).map(|m| m.name.clone())}))
            .collect();
        json!({"device": {"id": device, "name": member.name}, "also": also})
    }

    /// Remove `devices` from the mesh, for good, in one entry.
    pub async fn remove(&self, devices: &[String]) -> Value {
        let devices: Vec<DeviceId> = match devices.iter().map(|d| device_id(d)).collect() {
            Ok(devices) => devices,
            Err(refusal) => return refusal,
        };
        let Some(list) = self.node.list() else { return failed("not_in_a_mesh", "this device is in no mesh") };
        if devices.is_empty() {
            return failed("bad_request", "name a device to remove");
        }
        if let Some(stranger) = devices.iter().find(|d| !list.is_member(d)) {
            return failed("no_such_device", format!("{} is not a device in the mesh", stranger.fmt_short()));
        }
        if list.state().members.keys().all(|member| devices.contains(member)) {
            return failed("bad_request", "a mesh keeps at least one device");
        }
        match self.node.append(Op::Remove { devices }).await {
            Ok(_) => ok(),
            Err(e) => refused(&e),
        }
    }

    /// Revoke an app's note: it stops counting everywhere.
    pub async fn revoke(&self, note: &str) -> Value {
        let note: NoteId = match note.parse() {
            Ok(note) => note,
            Err(e) => return refused(&e),
        };
        match self.node.append(Op::Revoke { note }).await {
            Ok(_) => ok(),
            Err(e) => refused(&e),
        }
    }

    /// The mesh's devices as the person's screens list them.
    pub fn devices(&self) -> Vec<Value> {
        let Some(list) = self.node.list() else { return Vec::new() };
        let names = Names::of(&list);
        let me = self.id();
        let reach = self.reach.lock().unwrap_or_else(|e| e.into_inner()).clone();
        // The founder first, then the rest in the order they were added.
        let mut devices: Vec<((bool, i64), Value)> = list
            .state()
            .members
            .iter()
            .map(|(id, member)| {
                let this = *id == me;
                (
                    (member.added_by.is_some(), member.added_at),
                    json!({
                        "id": id,
                        "name": member.name,
                        "added_by": member.added_by,
                        "added_by_name": member.added_by.map(|by| names.name(&by)),
                        "added_at": member.added_at,
                        "this": this,
                        "reachable": if this { Some(true) } else { reach.get(id).copied() },
                    }),
                )
            })
            .collect();
        devices.sort_by_key(|(at, _)| *at);
        devices.into_iter().map(|(_, device)| device).collect()
    }

    /// The mesh's ErisDB, once this device has heard from it: its app, its name, the device that
    /// approved it, and whether it answers.
    pub fn erisdb(&self) -> Value {
        let Some(provider) = self.node.provider(ERISDB) else { return Value::Null };
        let Some(note) = provider.note() else { return Value::Null };
        let device = self.node.with_list(|list| list.map(|list| Names::of(list).name(&note.body.device)));
        json!({
            "app": provider.id,
            "note": note.id,
            "name": note.body.name,
            "device": device,
            "approved_at": note.body.at,
            "reachable": self.reach.lock().unwrap_or_else(|e| e.into_inner()).get(&provider.id).copied().unwrap_or(false),
        })
    }

    /// The notes this device issued, newest first, and whether each still counts.
    pub fn notes(&self) -> Vec<Value> {
        let erisdb = self.node.provider(ERISDB).and_then(|p| p.note().map(|n| n.id));
        self.node.with_list(|list| {
            self.issued()
                .into_iter()
                .rev()
                .map(|note| {
                    json!({
                        "id": note.id,
                        "app": note.body.app,
                        "name": note.body.name,
                        "service": note.body.service,
                        "grants": note.body.grants,
                        "serves": note.body.serves,
                        "program": note.body.program,
                        "at": note.body.at,
                        "revoked": list.is_some_and(|l| l.is_revoked(&note.id)),
                        "counts": list.is_some_and(|l| note.check(l).is_ok()),
                        "erisdb": erisdb == Some(note.id),
                    })
                })
                .collect()
        })
    }

    /// Tell the person what other devices change, for as long as the daemon runs: the devices
    /// they add and remove, and the ErisDB they approve.
    pub(crate) async fn follow_list(self: Arc<Self>) {
        let mut changes = self.node.subscribe();
        // The entries already told of, and the ErisDB: everything there is when a list first
        // appears is no news.
        let mut seen: Option<(HashSet<EntryId>, Option<DeviceId>)> = None;
        let me = self.id();
        loop {
            if let Some(list) = self.node.list() {
                let entries: Vec<Entry> = list.entries().into_iter().filter_map(|s| Entry::open(s).ok()).collect();
                let erisdb = self.node.provider(ERISDB);
                match &mut seen {
                    None => seen = Some((entries.iter().map(|e| e.id).collect(), erisdb.map(|p| p.id))),
                    Some((told, was)) => {
                        let names = Names::of(&list);
                        for entry in entries {
                            if !told.insert(entry.id) || entry.author == me {
                                continue;
                            }
                            let by = names.name(&entry.author);
                            match &entry.body.op {
                                Op::Add { device, .. } if list.is_member(device) => {
                                    self.tell(format!("{by} added {}", names.name(device)));
                                }
                                Op::Remove { devices } if devices.iter().any(|d| list.state().removed.contains(d)) => {
                                    let gone: Vec<String> = devices.iter().map(|d| names.name(d)).collect();
                                    self.tell(format!("{by} removed {}", gone.join(", ")));
                                }
                                _ => {}
                            }
                        }
                        let now = erisdb.as_ref().map(|p| p.id);
                        if now != *was {
                            *was = now;
                            if let Some(note) = erisdb.as_ref().and_then(|p| p.note())
                                && note.body.device != me
                            {
                                let by = names.name(&note.body.device);
                                self.tell(format!("{by} approved {} as your ErisDB", note.body.name));
                            }
                        }
                    }
                }
            }
            self.changed();
            if changes.changed().await.is_err() {
                return;
            }
        }
    }

    /// Ask every other device, and the mesh's ErisDB, whether they answer: every
    /// [`REACHABLE`] when they do; when they do not, sooner and sooner again, and at once when
    /// they say where they are anew.
    pub(crate) async fn probe(self: Arc<Self>) {
        let mut changes = self.node.subscribe();
        let mut asking = tokio::task::JoinSet::new();
        let mut busy: HashSet<DeviceId> = HashSet::new();
        // When each is next asked, how often in a row it did not answer, and when it last said
        // where it is, as of the last time it was asked.
        let mut next: HashMap<DeviceId, (Instant, u32, i64)> = HashMap::new();
        loop {
            let me = self.id();
            let mut targets: Vec<DeviceId> =
                self.node.members().into_iter().map(|(d, _)| d).filter(|d| *d != me).collect();
            targets.extend(self.node.provider(ERISDB).map(|p| p.id));
            let heard: HashMap<DeviceId, i64> = self.node.directory().into_iter().map(|p| (p.id, p.at)).collect();
            next.retain(|d, _| targets.contains(d));
            let forgot = {
                let mut reach = self.reach.lock().unwrap_or_else(|e| e.into_inner());
                let before = reach.len();
                reach.retain(|d, _| targets.contains(d));
                before != reach.len()
            };
            if forgot {
                self.changed();
            }
            let now = Instant::now();
            for target in targets {
                let said = heard.get(&target).copied().unwrap_or_default();
                let due =
                    next.get(&target).is_none_or(|(at, missed, then)| *at <= now || (*missed > 0 && said > *then));
                if due && busy.insert(target) {
                    let node = self.node.clone();
                    asking.spawn(async move {
                        let answered =
                            tokio::time::timeout(ANSWER, node.sync_with(target)).await.is_ok_and(|r| r.is_ok());
                        (target, answered)
                    });
                }
            }
            tokio::select! {
                Some(Ok((target, answered))) = asking.join_next() => {
                    busy.remove(&target);
                    let missed = if answered { 0 } else { next.get(&target).map_or(0, |n| n.1) + 1 };
                    let wait = if answered { REACHABLE } else { Duration::from_secs(1 << missed.min(5)).min(REACHABLE) };
                    let said = self.node.directory().into_iter().find(|p| p.id == target).map_or(0, |p| p.at);
                    next.insert(target, (Instant::now() + wait, missed, said));
                    let was = self.reach.lock().unwrap_or_else(|e| e.into_inner()).insert(target, answered);
                    if was != Some(answered) {
                        self.changed();
                    }
                }
                changed = changes.changed() => if changed.is_err() { return },
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    }
}
