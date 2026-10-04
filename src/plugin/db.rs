//! The ErisDB a plugin call brings: the core's address and a capability for
//! the grants ErisAuth's manifest names, good for this call. The plugin runs
//! beside the core, so it uses the core's own HTTP listener.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

/// Where the core is, and what to show it, as the invocation's context says.
#[derive(Debug, Clone, Deserialize)]
pub struct Access {
    pub url: String,
    pub token: String,
}

pub struct Db {
    http: reqwest::Client,
    access: Access,
}

impl Db {
    pub fn new(access: Access) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .expect("an HTTP client");
        Db { http, access }
    }

    /// One call; any status is the caller's to read.
    pub async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<(u16, Value)> {
        let mut request =
            self.http.request(method, format!("{}{path}", self.access.url)).bearer_auth(&self.access.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.with_context(|| format!("reaching ErisDB at {}", self.access.url))?;
        let status = response.status().as_u16();
        Ok((status, response.json().await.unwrap_or(Value::Null)))
    }

    pub async fn get(&self, path: &str) -> Result<(u16, Value)> {
        self.send(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<(u16, Value)> {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<(u16, Value)> {
        self.send(reqwest::Method::PUT, path, Some(body)).await
    }
}
