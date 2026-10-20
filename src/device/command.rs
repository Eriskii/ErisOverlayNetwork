//! What the person's tools ask the daemon: one JSON command, one JSON answer. The terminal and
//! the tests send these on the agent socket, the approval page posts them to `/api/command`,
//! and the Android app's screens pass them in through JNI.
//!
//! ```text
//! {"op":"state"}                                     → the state, below
//! {"op":"wait","after":N,"within_ms":M}              → {"version"} once the state's version passes N, or M ms pass
//! {"op":"init","name"}                               → {"ok","mesh"}: a new mesh, with this device in it
//! {"op":"join","ticket","name"}                      → {"ok","fingerprint","host"}; `joining` follows it
//! {"op":"add"}                                       → {"id","purpose","ticket","svg","qr":{"side","modules"},"expires"}
//! {"op":"add_app"}                                   → the same, for an app with no ErisAuth beside it
//! {"op":"close","id"}                                → {"ok"}: a ticket nobody redeemed stops working
//! {"op":"decide","id","grants"?,"passphrase"?}       → {"ok"}: approve a request, its grants as asked without "grants"
//! {"op":"deny","id"}                                 → {"ok"}
//! {"op":"rename","device","name"}                    → {"ok"}
//! {"op":"removal","device"}                          → {"device":{"id","name"},"also":[{"id","name"}]}: the devices it added
//! {"op":"remove","devices":[…]}                      → {"ok"}: one removal of them all
//! {"op":"revoke","note"}                             → {"ok"}
//! {"op":"passphrase","current","new"}                → {"ok"}: ErisDB's passphrase, changed from this device
//! ```
//!
//! Anything refused answers `{"error": code, "message": why}`, and a lockout adds `retry_after`
//! in seconds. The state:
//!
//! ```text
//! {"version", "device": {"id","name"},
//!  "mesh": null | {"id","member","devices":[{"id","name","added_by","added_by_name","added_at","this","reachable"}],
//!                  "erisdb": null | {"app","note","name","device","approved_at","reachable"}},
//!  "joining": null | {"fingerprint","host","error"},
//!  "approvals": [{"id","kind","via","name","key","service","grants","serves","program","pid","package",
//!                 "certificate","fingerprint","passphrase","becomes_erisdb","expires","busy"}],
//!  "tickets": [{"id","purpose","status","expires"}],
//!  "notes": [{"id","app","name","service","grants","serves","program","at","revoked","counts","erisdb"}],
//!  "events": [{"seq","at","text"}]}
//! ```

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use super::agent::failed;
use super::{Daemon, Purpose};

#[derive(Deserialize)]
struct Wait {
    #[serde(default)]
    after: u64,
    #[serde(default = "a_minute")]
    within_ms: u64,
}

fn a_minute() -> u64 {
    60_000
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Join {
    ticket: String,
    name: String,
}

#[derive(Deserialize)]
struct Id {
    id: Uuid,
}

#[derive(Deserialize)]
struct Decide {
    id: Uuid,
    #[serde(default)]
    grants: Option<Vec<String>>,
    #[serde(default)]
    passphrase: Option<String>,
}

#[derive(Deserialize)]
struct Rename {
    device: String,
    name: String,
}

#[derive(Deserialize)]
struct Device {
    device: String,
}

#[derive(Deserialize)]
struct Remove {
    devices: Vec<String>,
}

#[derive(Deserialize)]
struct Revoke {
    note: String,
}

#[derive(Deserialize)]
struct Passphrase {
    current: String,
    new: String,
}

fn read<T: DeserializeOwned>(message: Value) -> Result<T, Value> {
    serde_json::from_value(message).map_err(|e| failed("bad_request", e.to_string()))
}

/// Everything the person's screens show, in one answer.
pub fn state(daemon: &Daemon) -> Value {
    let list = daemon.node.list();
    json!({
        "version": daemon.version(),
        "device": {"id": daemon.id(), "name": daemon.name()},
        "mesh": list.map(|list| json!({
            "id": list.mesh(),
            "member": list.is_member(&daemon.id()),
            "devices": daemon.devices(),
            "erisdb": daemon.erisdb(),
        })),
        "joining": daemon.joining(),
        "approvals": daemon.prompts(),
        "tickets": daemon.tickets(),
        "notes": daemon.notes(),
        "events": daemon.events(),
    })
}

/// Answer one command.
pub async fn run(daemon: &Arc<Daemon>, message: Value) -> Value {
    let op = message["op"].as_str().unwrap_or_default().to_string();
    let outcome: Result<Value, Value> = match op.as_str() {
        "state" => Ok(state(daemon)),
        "wait" => match read::<Wait>(message) {
            Ok(wait) => Ok(wait_for(daemon, wait.after, Duration::from_millis(wait.within_ms.min(300_000))).await),
            Err(refusal) => Err(refusal),
        },
        "init" => match read::<Named>(message) {
            Ok(named) => Ok(daemon.init(&named.name).await),
            Err(refusal) => Err(refusal),
        },
        "join" => match read::<Join>(message) {
            Ok(join) => Ok(daemon.join(&join.ticket, &join.name).await),
            Err(refusal) => Err(refusal),
        },
        "add" => Ok(daemon.open_ticket(Purpose::Device).await),
        "add_app" => Ok(daemon.open_ticket(Purpose::App).await),
        "close" => read::<Id>(message).map(|id| daemon.close_ticket(id.id)),
        "decide" => match read::<Decide>(message) {
            Ok(decide) => Ok(daemon.decide(decide.id, decide.grants, decide.passphrase).await),
            Err(refusal) => Err(refusal),
        },
        "deny" => read::<Id>(message).map(|id| daemon.deny(id.id)),
        "rename" => match read::<Rename>(message) {
            Ok(rename) => Ok(daemon.rename(&rename.device, &rename.name).await),
            Err(refusal) => Err(refusal),
        },
        "removal" => read::<Device>(message).map(|device| daemon.removal(&device.device)),
        "remove" => match read::<Remove>(message) {
            Ok(remove) => Ok(daemon.remove(&remove.devices).await),
            Err(refusal) => Err(refusal),
        },
        "revoke" => match read::<Revoke>(message) {
            Ok(revoke) => Ok(daemon.revoke(&revoke.note).await),
            Err(refusal) => Err(refusal),
        },
        "passphrase" => match read::<Passphrase>(message) {
            Ok(passphrase) => Ok(daemon.change_passphrase(&passphrase.current, &passphrase.new).await),
            Err(refusal) => Err(refusal),
        },
        other => Err(failed("bad_request", format!("no command {other:?}"))),
    };
    outcome.unwrap_or_else(|refusal| refusal)
}

/// `{"version"}` once the state's version passes `after`, or once `within` passes.
async fn wait_for(daemon: &Daemon, after: u64, within: Duration) -> Value {
    let mut changes = daemon.subscribe();
    let _ = tokio::time::timeout(within, async {
        while *changes.borrow_and_update() <= after {
            if changes.changed().await.is_err() {
                return;
            }
        }
    })
    .await;
    json!({"version": daemon.version()})
}
