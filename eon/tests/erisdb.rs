//! ErisDB as the mesh's database, with the real `erisdb` core on a real Postgres: it joins the
//! mesh through a device's eon to serve `database/1`, and a new one replaces it the same way; an
//! app approved to use the database takes a tap and its passphrase, which ErisDB checks, with
//! lockouts, before the app hears it is approved; and the person changes the passphrase from a
//! device.
//!
//! The core is built from `ERISDB_SOURCE`, ErisDB's `erisdb` crate (`../ErisDB/erisdb` beside this
//! checkout in the Eris monorepo), or taken ready-built from `ERISDB_BIN`.

mod support;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use erisoverlaynetwork::agent::{self, Answer, Ask};
use erisoverlaynetwork::ceremony::{self, Approval};
use erisoverlaynetwork::directory::Snapshot;
use erisoverlaynetwork::net::{self, Network};
use erisoverlaynetwork::ticket::Ticket;
use erisoverlaynetwork::{Capability, Uses};
use iroh::SecretKey;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use support::{Device, PATIENCE, events};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{ImageExt, ReuseDirective};
use tokio::io::AsyncWriteExt;
use tokio::sync::OnceCell;

const PASSPHRASE: &str = "correct horse battery";

/// What ErisDB serves.
const DATABASE: &str = "database/1";

/// The server in use of the database, in a state.
fn database(state: &Value) -> &Value {
    state["mesh"]["servers"]
        .as_array()
        .and_then(|servers| servers.iter().find(|s| s["capability"] == DATABASE))
        .map(|s| &s["apps"][0])
        .unwrap_or(&Value::Null)
}

/// One HTTP request to ErisDB on a bi-stream of its own, as an app makes it: the status and the
/// JSON body.
async fn request(conn: &iroh::endpoint::Connection, method: &str, path: &str) -> (u16, Value) {
    use http_body_util::{BodyExt, Empty};
    let (send, recv) = conn.open_bi().await.unwrap();
    let io = hyper_util::rt::TokioIo::new(tokio::io::join(recv, send));
    let (mut sender, driver) = hyper::client::conn::http1::handshake(io).await.unwrap();
    let driver = tokio::spawn(driver);
    let request = hyper::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "erisdb")
        .body(Empty::<hyper::body::Bytes>::new())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    driver.abort();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

struct Pg {
    _container: ContainerAsync<Postgres>,
    base_url: String,
}

static PG: OnceCell<Pg> = OnceCell::const_new();

async fn pg() -> &'static Pg {
    PG.get_or_init(|| async {
        let container = Postgres::default()
            // One container, kept and used again by every run: a container held in a static is
            // never dropped, so one started afresh each run would outlive it.
            .with_reuse(ReuseDirective::Always)
            .start()
            .await
            .expect("start postgres");
        let port = container.get_host_port_ipv4(5432).await.expect("pg port");
        Pg { _container: container, base_url: format!("postgres://postgres:postgres@127.0.0.1:{port}") }
    })
    .await
}

/// The `erisdb` executable, built once per run from a copy of its source, so that building it
/// writes nothing into ErisDB's own checkout.
async fn binary() -> &'static PathBuf {
    static BINARY: OnceCell<PathBuf> = OnceCell::const_new();
    BINARY
        .get_or_init(|| async {
            if let Some(bin) = std::env::var_os("ERISDB_BIN") {
                return PathBuf::from(bin);
            }
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let source = std::env::var_os("ERISDB_SOURCE")
                .map(PathBuf::from)
                .unwrap_or_else(|| manifest.join("../../ErisDB/erisdb"));
            assert!(
                source.join("Cargo.toml").is_file(),
                "these tests run ErisDB's core: set ERISDB_SOURCE to its `erisdb` crate or ERISDB_BIN to a built `erisdb`"
            );
            // Outside this workspace, which would otherwise claim it; Cargo still takes its
            // configuration from where it runs.
            let copy = std::env::temp_dir().join("eon-tests-erisdb");
            let _ = std::fs::remove_dir_all(&copy);
            copy_tree(&source, &copy);
            let built = tokio::process::Command::new(env!("CARGO"))
                .args(["build", "--bin", "erisdb", "--manifest-path"])
                .arg(copy.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", manifest.join("../target/erisdb"))
                .status()
                .await
                .unwrap();
            assert!(built.success(), "building erisdb from {}", source.display());
            manifest.join("../target/erisdb/debug/erisdb")
        })
        .await
}

/// Copy a crate's files, leaving its build output behind.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (path, name) = (entry.path(), entry.file_name());
        if name == "target" {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(name));
        } else {
            std::fs::copy(&path, to.join(name)).unwrap();
        }
    }
}

/// One ErisDB deployment: its own database and secret.
struct ErisDb {
    database_url: String,
    secret: String,
    server: Option<tokio::process::Child>,
}

impl ErisDb {
    async fn new() -> Self {
        let pg = pg().await;
        let db = format!("eon_{}", uuid::Uuid::new_v4().simple());
        let admin =
            PgPoolOptions::new().max_connections(1).connect(&format!("{}/postgres", pg.base_url)).await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(r#"CREATE DATABASE "{db}""#))).execute(&admin).await.unwrap();
        ErisDb {
            database_url: format!("{}/{db}", pg.base_url),
            secret: uuid::Uuid::new_v4().simple().to_string(),
            server: None,
        }
    }

    async fn command(&self, args: &[&str]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(binary().await);
        command
            .args(args)
            .env("DATABASE_URL", &self.database_url)
            .env("ERISDB_SECRET", &self.secret)
            .env("ERISDB_LISTEN", "127.0.0.1:0")
            .env("ERIS_NETWORK", "loopback")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    /// `erisdb join`, asking `device`'s ErisOverlayNetwork, which the person there approves.
    async fn join(&self, device: &Device) {
        let mut join = self.command(&["join"]).await;
        join.env("XDG_RUNTIME_DIR", device.runtime.path());
        let child = join.spawn().unwrap();
        let prompt = device.prompt().await;
        assert_eq!(prompt["serves"], json!([{"capability": DATABASE, "single": true, "passphrase": true}]), "{prompt}");
        assert_eq!(prompt["uses"], json!({}));
        assert_eq!(device.say(json!({"op": "decide", "id": prompt["id"]})).await, json!({"ok": true}));
        let output = tokio::time::timeout(PATIENCE, child.wait_with_output()).await.unwrap().unwrap();
        assert!(output.status.success(), "erisdb join: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// Set the passphrase on the server, as its operator does.
    async fn set_passphrase(&self, passphrase: &str) {
        let mut set = self.command(&["passphrase", "set"]).await.spawn().unwrap();
        set.stdin.take().unwrap().write_all(format!("{passphrase}\n").as_bytes()).await.unwrap();
        let output = set.wait_with_output().await.unwrap();
        assert!(output.status.success(), "erisdb passphrase set: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// Start `erisdb serve`, and wait until `device` finds it.
    async fn serve(&mut self, device: &Device) {
        let mut serve = self.command(&["serve"]).await;
        serve.stdout(Stdio::null()).stderr(Stdio::inherit());
        self.server = Some(serve.spawn().unwrap());
        let app = self.app().await;
        device
            .until("the device finds this ErisDB", |s| database(s)["app"] == app && database(s)["reachable"] == true)
            .await;
    }

    /// The key this ErisDB serves under, as its endpoint id.
    async fn app(&self) -> String {
        let output = self.command(&["endpoint-id"]).await.output().await.unwrap();
        assert!(output.status.success(), "erisdb endpoint-id: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }
}

/// A mesh of a laptop and a phone, with ErisDB joined through the laptop, its passphrase set,
/// and serving.
async fn mesh() -> (Device, Device, ErisDb) {
    let laptop = Device::founder("laptop").await;
    let phone = Device::start().await;
    phone.join_through(&laptop, "phone").await;
    let mut db = ErisDb::new().await;
    db.join(&laptop).await;
    db.set_passphrase(PASSPHRASE).await;
    db.serve(&laptop).await;
    phone.until("the phone finds the mesh's database", |s| database(s)["reachable"] == true).await;
    (laptop, phone, db)
}

/// Using the database with `grants`.
fn uses(grants: &[&str]) -> Uses {
    Uses::from([(DATABASE.parse().unwrap(), grants.iter().map(|g| g.to_string()).collect())])
}

/// An app with a key of its own, beside a device's eon.
struct App {
    key: SecretKey,
}

impl App {
    fn new() -> Self {
        App { key: SecretKey::generate() }
    }

    fn ask(&self, grants: &[&str]) -> Ask {
        Ask { name: "Tasks".into(), uses: uses(grants), serves: vec![], app: self.key.public() }
    }

    /// Ask `device` in the background.
    fn asks(&self, device: &Device, grants: &[&str]) -> tokio::task::JoinHandle<erisoverlaynetwork::Result<Answer>> {
        let (socket, ask) = (device.socket(), self.ask(grants));
        tokio::spawn(async move { agent::connect_to(&socket).await?.expect("the daemon listens").approve(&ask).await })
    }

    /// Ask the mesh's database, under this app's own key, what this app may do there.
    async fn permissions(&self, snapshot: &Snapshot) -> (u16, Value) {
        let erisdb = snapshot.server(DATABASE).expect("the snapshot knows where the mesh's database is");
        let endpoint = net::endpoint(self.key.clone(), &Network::Loopback).await.unwrap();
        let alpn = DATABASE.parse::<Capability>().unwrap().alpn();
        let conn = endpoint.connect(erisdb.endpoint_addr(), &alpn).await.unwrap();
        request(&conn, "GET", "/v1/permissions").await
    }
}

#[tokio::test]
async fn erisdb_joins_through_a_device_as_the_database_and_every_device_finds_it() {
    let (laptop, phone, _db) = mesh().await;
    let state = phone.state().await;
    assert_eq!(database(&state)["device"], "laptop", "approved there");
    let name = database(&state)["name"].as_str().unwrap().to_string();
    phone
        .until("the phone hears of it", |s| events(s).contains(&format!("laptop approved {name} as your database")))
        .await;
    assert_eq!(database(&laptop.state().await)["app"], database(&state)["app"]);
}

/// A mesh has a single database: a new one approved on any device revokes the one before it, and
/// apps enroll at the new one from then on.
#[tokio::test]
async fn a_new_erisdb_replaces_the_one_before_it() {
    let (laptop, phone, old) = mesh().await;
    let before = database(&laptop.state().await).clone();
    let mut new = ErisDb::new().await;
    new.join(&phone).await;
    new.set_passphrase("another passphrase").await;
    new.serve(&laptop).await;

    let state = laptop
        .until("the laptop hears of it", |s| events(s).contains(&"phone approved ErisDB as your database".to_string()))
        .await;
    assert_eq!(database(&state)["device"], "phone");
    let snapshot = agent::connect_to(&laptop.socket()).await.unwrap().unwrap().snapshot().await.unwrap();
    let revoked: erisoverlaynetwork::NoteId = before["note"].as_str().unwrap().parse().unwrap();
    assert!(snapshot.list().unwrap().is_revoked(&revoked), "the old ErisDB's approval is revoked");
    assert_eq!(snapshot.server(DATABASE).unwrap().id.to_string(), new.app().await);
    drop(old);

    let app = App::new();
    let asking = app.asks(&laptop, &["tasks:read"]);
    let prompt = laptop.prompt().await;
    let right = json!({"op": "decide", "id": prompt["id"], "passphrase": "another passphrase"});
    assert_eq!(laptop.say(right).await, json!({"ok": true}), "the new ErisDB checks its own passphrase");
    let Answer::Approved { snapshot, .. } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    assert_eq!(app.permissions(&snapshot).await.0, 200);
}

#[tokio::test]
async fn an_app_is_approved_for_the_database_with_a_tap_and_the_passphrase() {
    let (laptop, _phone, _db) = mesh().await;
    let app = App::new();
    let asking = app.asks(&laptop, &["tasks:read", "tasks:write"]);
    let prompt = laptop.prompt().await;
    assert_eq!(prompt["passphrase"], true);
    assert_eq!(prompt["uses"], json!({DATABASE: ["tasks:read", "tasks:write"]}));

    let decide = |passphrase: Option<&str>| json!({"op": "decide", "id": prompt["id"], "grants": {DATABASE: ["tasks:read"]}, "passphrase": passphrase});
    assert_eq!(laptop.say(decide(None)).await["error"], "needs_passphrase");
    assert_eq!(laptop.say(decide(Some("a wrong guess"))).await["error"], "wrong_passphrase");
    assert_eq!(laptop.approvals().await.len(), 1, "a wrong passphrase lets the person try again");
    assert!(!asking.is_finished(), "the app hears nothing until ErisDB accepts");
    assert_eq!(laptop.say(decide(Some(PASSPHRASE))).await, json!({"ok": true}));

    let Answer::Approved { note, snapshot } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    assert_eq!(note.body.uses, uses(&["tasks:read"]), "the person chose part of what was asked");
    assert!(note.body.serves.is_empty());
    let (status, held) = app.permissions(&snapshot).await;
    assert_eq!(status, 200, "ErisDB knows the app by its key: {held}");
    assert_eq!(held["grants"], json!(["tasks:read"]));

    // Revoked, the app is cut off at ErisDB at once.
    assert_eq!(laptop.say(json!({"op": "revoke", "note": note.id.to_string()})).await, json!({"ok": true}));
    tokio::time::timeout(PATIENCE, async {
        while app.permissions(&snapshot).await.0 == 200 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("ErisDB kept serving a revoked app");
}

#[tokio::test]
async fn wrong_guesses_lock_a_device_out_and_not_another() {
    let (laptop, phone, db) = mesh().await;
    let app = App::new();
    let _asking = app.asks(&laptop, &["tasks:read"]);
    let prompt = laptop.prompt().await;
    let guess = |passphrase: &str| json!({"op": "decide", "id": prompt["id"], "passphrase": passphrase});
    for _ in 0..3 {
        assert_eq!(laptop.say(guess("a wrong guess")).await["error"], "wrong_passphrase", "three free guesses");
    }
    let locked = laptop.say(guess(PASSPHRASE)).await;
    assert_eq!(locked["error"], "locked", "{locked}");
    let wait = locked["retry_after"].as_i64().unwrap();
    assert!((1..=60).contains(&wait), "the person hears how long to wait: {locked}");
    assert!(locked["message"].as_str().unwrap().contains("try again"), "{locked}");

    // The phone guesses for itself.
    let other = App::new();
    let asking = other.asks(&phone, &["tasks:read"]);
    let prompt = phone.prompt().await;
    let right = json!({"op": "decide", "id": prompt["id"], "passphrase": PASSPHRASE});
    assert_eq!(phone.say(right).await, json!({"ok": true}));
    assert!(matches!(asking.await.unwrap().unwrap(), Answer::Approved { .. }));

    // The operator lets the laptop guess again.
    let unlock = db.command(&["passphrase", "unlock", &laptop.id().await]).await.output().await.unwrap();
    assert!(unlock.status.success(), "{}", String::from_utf8_lossy(&unlock.stderr));
    assert_eq!(laptop.say(guess(PASSPHRASE)).await, json!({"ok": true}));
}

/// An app with no eon beside it asks by ticket; for the database the person types the passphrase
/// there too, and the session is approved only once ErisDB has enrolled the app.
#[tokio::test]
async fn a_browser_app_is_enrolled_at_erisdb_by_ticket() {
    let (laptop, _phone, _db) = mesh().await;
    let added = laptop.say(json!({"op": "add_app"})).await;
    let app = App::new();
    let endpoint = net::endpoint(app.key.clone(), &Network::Loopback).await.unwrap();
    let wants = ceremony::Ask::App { name: "Tasks".into(), uses: uses(&["tasks:read"]), serves: vec![] };
    let ticket = Ticket::parse(added["ticket"].as_str().unwrap()).unwrap();
    let joining = ceremony::redeem(&endpoint, &ticket, wants).await.unwrap();
    let prompt = laptop.prompt().await;
    assert_eq!(prompt["fingerprint"], joining.fingerprint);
    assert_eq!(prompt["passphrase"], true);
    let wrong = laptop.say(json!({"op": "decide", "id": prompt["id"], "passphrase": "a wrong guess"})).await;
    assert_eq!(wrong["error"], "wrong_passphrase");
    let right = laptop.say(json!({"op": "decide", "id": prompt["id"], "passphrase": PASSPHRASE})).await;
    assert_eq!(right, json!({"ok": true}));

    let Some(Approval::App { note, snapshot }) = joining.wait(PATIENCE).await.unwrap() else { panic!("not approved") };
    assert_eq!(note.app(), app.key.public());
    let (status, held) = app.permissions(&snapshot).await;
    assert_eq!(status, 200, "{held}");
}

#[tokio::test]
async fn the_passphrase_is_changed_from_a_device() {
    let (laptop, phone, _db) = mesh().await;
    let short = laptop.say(json!({"op": "passphrase", "current": PASSPHRASE, "new": "short"})).await;
    assert_eq!(short["error"], "bad_request", "{short}");
    let wrong = laptop.say(json!({"op": "passphrase", "current": "a wrong guess", "new": "a new passphrase"})).await;
    assert_eq!(wrong["error"], "wrong_passphrase", "{wrong}");
    let changed = laptop.say(json!({"op": "passphrase", "current": PASSPHRASE, "new": "a new passphrase"})).await;
    assert_eq!(changed, json!({"ok": true}));

    // At the phone's terminal: the current passphrase, then the new one, on stdin.
    let mut change = phone.command(&["passphrase"]).spawn().unwrap();
    change.stdin.take().unwrap().write_all(b"a new passphrase\nanother passphrase\n").await.unwrap();
    let output = change.wait_with_output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let app = App::new();
    let asking = app.asks(&phone, &["tasks:read"]);
    let prompt = phone.prompt().await;
    let old = phone.say(json!({"op": "decide", "id": prompt["id"], "passphrase": PASSPHRASE})).await;
    assert_eq!(old["error"], "wrong_passphrase");
    let new = phone.say(json!({"op": "decide", "id": prompt["id"], "passphrase": "another passphrase"})).await;
    assert_eq!(new, json!({"ok": true}));
    assert!(matches!(asking.await.unwrap().unwrap(), Answer::Approved { .. }));
}

#[tokio::test]
async fn removing_a_device_cuts_off_the_apps_it_enrolled() {
    let (laptop, phone, _db) = mesh().await;
    let app = App::new();
    let asking = app.asks(&phone, &["tasks:read"]);
    let prompt = phone.prompt().await;
    phone.say(json!({"op": "decide", "id": prompt["id"], "passphrase": PASSPHRASE})).await;
    let Answer::Approved { snapshot, .. } = asking.await.unwrap().unwrap() else { panic!("not approved") };
    assert_eq!(app.permissions(&snapshot).await.0, 200);

    assert_eq!(laptop.say(json!({"op": "remove", "devices": [phone.id().await]})).await, json!({"ok": true}));
    tokio::time::timeout(PATIENCE, async {
        while app.permissions(&snapshot).await.0 == 200 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("ErisDB kept serving an app of a removed device");
}
