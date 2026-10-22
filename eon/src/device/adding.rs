//! Adding a device the other way round: the device not in yet shows a ticket of its own, and a
//! device already in scans it.
//!
//! A device in no mesh shows a join ticket on its page or at its terminal
//! ([`Daemon::show_join`]), a QR code for a phone's camera. The phone, already among the person's
//! devices, reads it and asks its person "add laptop?"; on a yes it offers the laptop the mesh
//! ([`Daemon::invite`]). The laptop's screen names the device offering and every device it would
//! join, and its person accepts there ([`Daemon::accept_offer`]). The phone then adds the laptop
//! and hands it the mesh.

use std::sync::Arc;
use std::time::Duration;

use erisoverlaynetwork::Error;
use erisoverlaynetwork::devices::Op;
use erisoverlaynetwork::join::{self, Status};
use erisoverlaynetwork::ticket::{Kind, Ticket};
use serde::Serialize;
use serde_json::{Value, json};

use super::agent::failed;
use super::changes::refused;
use super::{Daemon, TICKET_SECS};

/// How often handing a newcomer the mesh is tried before the member gives up.
const WELCOME_TRIES: u32 = 3;

/// The ticket this device shows to be added.
pub struct Shown {
    pub name: String,
    pub ticket: String,
    /// Its QR code, as [`super::qr`] draws it.
    pub drawn: Value,
}

/// A device this one is adding by its ticket, as the person's screens show it.
#[derive(Debug, Clone, Serialize)]
pub struct Inviting {
    /// The new device's name for itself.
    pub name: String,
    pub status: InviteStatus,
    /// Why it failed, once it has.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteStatus {
    /// Waiting for the new device's person to accept.
    Waiting,
    Added,
    /// The new device's person turned it down.
    Declined,
    Failed,
}

fn ok() -> Value {
    json!({"ok": true})
}

impl Daemon {
    /// Show a ticket to be added under `name`: `{name, ticket, svg, qr, expires, status}`, as
    /// [`Daemon::offer`] gives it.
    pub async fn show_join(&self, name: &str) -> Value {
        if self.node.mesh().is_some() {
            return failed("in_a_mesh", "this device is in a mesh already");
        }
        let name = match erisoverlaynetwork::check_name(name) {
            Ok(name) => name,
            Err(e) => return refused(&e),
        };
        let code = self.offers.open(TICKET_SECS);
        let addr = erisoverlaynetwork::net::advertised(self.node.endpoint(), &self.network).await;
        let encoded = Ticket::join(code, self.id(), name.clone()).map(|t| t.at(&addr)).and_then(|t| t.encode());
        let (ticket, drawn) = match encoded.and_then(|ticket| super::qr(&ticket).map(|drawn| (ticket, drawn))) {
            Ok(shown) => shown,
            Err(e) => return failed("internal", e.to_string()),
        };
        *self.shown.lock().unwrap_or_else(|e| e.into_inner()) = Some(Shown { name, ticket, drawn });
        self.changed();
        self.offer()
    }

    /// The ticket this device shows to be added, and what is offered on it: `{name, ticket, svg,
    /// qr, expires, status}`, where `status` is `showing`, `offered` or `accepted` (with `by`, the
    /// device offering, and `devices`, every device it would join), or `joined` (with `by`). Null
    /// while it shows none, and once it is in a mesh.
    pub fn offer(&self) -> Value {
        if self.is_member() {
            return Value::Null;
        }
        let shown = self.shown.lock().unwrap_or_else(|e| e.into_inner());
        let (Some(shown), Some((status, expires))) = (shown.as_ref(), self.offers.status()) else {
            return Value::Null;
        };
        let mut offer = json!(status);
        offer["name"] = json!(shown.name);
        offer["ticket"] = json!(shown.ticket);
        offer["svg"] = shown.drawn["svg"].clone();
        offer["qr"] = shown.drawn["qr"].clone();
        offer["expires"] = json!(expires);
        offer
    }

    /// The person accepts the offer made on this device's ticket.
    pub fn accept_offer(&self) -> Value {
        self.offers.accept().map_or_else(|e| refused(&e), |()| ok())
    }

    /// The person turns down the offer made on this device's ticket; the ticket stays shown.
    pub fn decline_offer(&self) -> Value {
        self.offers.decline().map_or_else(|e| refused(&e), |()| ok())
    }

    /// Stop showing a ticket to be added.
    pub fn close_join(&self) -> Value {
        self.offers.close();
        *self.shown.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.changed();
        ok()
    }

    /// The device this one is adding by its ticket, while there is one.
    pub fn inviting(&self) -> Option<Inviting> {
        self.inviting.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_inviting(&self, inviting: Option<Inviting>) {
        *self.inviting.lock().unwrap_or_else(|e| e.into_inner()) = inviting;
        self.changed();
    }

    /// Add the device showing the join ticket `ticket`, which the person here chose to add: offer
    /// it the mesh at once, and once its person accepts, add it and hand it the mesh.
    /// [`Daemon::inviting`] follows it.
    pub async fn invite(self: &Arc<Self>, ticket: &str) -> Value {
        if !self.is_member() {
            return failed("not_in_a_mesh", "this device is in no mesh; start one or join one first");
        }
        let ticket = match Ticket::parse(ticket) {
            Ok(ticket) if ticket.kind == Kind::Join => ticket,
            Ok(_) => {
                return failed(
                    "bad_request",
                    "that ticket lets others into another device's mesh; join with it instead",
                );
            }
            Err(e) => return refused(&e),
        };
        let known = self.node.with_list(|list| {
            list.is_some_and(|l| l.is_member(&ticket.eid) || l.state().removed.contains(&ticket.eid))
        });
        if known {
            return failed("conflict", "that device is, or was, in this mesh");
        }
        if self.inviting().is_some_and(|i| i.status == InviteStatus::Waiting) {
            return failed("busy", "this device is adding another already");
        }
        let Some(snapshot) = self.node.snapshot().await else { return failed("internal", "the mesh went away") };
        let offering = match join::offer(self.node.endpoint(), &ticket, snapshot).await {
            Ok(offering) => offering,
            Err(e) => return refused(&e),
        };
        let name = offering.name().to_string();
        self.set_inviting(Some(Inviting { name: name.clone(), status: InviteStatus::Waiting, error: None }));
        let daemon = self.clone();
        tokio::spawn(async move {
            let outcome = daemon.add_offered(&offering).await;
            let (status, error) = match outcome {
                Ok(true) => (InviteStatus::Added, None),
                Ok(false) => (InviteStatus::Declined, None),
                Err(why) => (InviteStatus::Failed, Some(why)),
            };
            daemon.set_inviting(Some(Inviting { name: offering.name().to_string(), status, error }));
        });
        json!({"ok": true, "name": name})
    }

    /// Wait for the newcomer's person; once they accept, add the newcomer and hand it the mesh.
    /// Whether they accepted.
    async fn add_offered(&self, offering: &join::Offering) -> Result<bool, String> {
        let name = offering.name();
        match offering.wait(Duration::from_secs(TICKET_SECS as u64)).await {
            Ok(true) => {}
            Ok(false) => return Ok(false),
            Err(Error::Expired) => return Err(format!("nobody accepted on {name} in time")),
            Err(e) => return Err(e.to_string()),
        }
        self.node
            .append(Op::Add { device: offering.newcomer(), name: name.to_string() })
            .await
            .map_err(|e| e.to_string())?;
        let mut tries = 0;
        loop {
            let snapshot = self.node.snapshot().await.ok_or("the mesh went away")?;
            match offering.welcome(snapshot).await {
                Ok(()) => return Ok(true),
                Err(e) if tries + 1 < WELCOME_TRIES => {
                    tries += 1;
                    eprintln!("eon: handing {name} the mesh: {e}; trying again");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(format!("{name} was added, but could not be handed the mesh: {e}")),
            }
        }
    }

    /// Follow the offers made on this device's ticket, for as long as the daemon runs: the
    /// person's screens show each, and hear once this device is in.
    pub(crate) async fn follow_offers(self: Arc<Self>) {
        let mut changes = self.offers.subscribe();
        let mut told = false;
        let mut shown = Value::Null;
        loop {
            if let Some((Status::Joined { by }, _)) = self.offers.status()
                && !told
            {
                told = true;
                self.tell(format!("{by} added this device"));
            }
            let now = self.offer();
            if now != shown {
                shown = now;
                self.changed();
            }
            tokio::select! {
                changed = changes.changed() => if changed.is_err() { return },
                // Tickets end at their expiry without a word.
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    }
}
