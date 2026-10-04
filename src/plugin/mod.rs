//! The plugin's side of ErisDB's one-shot process protocol: one invocation
//! on stdin, one answer on stdout. ErisDB has already checked the caller's
//! permission for the operation and the input against its schema, and hands
//! the plugin, with each call, a capability for the grants its manifest names.

use anyhow::Result;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

pub mod db;
pub mod state;
use self::db::{Access, Db};
use self::state::Guess;

#[derive(Debug, Deserialize)]
pub struct Invocation {
    pub protocol: u32,
    pub operation: String,
    pub input: Value,
    #[serde(default)]
    pub context: Context,
}

#[derive(Debug, Default, Deserialize)]
pub struct Context {
    /// The registered installation that called, as ErisDB authenticated it.
    pub installation: Option<Uuid>,
    /// The core, and a capability for this call.
    pub erisdb: Option<Access>,
}

/// An answer: an HTTP status and a JSON body.
pub type Answer = (u16, Value);

fn refuse(status: u16, error: &str, message: &str) -> Answer {
    (status, json!({"error": error, "message": message}))
}

#[derive(Deserialize)]
struct Approve {
    pairing: Uuid,
    granted: Vec<String>,
    identity: Value,
    passphrase: String,
}

#[derive(Deserialize)]
struct Change {
    current: String,
    new: String,
}

#[derive(Deserialize)]
struct Reset {
    new: String,
}

#[derive(Deserialize)]
struct Unlock {
    installation: Option<Uuid>,
}

/// Answer one invocation.
pub async fn answer(invocation: Invocation) -> Result<Answer> {
    if invocation.protocol != 1 {
        return Ok(refuse(400, "protocol", "this plugin speaks protocol 1"));
    }
    let Some(access) = invocation.context.erisdb else {
        return Ok(refuse(
            503,
            "no_grants",
            "ErisDB handed ErisAuth no capability; its manifest names the grants it acts with",
        ));
    };
    let db = Db::new(access);
    let installation = invocation.context.installation;
    let input = invocation.input;
    match invocation.operation.as_str() {
        "approve" => match (installation, read::<Approve>(input)) {
            (None, _) => Ok(not_a_device()),
            (_, Err(refusal)) => Ok(refusal),
            (Some(installation), Ok(ask)) => approve(&db, installation, ask).await,
        },
        "passphrase" => match (installation, read::<Change>(input)) {
            (None, _) => Ok(not_a_device()),
            (_, Err(refusal)) => Ok(refusal),
            (Some(installation), Ok(ask)) => change(&db, installation, ask).await,
        },
        "reset" => match read::<Reset>(input) {
            Err(refusal) => Ok(refusal),
            Ok(ask) => {
                state::set_passphrase(&db, &ask.new, true).await?;
                Ok((200, json!({"reset": true})))
            }
        },
        "unlock" => match read::<Unlock>(input) {
            Err(refusal) => Ok(refusal),
            Ok(ask) => {
                state::unlock(&db, ask.installation).await?;
                Ok((200, json!({"unlocked": true})))
            }
        },
        other => Ok(refuse(404, "unknown_operation", &format!("ErisAuth has no operation {other:?}"))),
    }
}

fn read<T: DeserializeOwned>(input: Value) -> Result<T, Answer> {
    serde_json::from_value(input).map_err(|e| refuse(400, "bad_input", &e.to_string()))
}

fn not_a_device() -> Answer {
    refuse(403, "not_a_device", "only a registered ErisAuth device guesses the passphrase")
}

/// A refusal for a guess that was not right, or `None` to go on.
fn guessed(guess: Guess) -> Option<Answer> {
    match guess {
        Guess::Right => None,
        Guess::Wrong => Some(refuse(403, "wrong_passphrase", "that is not the passphrase")),
        Guess::Locked(wait) => Some((
            429,
            json!({"error": "locked", "message": "too many wrong passphrases from this device", "retry_after": wait}),
        )),
        Guess::Unset => Some(refuse(
            503,
            "no_passphrase",
            "ErisAuth has no passphrase yet; set one from a device that holds erisauth:reset",
        )),
    }
}

async fn approve(db: &Db, installation: Uuid, ask: Approve) -> Result<Answer> {
    if let Some(refusal) = guessed(state::guess(db, installation, &ask.passphrase).await?) {
        return Ok(refusal);
    }
    let (status, session) = db.get(&format!("/v1/pairings/{}", ask.pairing)).await?;
    if status == 404 {
        return Ok(refuse(404, "no_such_pairing", "ErisDB has no such pairing"));
    }
    if status != 200 {
        return Ok((status, session));
    }
    if session["body"]["status"] != "requested" {
        return Ok(refuse(409, "not_waiting", "that pairing is not waiting for an answer"));
    }
    // The device vouched for one app's key. Whoever redeemed this pairing
    // must be that app, or the person approved something else.
    if session["body"]["identity"] != ask.identity {
        return Ok(refuse(
            409,
            "not_the_app",
            "a different key redeemed that pairing than the one the device vouched for",
        ));
    }
    let (status, body) =
        db.post(&format!("/v1/pairings/{}/approve", ask.pairing), json!({"granted": ask.granted})).await?;
    if status != 200 {
        return Ok((status, body));
    }
    Ok((200, json!({"approved": ask.granted})))
}

async fn change(db: &Db, installation: Uuid, ask: Change) -> Result<Answer> {
    if let Some(refusal) = guessed(state::guess(db, installation, &ask.current).await?) {
        return Ok(refusal);
    }
    state::set_passphrase(db, &ask.new, false).await?;
    Ok((200, json!({"changed": true})))
}
