//! A real Postgres, a real ErisDB core with the shipped ErisAuth plugin
//! manifest, and the real `erisauth` and plugin executables.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde_json::{Value, json};
use sha2::Digest;
use sqlx::postgres::PgPoolOptions;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio::io::AsyncWriteExt;
use tokio::sync::OnceCell;

pub const SECRET: &[u8] = b"erisauth-test-secret";
pub const PASSPHRASE: &str = "correct horse battery";

struct Pg {
    _container: ContainerAsync<Postgres>,
    base_url: String,
}

static PG: OnceCell<Pg> = OnceCell::const_new();

async fn pg() -> &'static Pg {
    PG.get_or_init(|| async {
        let container = Postgres::default().start().await.expect("start postgres");
        let port = container.get_host_port_ipv4(5432).await.expect("pg port");
        Pg { _container: container, base_url: format!("postgres://postgres:postgres@127.0.0.1:{port}") }
    })
    .await
}

/// An HTTP caller of the core: an operator's manual token, or a registered
/// installation proving its S256 secret over loopback.
#[derive(Clone)]
pub struct Caller {
    pub base: String,
    pub token: String,
    pub proof: String,
    http: reqwest::Client,
}

impl Caller {
    pub fn new(base: &str, token: &str) -> Self {
        let proof = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        Caller { base: base.to_string(), token: token.to_string(), proof, http: reqwest::Client::new() }
    }

    pub fn challenge(&self) -> String {
        B64.encode(sha2::Sha256::digest(self.proof.as_bytes()))
    }

    /// This caller's identity as a pairing session records it.
    pub fn identity(&self) -> Value {
        json!({"kind": "browser", "key": self.challenge()})
    }

    pub async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header("X-ErisDB-Client-Proof", &self.proof);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    pub async fn get(&self, path: &str) -> (u16, Value) {
        self.send(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.send(reqwest::Method::POST, path, Some(body)).await
    }

    /// Call the ErisAuth plugin.
    pub async fn call(&self, operation: &str, input: Value) -> (u16, Value) {
        self.post("/v1/call", json!({"plugin": "erisauth", "operation": operation, "input": input})).await
    }
}

pub struct Core {
    pub base: String,
    pub root: Caller,
    /// The plugin's state directory, as the manifest's `--home` names it.
    pub home: PathBuf,
    _dirs: tempfile::TempDir,
}

impl Core {
    /// A core in a fresh database, serving the shipped plugin manifest with
    /// its executable and home pointed at this build and a temporary directory.
    pub async fn start() -> Self {
        let pg = pg().await;
        let db = format!("erisauth_{}", uuid::Uuid::new_v4().simple());
        let admin =
            PgPoolOptions::new().max_connections(1).connect(&format!("{}/postgres", pg.base_url)).await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(r#"CREATE DATABASE "{db}""#))).execute(&admin).await.unwrap();
        let pool = PgPoolOptions::new().max_connections(8).connect(&format!("{}/{db}", pg.base_url)).await.unwrap();
        erisdb::MIGRATOR.run(&pool).await.unwrap();

        let dirs = tempfile::tempdir().unwrap();
        let plugins = dirs.path().join("plugins.d");
        let home = dirs.path().join("home");
        std::fs::create_dir(&plugins).unwrap();
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/erisauth.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(shipped).unwrap()).unwrap();
        manifest["executable"] = json!(env!("CARGO_BIN_EXE_erisdb-plugin-erisauth"));
        manifest["args"] = json!(["--home", home]);
        std::fs::write(plugins.join("erisauth.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let registry = erisdb::PluginRegistry::load_dir(&plugins).unwrap();

        let app = erisdb::app_with_plugins(pool, SECRET.to_vec(), registry);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap();
        });
        let root = Caller::new(&base, &erisdb::auth::mint(SECRET, &["*"], Some(3600), None).unwrap());
        Core { base, root, home, _dirs: dirs }
    }

    /// Cut a pairing as the root operator: its id and a ticket for it.
    pub async fn cut(&self) -> (String, String) {
        let (status, cut) = self.root.post("/v1/pairings", json!({})).await;
        assert_eq!(status, 201, "{cut}");
        let ticket =
            erislogin::ticket::Ticket::new(cut["secret"].as_str().unwrap().into(), None, Some(self.base.clone()), None)
                .unwrap()
                .encode("erisdb")
                .unwrap();
        (cut["id"].as_str().unwrap().to_string(), ticket)
    }

    /// Run `erisauth home setup`, approving its request as the operator would.
    pub async fn setup_home(&self) -> std::process::Output {
        let (id, ticket) = self.cut().await;
        let mut child = self.erisauth(&["home", "setup", "--erisdb", &self.base, &ticket]);
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(format!("{PASSPHRASE}\n").as_bytes()).await.unwrap();
        drop(stdin);
        self.await_requested(&id).await;
        let (status, approved) = self.root.post(&format!("/v1/pairings/{id}/approve"), json!({})).await;
        assert_eq!(status, 200, "{approved}");
        let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await.unwrap().unwrap();
        assert!(output.status.success(), "setup failed: {}", String::from_utf8_lossy(&output.stderr));
        output
    }

    /// `erisauth` with this core's home directory.
    pub fn erisauth(&self, args: &[&str]) -> tokio::process::Child {
        let (first, rest) = args.split_at(2);
        tokio::process::Command::new(env!("CARGO_BIN_EXE_erisauth"))
            .args(first)
            .arg("--home")
            .arg(&self.home)
            .args(rest)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }

    /// Run an `erisauth home …` command to completion.
    pub async fn run(&self, args: &[&str]) -> std::process::Output {
        let child = self.erisauth(args);
        tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await.unwrap().unwrap()
    }

    pub async fn await_requested(&self, id: &str) {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let (_, session) = self.root.get(&format!("/v1/pairings/{id}")).await;
                if session["body"]["status"] == "requested" {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the pairing was never redeemed");
    }

    /// Enroll an HTTP installation for `requested`, approved by the operator.
    pub async fn enroll(&self, name: &str, requested: &[&str]) -> (Caller, String) {
        let (id, _) = self.cut_raw().await;
        let (caller, _) = self.redeem(&id.1, name, requested).await;
        let (status, approved) = self.root.post(&format!("/v1/pairings/{}/approve", id.0), json!({})).await;
        assert_eq!(status, 200, "{approved}");
        self.collect(caller).await
    }

    /// Cut a pairing with the operator's token: `((id, code), expires)`.
    pub async fn cut_raw(&self) -> ((String, String), i64) {
        cut_with(&self.root).await
    }

    /// A newcomer redeems `code`, asking for `requested`.
    pub async fn redeem(&self, code: &str, name: &str, requested: &[&str]) -> (Caller, Value) {
        let caller = Caller::new(&self.base, code);
        let (status, session) = caller
            .post("/v1/pair/redeem", json!({"client": name, "requested": requested, "challenge": caller.challenge()}))
            .await;
        assert_eq!(status, 200, "{session}");
        (caller, session)
    }

    /// The newcomer collects its approval and becomes a registered installation.
    pub async fn collect(&self, mut caller: Caller) -> (Caller, String) {
        let (status, collected) = caller.get("/v1/pair/status").await;
        assert_eq!(status, 200, "{collected}");
        assert_eq!(collected["status"], "approved", "{collected}");
        caller.token = collected["token"].as_str().unwrap().to_string();
        (caller, collected["client_id"].as_str().unwrap().to_string())
    }
}

/// Cut a pairing with any caller holding `meta:pairing:create`.
pub async fn cut_with(caller: &Caller) -> ((String, String), i64) {
    let (status, cut) = caller.post("/v1/pairings", json!({})).await;
    assert_eq!(status, 201, "{cut}");
    (
        (cut["id"].as_str().unwrap().to_string(), cut["secret"].as_str().unwrap().to_string()),
        cut["expires"].as_i64().unwrap(),
    )
}
