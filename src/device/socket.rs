//! Where apps, and the person's terminal, reach the device: `$XDG_RUNTIME_DIR/erisauth/agent.sock`,
//! readable only by this user.
//!
//! Each connection sends one JSON line. Apps speak the mesh library's
//! [agent protocol](erisoverlaynetwork::agent): `approve`, answered once the person has, with
//! hanging up withdrawing the request, and `snapshot`. The person's tools send
//! `{"op":"watch"}`, answered with the [state](super::command::state) at once and again on
//! every change until they hang up, or one of the [commands](super::command), each answered
//! once.
//!
//! Any process of this user can connect, as it can read the device's key.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use erisoverlaynetwork::agent::{MAX_LINE, Reply, Request, VERSION};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};

use super::Daemon;
use super::agent::Caller;

/// Bind the socket, replacing one left behind.
pub fn bind(path: &Path) -> Result<UnixListener> {
    super::private_dir(path.parent().context("the socket has a directory")?)?;
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let listener = UnixListener::bind(path).with_context(|| format!("listening on {}", path.display()))?;
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(listener)
}

/// Serve apps and the person on `listener` until the process ends.
pub async fn serve(listener: UnixListener, daemon: Arc<Daemon>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let daemon = daemon.clone();
        tokio::spawn(async move {
            // A peer that hangs up mid-answer has only stopped listening.
            if let Err(e) = connection(stream, daemon).await
                && !e.downcast_ref::<std::io::Error>().is_some_and(|e| {
                    matches!(e.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset)
                })
            {
                eprintln!("erisauth: {e:#}");
            }
        });
    }
}

async fn send(write: &mut OwnedWriteHalf, value: &impl serde::Serialize) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    write.write_all(&line).await?;
    Ok(())
}

/// The reply to an app's request: the agent protocol's own answer, or why it was not one.
pub async fn agent(daemon: &Daemon, message: Value, caller: Caller) -> Reply {
    let request: Request = match serde_json::from_value(message) {
        Ok(request) => request,
        Err(e) => return Reply::Error(format!("an unreadable request: {e}")),
    };
    match request {
        Request::Approve { v, .. } | Request::Snapshot { v } if v != VERSION => {
            Reply::Error(format!("this ErisAuth speaks version {VERSION}"))
        }
        Request::Approve { ask, .. } => daemon.ask(ask, caller).await,
        Request::Snapshot { .. } => match daemon.node.snapshot().await {
            Some(snapshot) => Reply::Snapshot(snapshot),
            None => Reply::Error("this device is in no mesh yet".into()),
        },
    }
}

async fn connection(stream: UnixStream, daemon: Arc<Daemon>) -> Result<()> {
    let caller = Caller::process(stream.peer_cred().ok().and_then(|cred| cred.pid()));
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    let mut line = String::new();
    if (&mut read).take(MAX_LINE).read_line(&mut line).await? == 0 {
        return Ok(());
    }
    let message: Value = match serde_json::from_str(&line) {
        Ok(message) => message,
        Err(e) => return send(&mut write, &Reply::Error(format!("an unreadable request: {e}"))).await,
    };
    let mut rest = [0u8; 1];
    match message["op"].as_str() {
        Some("approve" | "snapshot") => {
            // The app says nothing more; anything from it, or its hanging up, withdraws the request.
            tokio::select! {
                reply = agent(&daemon, message, caller) => send(&mut write, &reply).await,
                _ = read.read(&mut rest) => Ok(()),
            }
        }
        Some("watch") => {
            let mut changes = daemon.subscribe();
            loop {
                changes.borrow_and_update();
                send(&mut write, &super::command::state(&daemon)).await?;
                tokio::select! {
                    changed = changes.changed() => if changed.is_err() { return Ok(()) },
                    _ = read.read(&mut rest) => return Ok(()),
                }
            }
        }
        _ => send(&mut write, &super::command::run(&daemon, message).await).await,
    }
}

/// Send one command to the daemon at `socket` and read its answer.
pub async fn say(socket: &Path, message: &Value) -> Result<Value> {
    let stream = UnixStream::connect(socket).await.context("ErisAuth is not running; start `erisauth serve`")?;
    let (read, mut write) = stream.into_split();
    write.write_all(format!("{message}\n").as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(read).take(MAX_LINE).read_line(&mut line).await?;
    serde_json::from_str(&line).context("ErisAuth sent something unreadable")
}

/// The answer's refusal, as an error: `None` when it is no refusal.
pub fn refusal(answer: &Value) -> Option<anyhow::Error> {
    answer["error"].as_str().map(|code| match answer["message"].as_str() {
        Some(message) => anyhow::anyhow!("{message}"),
        None => anyhow::anyhow!("{code}"),
    })
}

/// A watch on the daemon: its state now and after every change, until it is dropped.
pub struct Watch {
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    // Hanging up ends the watch, so the write half lives as long as the watch.
    _write: OwnedWriteHalf,
}

impl Watch {
    pub async fn open(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket).await.context("ErisAuth is not running; start `erisauth serve`")?;
        let (read, mut write) = stream.into_split();
        write.write_all(format!("{}\n", json!({"op": "watch"})).as_bytes()).await?;
        Ok(Watch { lines: BufReader::new(read).lines(), _write: write })
    }

    /// The next state.
    pub async fn next(&mut self) -> Result<Value> {
        let line = self.lines.next_line().await?.context("ErisAuth stopped")?;
        serde_json::from_str(&line).context("ErisAuth sent something unreadable")
    }
}
