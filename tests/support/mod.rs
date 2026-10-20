//! Devices running the real `erisauth` daemon on loopback Iroh, each with its own data and
//! runtime directories, and the person at each, who answers through the daemon's socket as its
//! approval page does.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// How long anything in these tests may take to happen.
pub const PATIENCE: Duration = Duration::from_secs(60);

pub struct Device {
    pub data: tempfile::TempDir,
    pub runtime: tempfile::TempDir,
    daemon: Option<tokio::process::Child>,
}

impl Device {
    /// A device running `erisauth serve`, in no mesh yet.
    pub async fn start() -> Self {
        let mut device =
            Device { data: tempfile::tempdir().unwrap(), runtime: tempfile::tempdir().unwrap(), daemon: None };
        device.serve().await;
        device
    }

    /// A device that starts a mesh, under `name`.
    pub async fn founder(name: &str) -> Self {
        let device = Device::start().await;
        device.init(name).await;
        device
    }

    /// `erisauth` with this device's directories, on loopback.
    pub fn command(&self, args: &[&str]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_erisauth"));
        command
            .args(args)
            .env("XDG_DATA_HOME", self.data.path())
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env("ERIS_NETWORK", "loopback")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    /// Start `erisauth serve` and wait until it answers: its socket listens and its page's link
    /// is written, which happens once the daemon has started.
    pub async fn serve(&mut self) {
        let mut command = self.command(&["serve", "--no-notify"]);
        command.stdout(Stdio::null()).stderr(Stdio::inherit());
        self.daemon = Some(command.spawn().unwrap());
        let (socket, link) = (self.socket(), self.runtime.path().join("erisauth/page"));
        tokio::time::timeout(PATIENCE, async {
            while !link.exists() || tokio::net::UnixStream::connect(&socket).await.is_err() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the daemon never listened");
    }

    /// Stop the daemon, as the system would at shutdown.
    pub async fn stop(&mut self) {
        if let Some(mut daemon) = self.daemon.take() {
            let pid = daemon.id().unwrap() as i32;
            terminate(pid);
            tokio::time::timeout(PATIENCE, daemon.wait()).await.unwrap().unwrap();
        }
    }

    pub fn socket(&self) -> PathBuf {
        erisoverlaynetwork::agent::socket_in(self.runtime.path())
    }

    /// Send one line to the daemon and read its one-line answer.
    pub async fn say(&self, line: Value) -> Value {
        let stream = tokio::net::UnixStream::connect(self.socket()).await.unwrap();
        let (read, mut write) = stream.into_split();
        write.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        let mut answer = String::new();
        BufReader::new(read).read_line(&mut answer).await.unwrap();
        serde_json::from_str(&answer).unwrap_or_else(|e| panic!("{e}: {answer:?}"))
    }

    pub async fn state(&self) -> Value {
        self.say(json!({"op": "state"})).await
    }

    /// The state, once `ok` holds of it.
    pub async fn until(&self, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let mut last = Value::Null;
        let reached = tokio::time::timeout(PATIENCE, async {
            loop {
                last = self.state().await;
                if ok(&last) {
                    return last.clone();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        reached.unwrap_or_else(|_| panic!("never: {what}; the state was {last:#}"))
    }

    /// This device's id.
    pub async fn id(&self) -> String {
        self.state().await["device"]["id"].as_str().unwrap().to_string()
    }

    /// Start a mesh here.
    pub async fn init(&self, name: &str) {
        assert_eq!(self.say(json!({"op": "init", "name": name})).await["ok"], true);
    }

    /// The first request waiting for the person.
    pub async fn prompt(&self) -> Value {
        self.until("something is put to the person", |s| !s["approvals"].as_array().unwrap().is_empty()).await
            ["approvals"][0]
            .clone()
    }

    pub async fn approvals(&self) -> Vec<Value> {
        self.state().await["approvals"].as_array().unwrap().clone()
    }

    /// Show a ticket a new device joins with: `(session, ticket)`.
    pub async fn add(&self) -> (String, String) {
        let added = self.say(json!({"op": "add"})).await;
        let ticket = added["ticket"].as_str().unwrap_or_else(|| panic!("{added}")).to_string();
        (added["id"].as_str().unwrap().to_string(), ticket)
    }

    /// Join `host`'s mesh as `name` the way a person does it: a ticket on the host, the
    /// fingerprint on both screens, one approval there.
    pub async fn join_through(&self, host: &Device, name: &str) {
        let (session, ticket) = host.add().await;
        let joined = self.say(json!({"op": "join", "ticket": ticket, "name": name})).await;
        let fingerprint = joined["fingerprint"].as_str().unwrap_or_else(|| panic!("{joined}")).to_string();
        let prompt = host.prompt().await;
        assert_eq!(prompt["id"], session);
        assert_eq!(prompt["kind"], "device");
        assert_eq!(prompt["name"], name);
        assert_eq!(prompt["fingerprint"], fingerprint, "both screens show the same code");
        assert_eq!(prompt["key"], self.id().await);
        assert_eq!(host.say(json!({"op": "decide", "id": session})).await, json!({"ok": true}));
        self.until("the newcomer is a member", |s| s["mesh"]["member"] == true && s["joining"].is_null()).await;
    }
}

/// The device named `name` in a state's device list.
pub fn device_named<'a>(state: &'a Value, name: &str) -> &'a Value {
    state["mesh"]["devices"]
        .as_array()
        .and_then(|devices| devices.iter().find(|d| d["name"] == name))
        .unwrap_or_else(|| panic!("no device {name} in {state:#}"))
}

/// The names in a state's device list, sorted.
pub fn device_names(state: &Value) -> Vec<String> {
    let mut names: Vec<String> = state["mesh"]["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

/// What the person has been told, oldest first.
pub fn events(state: &Value) -> Vec<String> {
    state["events"].as_array().into_iter().flatten().map(|e| e["text"].as_str().unwrap().to_string()).collect()
}

fn terminate(pid: i32) {
    std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status().unwrap();
}
