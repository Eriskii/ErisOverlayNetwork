//! ErisDB over loopback HTTP, as a registered installation that proves its
//! S256 secret. The plugin runs on the same machine as the core, so it uses
//! the core's own listener rather than dialing it over Iroh.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use uuid::Uuid;

/// The header an HTTP installation proves itself with.
pub const PROOF_HEADER: &str = "X-ErisDB-Client-Proof";

/// A registered installation's standing with one core: where it is, who the
/// core knows it as, the secret that proves it, and its latest access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub url: String,
    pub client_id: Uuid,
    pub proof: String,
    pub token: String,
}

impl Credential {
    fn path(dir: &Path) -> PathBuf {
        dir.join("erisdb.json")
    }

    pub fn exists(dir: &Path) -> bool {
        Self::path(dir).exists()
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("no ErisDB credential at {}; run `erisauth home setup` first", path.display()))?;
        serde_json::from_slice(&bytes).with_context(|| format!("{} is not an ErisDB credential", path.display()))
    }

    /// Write owner-only and atomically, so a reader sees the old credential
    /// or the new one and never half of either.
    pub fn save(&self, dir: &Path) -> Result<()> {
        write_private(&Self::path(dir), &serde_json::to_vec_pretty(self)?)
    }
}

/// Write `bytes` to `path`, readable only by its owner, replacing any file there atomically.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().context("a credential path has a directory")?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// A fresh installation secret and its S256 challenge.
pub fn new_proof() -> (String, String) {
    let proof = B64.encode(rand::random::<[u8; 32]>());
    let challenge = B64.encode(sha2::Sha256::digest(proof.as_bytes()));
    (proof, challenge)
}

pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .expect("an HTTP client")
}

/// A connection to a core as one installation, renewing its access token
/// whenever the core says it has expired and saving the renewed one.
pub struct Session {
    http: reqwest::Client,
    credential: Credential,
    dir: PathBuf,
}

impl Session {
    pub fn open(dir: &Path) -> Result<Self> {
        Ok(Session { http: http(), credential: Credential::load(dir)?, dir: dir.to_path_buf() })
    }

    pub fn client_id(&self) -> Uuid {
        self.credential.client_id
    }

    async fn once(&self, method: &reqwest::Method, path: &str, body: &Option<Value>) -> Result<(u16, Value)> {
        let mut request = self
            .http
            .request(method.clone(), format!("{}{path}", self.credential.url))
            .bearer_auth(&self.credential.token)
            .header(PROOF_HEADER, &self.credential.proof);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.with_context(|| format!("reaching ErisDB at {}", self.credential.url))?;
        let status = response.status().as_u16();
        Ok((status, response.json().await.unwrap_or(Value::Null)))
    }

    /// One call. A 401 renews the access token with the installation's
    /// proof and tries once more; any other status is the caller's to read.
    pub async fn send(&mut self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<(u16, Value)> {
        let first = self.once(&method, path, &body).await?;
        if first.0 != 401 {
            return Ok(first);
        }
        self.renew().await?;
        self.once(&method, path, &body).await
    }

    async fn renew(&mut self) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/v1/clients/{}/refresh", self.credential.url, self.credential.client_id))
            .header(PROOF_HEADER, &self.credential.proof)
            .json(&json!({}))
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!("ErisDB refused to renew this installation ({status}): {body}");
        }
        self.credential.token = body["token"].as_str().context("renewal returned no token")?.to_string();
        self.credential.save(&self.dir)
    }

    pub async fn get(&mut self, path: &str) -> Result<(u16, Value)> {
        self.send(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&mut self, path: &str, body: Value) -> Result<(u16, Value)> {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }

    pub async fn put(&mut self, path: &str, body: Value) -> Result<(u16, Value)> {
        self.send(reqwest::Method::PUT, path, Some(body)).await
    }
}
