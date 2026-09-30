//! ErisAuth inside an Android app. The daemon runs in the app's process: the
//! approval page for the app's WebView, and the requests that apps make
//! through the app's bound service. The service passes each request here
//! with who Android says is calling, since apps on Android cannot reach one
//! another's sockets. Answers are JSON strings, for the JNI layer to hand on.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use erislogin::agent::{Ask, Reply};
use serde_json::json;
use uuid::Uuid;

use super::agent::{Agent, Peer};
use super::{Joined, Paths, page};

#[derive(Clone)]
pub struct Mobile {
    agent: Arc<Agent>,
    link: Arc<String>,
}

impl Mobile {
    /// Start with the key and standing kept in `data`, and serve the page.
    pub async fn start(data: PathBuf, runtime: PathBuf) -> Result<Self> {
        let paths = Paths { data, runtime };
        let agent = Agent::new(paths.clone(), false);
        if let Some(joined) = Joined::load(&paths)? {
            agent.stand(joined).await?;
        }
        let (link, page) = page::listen(&paths.runtime, agent.clone()).await?;
        agent.set_page(link.clone());
        tokio::spawn(page);
        Ok(Mobile { agent, link: Arc::new(link) })
    }

    /// The approval page, token included, for the app's WebView.
    pub fn link(&self) -> &str {
        &self.link
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
