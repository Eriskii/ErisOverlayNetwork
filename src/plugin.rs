//! The plugin's side of ErisDB's one-shot process protocol: one invocation
//! on stdin, one answer on stdout. ErisDB has already checked the caller's
//! permission for the operation and the input against its schema.

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::erisdb::Session;
use crate::home::{self, Guess};

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

/// Answer one invocation, with home's state in `dir`.
pub async fn answer(dir: &Path, invocation: Invocation) -> Result<Answer> {
    if invocation.protocol != 1 {
        return Ok(refuse(400, "protocol", "this plugin speaks protocol 1"));
    }
    let Some(installation) = invocation.context.installation else {
        return Ok(refuse(403, "not_a_device", "only a registered ErisAuth device can ask home"));
    };
    let mut db = Session::open(dir)?;
    match invocation.operation.as_str() {
        "approve" => approve(&mut db, installation, serde_json::from_value(invocation.input)?).await,
        "passphrase" => change(&mut db, installation, serde_json::from_value(invocation.input)?).await,
        other => Ok(refuse(404, "unknown_operation", &format!("home has no operation {other:?}"))),
    }
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
    }
}

async fn approve(db: &mut Session, installation: Uuid, ask: Approve) -> Result<Answer> {
    if let Some(refusal) = guessed(home::guess(db, installation, &ask.passphrase).await?) {
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

async fn change(db: &mut Session, installation: Uuid, ask: Change) -> Result<Answer> {
    if let Some(refusal) = guessed(home::guess(db, installation, &ask.current).await?) {
        return Ok(refusal);
    }
    home::set_passphrase(db, &ask.new).await?;
    Ok((200, json!({"changed": true})))
}
