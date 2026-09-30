//! Where apps, and the person's prompt and page, reach the device:
//! `$XDG_RUNTIME_DIR/erisauth/agent.sock`, readable only by this user.
//!
//! Each connection sends one JSON line. Apps send ErisLogin's
//! [`pair`](erislogin::agent) request and hold the connection open until it
//! is answered; hanging up withdraws it. The person's tools send:
//!
//! ```text
//! {"op":"approvals"}                                  → {"approvals":[…]}
//! {"op":"watch"}                                      → {"approvals":[…]} on every change
//! {"op":"decide","id":…,"granted":[…],"passphrase":…} → {"ok":true} or {"error":…}
//! {"op":"deny","id":…}                                → {"ok":true} or {"error":…}
//! ```
//!
//! Any process of this user can connect, as it can read the device's key.
//! What the device approves by itself is within its ceiling either way; the
//! passphrase stands between a process and anything beyond it.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use erislogin::agent::{MAX_LINE, Reply, Request};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use uuid::Uuid;

use super::agent::{Agent, Peer};

/// Bind the socket, replacing one left behind.
pub fn bind(path: &Path) -> Result<UnixListener> {
    let dir = path.parent().context("the socket has a directory")?;
    super::private_dir(dir)?;
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let listener = UnixListener::bind(path).with_context(|| format!("listening on {}", path.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(listener)
}

/// Serve apps and the person on `listener` until the process ends.
pub async fn serve(listener: UnixListener, agent: Arc<Agent>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let agent = agent.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(stream, agent).await {
                eprintln!("erisauth: {e:#}");
            }
        });
    }
}

fn peer(stream: &UnixStream) -> Peer {
    let pid = stream.peer_cred().ok().and_then(|cred| cred.pid());
    let executable = pid.and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok());
    Peer { pid, executable }
}

async fn send(write: &mut OwnedWriteHalf, value: &impl serde::Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    write.write_all(&line).await?;
    Ok(())
}

#[derive(Deserialize)]
struct Decide {
    id: Uuid,
    granted: Vec<String>,
    #[serde(default)]
    passphrase: Option<String>,
}

#[derive(Deserialize)]
struct Deny {
    id: Uuid,
}

async fn connection(stream: UnixStream, agent: Arc<Agent>) -> Result<()> {
    let peer = peer(&stream);
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    let mut line = String::new();
    (&mut read).take(MAX_LINE).read_line(&mut line).await?;
    let message: Value = match serde_json::from_str(&line) {
        Ok(message) => message,
        Err(e) => return send(&mut write, &json!({"error": format!("unreadable request: {e}")})).await,
    };
    match message["op"].as_str() {
        Some("pair") => {
            let request: Request = match serde_json::from_value(message) {
                Ok(request) => request,
                Err(e) => return send(&mut write, &Reply::Error(format!("unreadable request: {e}"))).await,
            };
            if request.v != erislogin::agent::VERSION {
                let why = format!("this ErisAuth speaks version {}", erislogin::agent::VERSION);
                return send(&mut write, &Reply::Error(why)).await;
            }
            let (id, ticket) = match agent.open(&request.ask, peer).await {
                Ok(opened) => opened,
                Err(why) => return send(&mut write, &Reply::Error(why)).await,
            };
            send(&mut write, &Reply::Ticket(ticket)).await?;
            // The app says nothing more; anything from it, or its hanging up, withdraws the request.
            let mut rest = [0u8; 1];
            tokio::select! {
                reply = agent.follow(id) => send(&mut write, &reply).await,
                _ = read.read(&mut rest) => {
                    agent.withdraw(id).await;
                    Ok(())
                }
            }
        }
        Some("approvals") => send(&mut write, &json!({"approvals": agent.approvals()})).await,
        Some("watch") => {
            let mut changes = agent.subscribe();
            let mut rest = [0u8; 1];
            loop {
                send(&mut write, &json!({"approvals": agent.approvals()})).await?;
                tokio::select! {
                    changed = changes.changed() => if changed.is_err() { return Ok(()) },
                    _ = read.read(&mut rest) => return Ok(()),
                }
            }
        }
        Some("decide") => match serde_json::from_value::<Decide>(message) {
            Ok(d) => send(&mut write, &agent.approve(d.id, d.granted, d.passphrase).await).await,
            Err(e) => send(&mut write, &json!({"error": "bad_request", "message": e.to_string()})).await,
        },
        Some("deny") => match serde_json::from_value::<Deny>(message) {
            Ok(d) => send(&mut write, &agent.deny(d.id)).await,
            Err(e) => send(&mut write, &json!({"error": "bad_request", "message": e.to_string()})).await,
        },
        other => send(&mut write, &json!({"error": "bad_request", "message": format!("no operation {other:?}")})).await,
    }
}
