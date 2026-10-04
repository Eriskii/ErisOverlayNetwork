//! ErisAuth inside an Android app. The daemon runs in the app's process: the
//! app's screens send it the [commands](super::command) the socket takes, and
//! apps make their requests through the app's bound service, which passes
//! each here with who Android says is calling, since apps on Android cannot
//! reach one another's sockets. Answers are JSON strings, for the JNI layer
//! to hand on. Nothing listens on the network for the app's screens.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use erislogin::agent::{Ask, Reply};
use serde_json::{Value, json};
use uuid::Uuid;

use super::agent::{Agent, Peer};
use super::{Joined, Paths};

#[derive(Clone)]
pub struct Mobile {
    agent: Arc<Agent>,
}

impl Mobile {
    /// Start with the key and standing kept in `data`.
    pub async fn start(data: PathBuf, runtime: PathBuf) -> Result<Self> {
        let paths = Paths { data, runtime };
        let agent = Agent::new(paths.clone(), false);
        if let Some(joined) = Joined::load(&paths)? {
            agent.stand(joined).await?;
        }
        Ok(Mobile { agent })
    }

    /// One command from the app's screens. A ticket for a new device comes
    /// with its QR code laid out for the app to draw: `qr.side` modules a
    /// side, `qr.modules` a `1` for each dark one, row by row.
    pub async fn command(&self, message: &str) -> String {
        let message: Value = match serde_json::from_str(message) {
            Ok(message) => message,
            Err(e) => {
                return json!({"error": "bad_request", "message": format!("unreadable command: {e}")}).to_string();
            }
        };
        let mut answer = super::command::run(&self.agent, message).await;
        if let Some(ticket) = answer["ticket"].as_str() {
            match erislogin::qr::raster(ticket, 1) {
                Ok((side, pixels)) => {
                    let modules: String = pixels.iter().map(|&p| if p == 0 { '1' } else { '0' }).collect();
                    answer["qr"] = json!({"side": side, "modules": modules});
                }
                Err(e) => answer = json!({"error": "cannot_add", "message": e.to_string()}),
            }
        }
        answer.to_string()
    }

    /// An app asks, as ErisLogin's [`Ask`] in JSON, with `caller` the
    /// [`Peer`] Android names: `{"id", "ticket"}`, or `{"error"}`.
    pub async fn ask(&self, ask: &str, caller: &str) -> String {
        let ask: Ask = match serde_json::from_str(ask) {
            Ok(ask) => ask,
            Err(e) => return json!({"error": format!("unreadable request: {e}")}).to_string(),
        };
        let caller: Peer = serde_json::from_str(caller).unwrap_or_default();
        match self.agent.open(&ask, caller).await {
            Ok((id, ticket)) => json!({"id": id, "ticket": ticket}).to_string(),
            Err(why) => json!({"error": why}).to_string(),
        }
    }

    /// Follow request `id` to its answer: `{"approved": [...]}` or `{"denied": "why"}`.
    pub async fn answer(&self, id: &str) -> String {
        let Ok(id) = id.parse::<Uuid>() else { return json!({"error": "no such request"}).to_string() };
        let reply = self.agent.follow(id).await;
        serde_json::to_string(&reply)
            .unwrap_or_else(|_| serde_json::to_string(&Reply::Error("unanswerable".into())).unwrap())
    }

    /// Wait up to `within` for request `id` to reach the person, for the app's
    /// notification: the approval, or `{"settled": true}` if it ended first or
    /// time ran out.
    pub async fn asked(&self, id: &str, within: Duration) -> String {
        let Ok(id) = id.parse::<Uuid>() else { return json!({"settled": true}).to_string() };
        match self.agent.asked(id, within).await {
            Some(approval) => serde_json::to_string(&approval).unwrap_or_default(),
            None => json!({"settled": true}).to_string(),
        }
    }

    /// The app that asked went away.
    pub async fn withdraw(&self, id: &str) {
        if let Ok(id) = id.parse::<Uuid>() {
            self.agent.withdraw(id).await;
        }
    }
}
