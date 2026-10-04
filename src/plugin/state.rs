//! What ErisAuth's plugin keeps in ErisDB.
//!
//! The plugin acts with the grants its manifest names, `*` as shipped, so it
//! can approve anything a person asks for. It approves only for a registered
//! ErisAuth device that brings the passphrase, which is how a device with
//! little authority of its own still approves what an app needs: the device
//! is something the person has, the passphrase something they know.
//!
//! The passphrase is kept as an Argon2id hash in ErisDB's
//! `erisauth.passphrase` facet, with each device's count of wrong guesses.
//! Only the plugin and whoever holds that facet can read it. After [`FREE_ATTEMPTS`] wrong guesses a
//! device waits a minute before it may guess again, then two, then four,
//! doubling up to a day; a right guess clears its count. One device's
//! guesses never lock another out.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::db::Db;

/// The facet holding the passphrase hash and each device's lockout.
pub const FACET: &str = "erisauth.passphrase";

/// Wrong guesses a device gets before it has to wait.
pub const FREE_ATTEMPTS: u32 = 3;

/// The longest a device waits between guesses.
pub const MAX_LOCK_SECS: i64 = 86_400;

/// The shortest passphrase the plugin accepts.
pub const MIN_PASSPHRASE: usize = 8;

/// What the plugin keeps in ErisDB.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// The passphrase's Argon2id hash, in PHC form.
    pub passphrase: String,
    /// Wrong guesses by installation.
    #[serde(default)]
    pub lockouts: BTreeMap<Uuid, Lockout>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockout {
    pub failures: u32,
    /// Unix seconds until which this device may not guess.
    pub locked_until: i64,
}

impl Lockout {
    /// Seconds this device still has to wait, if any.
    pub fn wait(&self, now: i64) -> Option<i64> {
        (self.locked_until > now).then(|| self.locked_until - now)
    }

    /// Count a wrong guess made at `now`.
    pub fn fail(&mut self, now: i64) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= FREE_ATTEMPTS {
            let doublings = (self.failures - FREE_ATTEMPTS).min(20);
            self.locked_until = now + (60i64 << doublings).min(MAX_LOCK_SECS);
        }
    }
}

/// The JSON schema of [`State`], registered as the facet.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "required": ["passphrase"],
        "properties": {
            "passphrase": {"type": "string", "minLength": 1},
            "lockouts": {
                "type": "object",
                "additionalProperties": {
                    "type": "object",
                    "required": ["failures", "locked_until"],
                    "properties": {
                        "failures": {"type": "integer", "minimum": 0},
                        "locked_until": {"type": "integer"}
                    },
                    "additionalProperties": false
                }
            }
        },
        "additionalProperties": false
    })
}

pub fn hash(passphrase: &str) -> Result<String> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        bail!("a passphrase has at least {MIN_PASSPHRASE} characters");
    }
    let salt = SaltString::generate(&mut OsRng);
    Ok(argon2::Argon2::default()
        .hash_password(passphrase.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing the passphrase: {e}"))?
        .to_string())
}

pub fn verify(passphrase: &str, hash: &str) -> Result<bool> {
    let hash = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("the stored passphrase hash is damaged: {e}"))?;
    Ok(argon2::Argon2::default().verify_password(passphrase.as_bytes(), &hash).is_ok())
}

/// The state item as ErisDB holds it.
pub struct Stored {
    pub id: Uuid,
    pub revision: i64,
    pub state: State,
}

/// The plugin's state, or `None` before anyone has set a passphrase.
pub async fn load(db: &Db) -> Result<Option<Stored>> {
    let (status, body) = db.get(&format!("/v1/items?facet={FACET}&limit=1")).await?;
    if status != 200 {
        bail!("reading {FACET} ({status}): {body}");
    }
    let Some(item) = body["items"].as_array().and_then(|items| items.first()) else { return Ok(None) };
    Ok(Some(Stored {
        id: item["id"].as_str().and_then(|id| id.parse().ok()).context("an item without an id")?,
        revision: item["revision"].as_i64().context("an item without a revision")?,
        state: serde_json::from_value(item["body"].clone()).context("the passphrase facet is damaged")?,
    }))
}

/// Apply `change` to the stored state and save it, starting over from the
/// current state when another writer got there first. Returns what `change`
/// returned on the attempt that was saved.
pub async fn update<T>(db: &Db, mut change: impl FnMut(&mut State) -> T) -> Result<T> {
    for _ in 0..16 {
        let mut stored = load(db).await?.context("ErisAuth has no passphrase yet")?;
        let out = change(&mut stored.state);
        let (status, body) = db
            .put(&format!("/v1/items/{}", stored.id), json!({"body": stored.state, "revision": stored.revision}))
            .await?;
        match status {
            200 => return Ok(out),
            409 => continue,
            _ => bail!("saving {FACET} ({status}): {body}"),
        }
    }
    bail!("{FACET} kept changing underneath; try again")
}

/// Register the facet, unless it already is.
pub async fn register(db: &Db) -> Result<()> {
    let definition =
        json!({"facet": "facet", "body": {"name": FACET, "version": 1, "strict": true, "schema": schema()}});
    let (status, body) = db.post("/v1/items", definition).await?;
    match status {
        201 | 409 => Ok(()),
        _ => bail!("registering {FACET} ({status}): {body}"),
    }
}

/// Store `passphrase` as the one the plugin checks: the first time, or in
/// place of the old one. A reset starts every device's count of wrong guesses
/// afresh; a change keeps them.
pub async fn set_passphrase(db: &Db, passphrase: &str, reset: bool) -> Result<()> {
    let hashed = hash(passphrase)?;
    if load(db).await?.is_none() {
        register(db).await?;
        let state = State { passphrase: hashed, lockouts: BTreeMap::new() };
        let (status, body) = db.post("/v1/items", json!({"facet": FACET, "body": state})).await?;
        if status != 201 {
            bail!("storing the passphrase ({status}): {body}");
        }
        return Ok(());
    }
    update(db, |state| {
        state.passphrase = hashed.clone();
        if reset {
            state.lockouts.clear();
        }
    })
    .await
}

/// Let `installation` guess again now, or every device when `None`. Before
/// there is a passphrase, nobody is locked out.
pub async fn unlock(db: &Db, installation: Option<Uuid>) -> Result<()> {
    if load(db).await?.is_none() {
        return Ok(());
    }
    update(db, |state| match installation {
        Some(id) => {
            state.lockouts.remove(&id);
        }
        None => state.lockouts.clear(),
    })
    .await
}

/// How a guess went.
#[derive(Debug, PartialEq, Eq)]
pub enum Guess {
    Right,
    Wrong,
    /// This device has to wait this many seconds before guessing.
    Locked(i64),
    /// There is no passphrase to guess yet.
    Unset,
}

/// Check `passphrase` for `installation`, counting a wrong guess against it
/// and clearing its count on a right one. A locked device's guess is not
/// even checked.
pub async fn guess(db: &Db, installation: Uuid, passphrase: &str) -> Result<Guess> {
    let Some(stored) = load(db).await? else { return Ok(Guess::Unset) };
    let now = erislogin::now();
    let lockout = stored.state.lockouts.get(&installation).copied().unwrap_or_default();
    if let Some(wait) = lockout.wait(now) {
        return Ok(Guess::Locked(wait));
    }
    let right = verify(passphrase, &stored.state.passphrase)?;
    if right && lockout == Lockout::default() {
        return Ok(Guess::Right);
    }
    update(db, |state| {
        if right {
            state.lockouts.remove(&installation);
        } else {
            state.lockouts.entry(installation).or_default().fail(now);
        }
    })
    .await?;
    Ok(if right { Guess::Right } else { Guess::Wrong })
}
