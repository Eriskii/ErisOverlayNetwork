//! `erisauth approvals`: answer apps' requests at a terminal.

use std::collections::HashSet;
use std::io::{IsTerminal, Write};

use anyhow::{Context, Result, bail};
use erislogin::prompt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc::UnboundedReceiver;

use super::Paths;

/// Wrong passphrases the prompt accepts for one request before it leaves the request open.
const TRIES: usize = 3;

/// Send one line to the daemon and read its one-line answer.
async fn say(paths: &Paths, message: Value) -> Result<Value> {
    let stream =
        UnixStream::connect(paths.socket()).await.context("ErisAuth is not running; start `erisauth serve`")?;
    let (read, mut write) = stream.into_split();
    write.write_all(format!("{message}\n").as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    serde_json::from_str(&line).context("ErisAuth sent something unreadable")
}

/// Answer requests as they arrive; with `once`, stop after the first.
pub async fn run(paths: &Paths, once: bool) -> Result<()> {
    let stream =
        UnixStream::connect(paths.socket()).await.context("ErisAuth is not running; start `erisauth serve`")?;
    let (read, mut write) = stream.into_split();
    write.write_all(b"{\"op\":\"watch\"}\n").await?;
    let mut updates = BufReader::new(read).lines();
    let mut lines = prompt::stdin_lines();
    let mut seen = HashSet::new();
    let mut out = std::io::stdout();
    writeln!(out, "Waiting for apps to ask. Ctrl-C stops.")?;
    loop {
        let Some(update) = updates.next_line().await? else { bail!("ErisAuth stopped") };
        let update: Value = serde_json::from_str(&update)?;
        for approval in update["approvals"].as_array().into_iter().flatten() {
            let Some(id) = approval["id"].as_str() else { continue };
            if !seen.insert(id.to_string()) {
                continue;
            }
            prompt::drain(&mut lines);
            answer(paths, &mut lines, &mut out, approval).await?;
            if once {
                return Ok(());
            }
        }
    }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

async fn passphrase(lines: &mut UnboundedReceiver<String>, expires: i64) -> Result<String> {
    if std::io::stdin().is_terminal() {
        return Ok(tokio::task::spawn_blocking(|| rpassword::prompt_password("Passphrase: ")).await??);
    }
    print!("Passphrase: ");
    std::io::stdout().flush()?;
    Ok(prompt::answer_before(lines, expires).await?)
}

async fn answer(
    paths: &Paths,
    lines: &mut UnboundedReceiver<String>,
    out: &mut impl Write,
    approval: &Value,
) -> Result<()> {
    let id = &approval["id"];
    let expires = approval["expires"].as_i64().unwrap_or_else(erislogin::now);
    let asked = strings(&approval["requested"]);
    let program = approval["executable"].as_str().unwrap_or("an unknown program");
    let service = match approval["service"].as_str() {
        Some("erisdb") => "ErisDB",
        Some(other) => other,
        None => "a service",
    };
    writeln!(out, "\n{} asks to use {service}.", approval["client"].as_str().unwrap_or("An app"))?;
    match approval["pid"].as_i64() {
        Some(pid) => writeln!(out, "  program  {program} (pid {pid})")?,
        None => writeln!(out, "  program  {program}")?,
    }
    if approval["needs_passphrase"] == true {
        writeln!(out, "  This asks for more than this device approves by itself; approving takes the passphrase.")?;
    }
    writeln!(out, "  It wants:")?;
    let Some(granted) = prompt::choose(lines, out, &asked, expires).await? else {
        let answer = say(paths, json!({"op": "deny", "id": id})).await?;
        writeln!(out, "{}", if answer["ok"] == true { "Denied." } else { "It was already answered." })?;
        return Ok(());
    };
    let mut given = None;
    for _ in 0..=TRIES {
        let answer = say(paths, json!({"op": "decide", "id": id, "granted": granted, "passphrase": given})).await?;
        match answer["error"].as_str() {
            None => {
                writeln!(out, "Approved: {}", granted.join(", "))?;
                return Ok(());
            }
            Some("needs_passphrase") if given.is_none() => {}
            Some("wrong_passphrase") => writeln!(out, "That is not the passphrase.")?,
            Some("locked") => {
                writeln!(
                    out,
                    "Too many wrong passphrases from this device; try again in {} s.",
                    answer["retry_after"]
                )?;
                return Ok(());
            }
            Some(_) => {
                writeln!(out, "Not approved: {}", answer["message"].as_str().unwrap_or("ErisAuth refused"))?;
                return Ok(());
            }
        }
        given = Some(passphrase(lines, expires).await?);
    }
    writeln!(out, "The request stays open; answer it again with `erisauth approvals`.")?;
    Ok(())
}
