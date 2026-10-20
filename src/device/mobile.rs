//! ErisAuth inside an Android app. The daemon runs in the app's process. Android apps cannot
//! reach one another's sockets, so apps bind the app's service instead, which hands each request
//! line here with who Android says is calling ([`Mobile::request`]); the app's own screens send
//! the [commands](super::command) the socket takes ([`Mobile::command`]). Answers are JSON
//! strings, for the JNI layer to hand on. Nothing listens on the network for the app's screens.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use erisoverlaynetwork::agent::Reply;
use erisoverlaynetwork::net::Network;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::Daemon;
use super::agent::Caller;

/// The most withdrawals kept for requests that have not arrived yet.
const EARLY: usize = 256;

#[derive(Clone)]
pub struct Mobile {
    daemon: Arc<Daemon>,
    requests: Arc<Mutex<Requests>>,
}

#[derive(Default)]
struct Requests {
    /// The requests apps have open, by the key the service gave each, and how to take each back.
    open: HashMap<String, oneshot::Sender<()>>,
    /// Requests withdrawn before they arrived: the service hands each on from a thread of its own,
    /// so an app's withdrawal can overtake its request.
    early: VecDeque<String>,
}

impl Mobile {
    /// Start with the key and the mesh kept in `data`.
    pub async fn start(data: PathBuf, network: Network) -> Result<Self> {
        let daemon = Daemon::start(&data, network, false).await?;
        Ok(Mobile { daemon, requests: Arc::new(Mutex::new(Requests::default())) })
    }

    /// One command from the app's screens.
    pub async fn command(&self, message: &str) -> String {
        match serde_json::from_str::<Value>(message) {
            Ok(message) => super::command::run(&self.daemon, message).await.to_string(),
            Err(e) => json!({"error": "bad_request", "message": format!("an unreadable command: {e}")}).to_string(),
        }
    }

    /// An app's request line, exactly as it would send it on the agent socket, from `caller`: the
    /// [`Caller`] Android names, as JSON. Answers the reply line once there is one; `key`, the
    /// service's name for the request, takes it back with [`Mobile::withdraw`].
    pub async fn request(&self, key: &str, line: &str, caller: &str) -> String {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(e) => return reply(&Reply::Error(format!("an unreadable request: {e}"))),
        };
        let caller: Caller = serde_json::from_str(caller).unwrap_or_default();
        let withdrawn_already = Reply::Error("the app withdrew its request".into());
        let (send, withdrawn) = oneshot::channel();
        {
            let mut requests = self.requests();
            if let Some(at) = requests.early.iter().position(|early| early == key) {
                requests.early.remove(at);
                return reply(&withdrawn_already);
            }
            requests.open.insert(key.to_string(), send);
        }
        let answer = tokio::select! {
            answer = super::socket::agent(&self.daemon, message, caller) => reply(&answer),
            _ = withdrawn => reply(&withdrawn_already),
        };
        self.requests().open.remove(key);
        answer
    }

    /// The app that opened request `key` went away, or took it back.
    pub fn withdraw(&self, key: &str) {
        let mut requests = self.requests();
        match requests.open.remove(key) {
            Some(withdraw) => {
                let _ = withdraw.send(());
            }
            None => {
                requests.early.push_back(key.to_string());
                if requests.early.len() > EARLY {
                    requests.early.pop_front();
                }
            }
        }
    }

    fn requests(&self) -> std::sync::MutexGuard<'_, Requests> {
        self.requests.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn reply(reply: &Reply) -> String {
    serde_json::to_string(reply).unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
}
