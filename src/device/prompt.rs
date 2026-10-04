//! Answering at a terminal: `erisauth approvals` for requests as they
//! arrive, `erisauth add` to show a ticket a new device joins with, and
//! `erisauth passphrase` and `erisauth unlock` to look after the passphrase.

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

async fn connect(paths: &Paths) -> Result<UnixStream> {
    UnixStream::connect(paths.socket()).await.context("ErisAuth is not running; start `erisauth serve`")
}

/// Send one line to the daemon and read its one-line answer.
async fn say(paths: &Paths, message: Value) -> Result<Value> {
    let (read, mut write) = connect(paths).await?.into_split();
    write.write_all(format!("{message}\n").as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    serde_json::from_str(&line).context("ErisAuth sent something unreadable")
}

/// Answer requests as they arrive; with `once`, stop after the first.
pub async fn run(paths: &Paths, once: bool) -> Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "Waiting for requests. Ctrl-C stops.")?;
    answer_each(paths, &mut out, None, once).await
}

/// Show a ticket for a new device, then answer its request.
pub async fn add(paths: &Paths) -> Result<()> {
    let added = say(paths, json!({"op": "add"})).await?;
    if let Some(why) = added["message"].as_str() {
        bail!("{why}");
    }
    let encoded = added["ticket"].as_str().context("ErisAuth sent no ticket")?;
    let id = added["id"].as_str().context("ErisAuth sent no id")?.to_string();
    let ticket = erislogin::ticket::Ticket::parse("erisdb", encoded)?;
    let mut out = std::io::stdout();
    prompt::show(&mut out, "ErisDB", &ticket, encoded)?;
    writeln!(out, "\nOn the new device, run:\n\n  erisauth join '{encoded}'\n")?;
    writeln!(out, "Waiting for it to ask. Ctrl-C stops.")?;
    answer_each(paths, &mut out, Some(id), true).await
}

/// A passphrase typed at the terminal without echo, or one line of piped input.
fn secret(label: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    Ok(rpassword::prompt_password(label)?)
}

/// A new passphrase: typed twice at a terminal, or one line of piped input.
fn new_secret() -> Result<String> {
    if !std::io::stdin().is_terminal() {
        return secret("");
    }
    loop {
        let first = secret("New passphrase: ")?;
        if secret("Again: ")? == first {
            return Ok(first);
        }
        eprintln!("Those differ; once more.");
    }
}

/// Change the passphrase, or with `reset`, set it without the current one.
pub async fn passphrase(paths: &Paths, reset: bool) -> Result<()> {
    let current = if reset { None } else { Some(secret("Current passphrase: ")?) };
    let new = new_secret()?;
    let answer = say(paths, json!({"op": "passphrase", "current": current, "new": new})).await?;
    if let Some(why) = answer["message"].as_str() {
        bail!("{why}");
    }
    println!("The passphrase is set.");
    Ok(())
}

/// Let a device, or every device, guess the passphrase again now.
pub async fn unlock(paths: &Paths, installation: Option<uuid::Uuid>) -> Result<()> {
    let answer = say(paths, json!({"op": "unlock", "installation": installation})).await?;
    if let Some(why) = answer["message"].as_str() {
        bail!("{why}");
    }
    match installation {
        Some(id) => println!("{id} may guess again."),
        None => println!("Every device may guess again."),
    }
    Ok(())
}

/// Put each waiting request to the person, or only request `only`.
async fn answer_each(paths: &Paths, out: &mut impl Write, only: Option<String>, once: bool) -> Result<()> {
    let (read, mut write) = connect(paths).await?.into_split();
    write.write_all(b"{\"op\":\"watch\"}\n").await?;
    let mut updates = BufReader::new(read).lines();
    let mut lines = prompt::stdin_lines();
    let mut seen = HashSet::new();
    loop {
        let Some(update) = updates.next_line().await? else { bail!("ErisAuth stopped") };
        let update: Value = serde_json::from_str(&update)?;
        for approval in update["approvals"].as_array().into_iter().flatten() {
            let Some(id) = approval["id"].as_str() else { continue };
            if only.as_deref().is_some_and(|only| only != id) || !seen.insert(id.to_string()) {
                continue;
            }
            prompt::drain(&mut lines);
            answer(paths, &mut lines, out, approval).await?;
            if once {
                return Ok(());
            }
        }
    }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

async fn ask_passphrase(lines: &mut UnboundedReceiver<String>, expires: i64) -> Result<String> {
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
    let service = match approval["service"].as_str() {
        Some("erisdb") => "ErisDB",
        Some(other) => other,
        None => "a service",
    };
    if approval["kind"] == "device" {
        writeln!(out, "\n{} asks to join {service}.", approval["client"].as_str().unwrap_or("A device"))?;
        writeln!(out, "  Compare this code with the new device: {}", approval["fingerprint"].as_str().unwrap_or("?"))?;
        writeln!(out, "  Approve only if both screens show the same code.")?;
    } else {
        let program = approval["executable"].as_str().unwrap_or("an unknown program");
        writeln!(out, "\n{} asks to use {service}.", approval["client"].as_str().unwrap_or("An app"))?;
        match approval["pid"].as_i64() {
            Some(pid) => writeln!(out, "  program  {program} (pid {pid})")?,
            None => writeln!(out, "  program  {program}")?,
        }
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
                let wait = answer["retry_after"].as_i64().unwrap_or_default();
                writeln!(out, "Too many wrong passphrases from this device; try again in {wait} s.")?;
                return Ok(());
            }
            Some(_) => {
                writeln!(out, "Not approved: {}", answer["message"].as_str().unwrap_or("ErisAuth refused"))?;
                return Ok(());
            }
        }
        given = Some(ask_passphrase(lines, expires).await?);
    }
    writeln!(out, "The request stays open; answer it again with `erisauth approvals`.")?;
    Ok(())
}
