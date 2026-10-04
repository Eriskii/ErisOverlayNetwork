//! What the person's tools ask the device, one JSON command and one answer
//! each: the terminal over the socket, and the Android app's own screens.
//!
//! ```text
//! {"op":"state"}                                      → {device, approvals, joining}
//! {"op":"approvals"}                                  → {"approvals":[…]}
//! {"op":"decide","id":…,"granted":[…],"passphrase":…} → {"ok":true} or {"error":…}
//! {"op":"deny","id":…}                                → {"ok":true} or {"error":…}
//! {"op":"add"}                                        → {"id","ticket","svg","expires"}: a new device joins with it
//! {"op":"join","ticket":…,"name":…,"ceiling":[…]}     → {"ok":true}; `joining` in the state follows it
//! {"op":"passphrase","current":…,"new":…}             → {"ok":true} or {"error":…}; without `current`, a reset
//! {"op":"unlock","installation":…}                    → {"ok":true} or {"error":…}; every device without one
//! ```

use std::sync::Arc;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use super::agent::Agent;

#[derive(Deserialize)]
struct Decide {
    id: Uuid,
    granted: Vec<String>,
    #[serde(default)]
    passphrase: Option<String>,
}

#[derive(Deserialize)]
struct Deny {
    id: Uuid,
}

#[derive(Deserialize)]
struct Join {
    ticket: String,
    name: String,
    #[serde(default)]
    ceiling: Vec<String>,
}

#[derive(Deserialize)]
struct Passphrase {
    #[serde(default)]
    current: Option<String>,
    new: String,
}

#[derive(Deserialize)]
struct Unlock {
    #[serde(default)]
    installation: Option<Uuid>,
}

fn read<T: DeserializeOwned>(message: Value) -> Result<T, Value> {
    serde_json::from_value(message).map_err(|e| json!({"error": "bad_request", "message": e.to_string()}))
}

/// The device as its screens show it: whether and how it has joined, what
/// waits for the person, and a join in progress.
pub async fn state(agent: &Agent) -> Value {
    json!({"device": agent.device().await, "approvals": agent.approvals(), "joining": agent.joining()})
}

/// Answer one command.
pub async fn run(agent: &Arc<Agent>, message: Value) -> Value {
    let op = message["op"].as_str().unwrap_or_default().to_string();
    let answer = match op.as_str() {
        "state" => Ok(state(agent).await),
        "approvals" => Ok(json!({"approvals": agent.approvals()})),
        "decide" => match read::<Decide>(message) {
            Ok(d) => Ok(agent.approve(d.id, d.granted, d.passphrase).await),
            Err(refusal) => Err(refusal),
        },
        "deny" => read::<Deny>(message).map(|d| agent.deny(d.id)),
        "add" => Ok(agent.add().await),
        "join" => match read::<Join>(message) {
            Ok(j) => Ok(agent.join(j.ticket, j.name, j.ceiling).await),
            Err(refusal) => Err(refusal),
        },
        "passphrase" => match read::<Passphrase>(message) {
            Ok(p) => Ok(agent.passphrase(p.current.filter(|c| !c.is_empty()), p.new).await),
            Err(refusal) => Err(refusal),
        },
        "unlock" => match read::<Unlock>(message) {
            Ok(u) => Ok(agent.unlock(u.installation).await),
            Err(refusal) => Err(refusal),
        },
        other => Err(json!({"error": "bad_request", "message": format!("no operation {other:?}")})),
    };
    answer.unwrap_or_else(|refusal| refusal)
}
