//! ErisOverlayNetwork at a terminal: every command but `serve` and `open` asks the running daemon over its
//! socket, and the person answers here what the page would put to them.

use std::collections::HashSet;
use std::io::{IsTerminal, Write};

use anyhow::{Context, Result, bail};
use erisoverlaynetwork::prompt;
use erisoverlaynetwork::ticket::Ticket;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedReceiver;

use super::Paths;
use super::socket::{Watch, refusal};

/// Wrong passphrases the terminal takes for one request before it leaves the request open.
const TRIES: usize = 3;

async fn say(paths: &Paths, message: Value) -> Result<Value> {
    let answer = super::socket::say(&paths.socket(), &message).await?;
    match refusal(&answer) {
        Some(e) => Err(e),
        None => Ok(answer),
    }
}

async fn state(paths: &Paths) -> Result<Value> {
    say(paths, json!({"op": "state"})).await
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("?")
}

/// Unix seconds `at`, as how long ago.
fn ago(at: i64) -> String {
    let secs = (erisoverlaynetwork::now() - at).max(0);
    let (count, unit) = match secs {
        ..60 => return "just now".into(),
        60..3600 => (secs / 60, "minute"),
        3600..86_400 => (secs / 3600, "hour"),
        _ => (secs / 86_400, "day"),
    };
    format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
}

/// This machine's name, for a device the person did not name.
pub fn hostname() -> String {
    erisoverlaynetwork::sys::hostname().unwrap_or_else(|| "this device".into())
}

/// Start a mesh with this device in it.
pub async fn init(paths: &Paths, name: &str) -> Result<()> {
    say(paths, json!({"op": "init", "name": name})).await?;
    println!("Started a mesh, with this device in it as {name}.");
    println!("Add your other devices from here with `eon add`.");
    Ok(())
}

/// Join the mesh of the device showing `ticket`, or the code standing in for it, and wait for the
/// person there to approve.
pub async fn join(paths: &Paths, name: &str, ticket: &str) -> Result<()> {
    let mut watch = Watch::open(&paths.socket()).await?;
    let joined = say(paths, json!({"op": "join", "ticket": ticket, "name": name})).await?;
    println!("Compare this code with {}'s screen: {}", text(&joined["host"]), text(&joined["fingerprint"]));
    println!("Waiting for it to approve.");
    loop {
        let state = watch.next().await?;
        if let Some(why) = state["joining"]["error"].as_str() {
            bail!("{why}");
        }
        if state["joining"].is_null() && state["mesh"]["member"] == true {
            println!("Joined, as {name}.");
            return Ok(());
        }
    }
}

/// Show a ticket to be added as `name`, for a device already in, a phone most likely, to scan;
/// then put its offer to the person.
pub async fn show_join(paths: &Paths, name: &str) -> Result<()> {
    let mut watch = Watch::open(&paths.socket()).await?;
    let shown = say(paths, json!({"op": "show_join", "name": name})).await?;
    let encoded = shown["ticket"].as_str().context("eon sent no ticket")?;
    let mut out = std::io::stdout();
    prompt::show(&mut out, &Ticket::parse(encoded)?, encoded, None)?;
    writeln!(out, "\nOr, on a device already in, run:\n\n  eon add '{encoded}'\n")?;
    writeln!(out, "Waiting for one of your devices to scan it. Ctrl-C stops.")?;
    let mut lines = prompt::stdin_lines();
    let mut asked = None;
    // The watch's first state may come from before the ticket was shown.
    let mut seen = false;
    loop {
        let state = watch.next().await?;
        let offer = &state["offer"];
        if state["mesh"]["member"] == true {
            println!("Joined, as {name}.");
            return Ok(());
        }
        if offer.is_null() {
            if seen {
                bail!("the ticket ended before any device added this one");
            }
            continue;
        }
        seen = true;
        if offer["status"] != "offered" || asked.as_ref() == Some(&offer["by"]) {
            continue;
        }
        asked = Some(offer["by"].clone());
        prompt::drain(&mut lines);
        let devices = strings(&offer["devices"]).join(", ");
        writeln!(out, "\n{} offers to add this device to your devices: {devices}.", text(&offer["by"]))?;
        writeln!(out, "Join them? [y/N]")?;
        out.flush()?;
        let expires = offer["expires"].as_i64().unwrap_or_else(erisoverlaynetwork::now);
        let yes = prompt::answer_before(&mut lines, expires).await?.trim().eq_ignore_ascii_case("y");
        if yes {
            say(paths, json!({"op": "accept_offer"})).await?;
            writeln!(out, "Accepted; {} is adding this device.", text(&offer["by"]))?;
        } else {
            say(paths, json!({"op": "decline_offer"})).await?;
            writeln!(out, "Turned down. The ticket is still shown for another device to scan.")?;
            asked = None;
        }
    }
}

/// Show a ticket for a new device, or with `app` for an app with no eon beside it, then put its
/// request to the person.
pub async fn add(paths: &Paths, app: bool) -> Result<()> {
    let mut watch = Watch::open(&paths.socket()).await?;
    let added = say(paths, json!({"op": if app { "add_app" } else { "add" }})).await?;
    let encoded = added["ticket"].as_str().context("eon sent no ticket")?;
    let session = added["id"].as_str().context("eon sent no id")?.to_string();
    let ticket = Ticket::parse(encoded)?;
    let mut out = std::io::stdout();
    prompt::show(&mut out, &ticket, encoded, added["code"].as_str())?;
    if app {
        writeln!(out, "\nOpen the app and give it this ticket.\n")?;
    } else {
        let code = added["code"].as_str().unwrap_or(encoded);
        writeln!(out, "\nOn the new device, run:\n\n  eon join --name NAME {code}\n")?;
    }
    writeln!(out, "Waiting for it to ask. Ctrl-C stops.")?;
    let mut lines = prompt::stdin_lines();
    // The watch's first state may come from before the ticket was opened.
    let mut opened = false;
    loop {
        let state = watch.next().await?;
        if let Some(prompt) = state["approvals"].as_array().into_iter().flatten().find(|p| p["id"] == session) {
            prompt::drain(&mut lines);
            return answer(paths, &mut lines, &mut out, prompt).await;
        }
        let open = state["tickets"].as_array().into_iter().flatten().any(|t| t["id"] == session);
        if opened && !open {
            bail!("the ticket ended before anyone asked with it");
        }
        opened |= open;
    }
}

/// Add the device showing the join ticket `ticket`: ask the person here, offer it the mesh, and
/// wait for its person to accept.
pub async fn invite(paths: &Paths, ticket: &str, yes: bool) -> Result<()> {
    let shown = Ticket::parse(ticket)?;
    let name = shown.name.clone().unwrap_or_default();
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("pass --yes to add a device without being asked");
        }
        prompt::prompt(&mut std::io::stdout(), &format!("Add {name} to your devices? [y/N] "))?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("Nothing added.");
            return Ok(());
        }
    }
    let mut watch = Watch::open(&paths.socket()).await?;
    say(paths, json!({"op": "invite", "ticket": ticket})).await?;
    println!("Waiting for {name} to accept, on its own screen.");
    loop {
        let state = watch.next().await?;
        let inviting = &state["inviting"];
        match inviting["status"].as_str() {
            Some("added") => {
                println!("Added {name}.");
                return Ok(());
            }
            Some("declined") => bail!("{name} turned it down"),
            Some("failed") => bail!("{}", text(&inviting["error"])),
            _ => {}
        }
    }
}

/// Answer requests as they arrive; with `once`, stop after the first.
pub async fn approvals(paths: &Paths, once: bool) -> Result<()> {
    let mut watch = Watch::open(&paths.socket()).await?;
    let mut out = std::io::stdout();
    writeln!(out, "Waiting for requests. Ctrl-C stops.")?;
    let mut lines = prompt::stdin_lines();
    let mut seen = HashSet::new();
    loop {
        let state = watch.next().await?;
        for prompt in state["approvals"].as_array().into_iter().flatten() {
            if prompt["busy"] == true || !seen.insert(prompt["id"].to_string()) {
                continue;
            }
            prompt::drain(&mut lines);
            answer(paths, &mut lines, &mut out, prompt).await?;
            if once {
                return Ok(());
            }
        }
    }
}

/// Approve or deny a request that asks for no grants: there is nothing to select.
async fn approve_or_deny(lines: &mut UnboundedReceiver<String>, out: &mut impl Write, expires: i64) -> Result<bool> {
    writeln!(out, "\n[a] approve   [d] deny")?;
    prompt::prompt(out, "> ")?;
    loop {
        match prompt::answer_before(lines, expires).await?.trim().to_ascii_lowercase().as_str() {
            "a" => return Ok(true),
            "d" => return Ok(false),
            _ => {
                writeln!(out, "Enter a or d.")?;
                prompt::prompt(out, "> ")?;
            }
        }
    }
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

async fn ask_passphrase(lines: &mut UnboundedReceiver<String>, expires: i64) -> Result<String> {
    if std::io::stdin().is_terminal() {
        return Ok(tokio::task::spawn_blocking(|| rpassword::prompt_password("Passphrase: ")).await??);
    }
    prompt::prompt(&mut std::io::stdout(), "Passphrase: ")?;
    Ok(prompt::answer_before(lines, expires).await?)
}

/// Put one request to the person and carry out their answer.
async fn answer(
    paths: &Paths,
    lines: &mut UnboundedReceiver<String>,
    out: &mut impl Write,
    prompt: &Value,
) -> Result<()> {
    let id = &prompt["id"];
    let name = text(&prompt["name"]);
    let expires = prompt["expires"].as_i64().unwrap_or_else(erisoverlaynetwork::now);
    if prompt["kind"] == "device" {
        writeln!(out, "\n{name} asks to join your devices.")?;
        writeln!(out, "  Compare this code with the new device's screen: {}", text(&prompt["fingerprint"]))?;
        writeln!(out, "  Approve only if both screens show the same code.")?;
        writeln!(out, "Approve {name}? [y/N]")?;
        out.flush()?;
        let yes = prompt::answer_before(lines, expires).await?.trim().eq_ignore_ascii_case("y");
        return decided(out, say(paths, json!({"op": if yes { "decide" } else { "deny" }, "id": id})).await, yes);
    }

    let uses: Vec<(String, Vec<String>)> = prompt["uses"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(capability, grants)| (capability.clone(), strings(grants)))
        .collect();
    let served: Vec<&Value> = prompt["serves"].as_array().into_iter().flatten().collect();
    let single: Vec<String> =
        served.iter().filter(|s| s["single"] == true).map(|s| your(text(&s["capability"]))).collect();
    let many: Vec<String> =
        served.iter().filter(|s| s["single"] != true).map(|s| text(&s["capability"]).to_string()).collect();
    if !single.is_empty() {
        writeln!(out, "\n{name} asks to be {}, for every device in the mesh.", single.join(" and "))?;
        for replaced in prompt["replaces"].as_array().into_iter().flatten() {
            writeln!(out, "  It replaces {} as {}.", text(&replaced["name"]), your(text(&replaced["capability"])))?;
        }
    } else if uses.is_empty() {
        writeln!(out, "\n{name} asks to serve {}.", many.join(" and "))?;
    } else {
        let used: Vec<String> = uses.iter().map(|(c, _)| your(c)).collect();
        writeln!(out, "\n{name} asks to use {}.", used.join(" and "))?;
        if !many.is_empty() {
            writeln!(out, "  It also serves {}.", many.join(" and "))?;
        }
    }
    match (prompt["program"].as_str(), prompt["fingerprint"].as_str()) {
        (_, Some(code)) => writeln!(out, "  Compare this code with the app's screen: {code}")?,
        (Some(program), None) => match prompt["pid"].as_i64() {
            Some(pid) => writeln!(out, "  program  {program} (pid {pid})")?,
            None => writeln!(out, "  program  {program}")?,
        },
        (None, None) => writeln!(out, "  program  unknown")?,
    }
    writeln!(out, "  key      {}", text(&prompt["key"]))?;
    if prompt["passphrase"] == true {
        writeln!(out, "  Approving takes a passphrase.")?;
    }
    // Every grant asked for, as one list; with more than one capability, each says where.
    let asked: Vec<(String, String)> =
        uses.iter().flat_map(|(c, grants)| grants.iter().map(|g| (c.clone(), g.clone()))).collect();
    let label = |(capability, grant): &(String, String)| {
        if uses.len() > 1 { format!("{grant} in {}", your(capability)) } else { grant.clone() }
    };
    let labels: Vec<String> = asked.iter().map(label).collect();
    let chosen = if asked.is_empty() {
        approve_or_deny(lines, out, expires).await?.then(Vec::new)
    } else {
        writeln!(out, "  It wants:")?;
        prompt::choose(lines, out, &labels, expires).await?
    };
    let Some(chosen) = chosen else {
        return decided(out, say(paths, json!({"op": "deny", "id": id})).await, false);
    };
    // The grants chosen in each capability; a capability whose grants all went is left out.
    let mut grants = serde_json::Map::new();
    for (capability, asked_there) in &uses {
        let kept: Vec<&String> = asked
            .iter()
            .zip(&labels)
            .filter(|((c, _), l)| c == capability && chosen.contains(l))
            .map(|((_, g), _)| g)
            .collect();
        if asked_there.is_empty() || !kept.is_empty() {
            grants.insert(capability.clone(), json!(kept));
        }
    }
    if prompt["passphrase"] != true {
        return decided(out, say(paths, json!({"op": "decide", "id": id, "grants": grants})).await, true);
    }
    for _ in 0..TRIES {
        let passphrase = ask_passphrase(lines, expires).await?;
        let answer = super::socket::say(
            &paths.socket(),
            &json!({"op": "decide", "id": id, "grants": grants, "passphrase": passphrase}),
        )
        .await?;
        match answer["error"].as_str() {
            None => {
                writeln!(out, "Approved.")?;
                return Ok(());
            }
            Some("wrong_passphrase") => writeln!(out, "That is not the passphrase.")?,
            Some(_) => {
                writeln!(out, "Not approved: {}", text(&answer["message"]))?;
                return Ok(());
            }
        }
    }
    writeln!(out, "The request stays open; answer it again with `eon approvals`.")?;
    Ok(())
}

/// A capability as a person says it: `database/1` is "your database".
fn your(capability: &str) -> String {
    format!("your {}", capability.split_once('/').map_or(capability, |(kind, _)| kind))
}

fn decided(out: &mut impl Write, answer: Result<Value>, approved: bool) -> Result<()> {
    match answer {
        Ok(_) => writeln!(out, "{}", if approved { "Approved." } else { "Denied." })?,
        Err(e) => writeln!(out, "Not answered: {e}")?,
    }
    Ok(())
}

/// The device a person means by `which`: its name, or the start of its id.
fn device<'a>(state: &'a Value, which: &str) -> Result<&'a Value> {
    let devices = state["mesh"]["devices"].as_array().context("this device is in no mesh")?;
    let named: Vec<&Value> = devices.iter().filter(|d| d["name"] == which).collect();
    let matches = if named.is_empty() {
        devices.iter().filter(|d| which.len() >= 4 && text(&d["id"]).starts_with(which)).collect()
    } else {
        named
    };
    match matches.as_slice() {
        [one] => Ok(one),
        [] => bail!("no device in the mesh is called {which:?} or has an id starting so"),
        _ => bail!("more than one device answers to {which:?}; give the start of its id"),
    }
}

/// List the mesh's devices.
pub async fn devices(paths: &Paths) -> Result<()> {
    let state = state(paths).await?;
    let devices = state["mesh"]["devices"].as_array().context("this device is in no mesh")?;
    for device in devices {
        let this = if device["this"] == true { " (this device)" } else { "" };
        let added = match device["added_by_name"].as_str() {
            Some(by) => format!("added by {by} {}", ago(device["added_at"].as_i64().unwrap_or_default())),
            None => format!("started the mesh {}", ago(device["added_at"].as_i64().unwrap_or_default())),
        };
        let reach = match device["reachable"].as_bool() {
            Some(true) => "reachable",
            Some(false) => "not reachable",
            None => "not asked yet",
        };
        println!("{}{this}\n  {added}; {reach}\n  {}", text(&device["name"]), text(&device["id"]));
    }
    for served in state["mesh"]["servers"].as_array().into_iter().flatten() {
        let capability = text(&served["capability"]);
        let apps: Vec<&Value> = served["apps"].as_array().into_iter().flatten().collect();
        let apps = if served["single"] == true { &apps[..apps.len().min(1)] } else { &apps[..] };
        for app in apps {
            let reach = match app["reachable"].as_bool() {
                Some(true) => "reachable",
                Some(false) => "not reachable",
                None => "not asked",
            };
            println!("{capability}: {}, approved on {}; {reach}", text(&app["name"]), text(&app["device"]));
        }
    }
    Ok(())
}

/// Call a device something new.
pub async fn rename(paths: &Paths, which: &str, name: &str) -> Result<()> {
    let state = state(paths).await?;
    let device = device(&state, which)?;
    say(paths, json!({"op": "rename", "device": device["id"], "name": name})).await?;
    println!("{} is now {name}.", text(&device["name"]));
    Ok(())
}

/// Remove a device and, unless the person keeps them, the devices it added.
pub async fn remove(paths: &Paths, which: &str, keep: &[String], yes: bool) -> Result<()> {
    let state = state(paths).await?;
    let device = device(&state, which)?;
    let offer = say(paths, json!({"op": "removal", "device": device["id"]})).await?;
    let mut kept = Vec::new();
    for which in keep {
        kept.push(self::device(&state, which)?["id"].clone());
    }
    let also: Vec<&Value> = offer["also"].as_array().into_iter().flatten().collect();
    let (keeping, going): (Vec<&Value>, Vec<&Value>) = also.into_iter().partition(|d| kept.contains(&d["id"]));
    let names =
        |devices: &[&Value]| devices.iter().map(|d| text(&d["name"]).to_string()).collect::<Vec<_>>().join(", ");
    println!("Removing {}.", text(&device["name"]));
    if !going.is_empty() {
        println!("  With the devices it added: {}", names(&going));
    }
    if !keeping.is_empty() {
        println!("  Keeping: {}", names(&keeping));
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("pass --yes to remove devices without being asked");
        }
        prompt::prompt(&mut std::io::stdout(), "Remove them for good? [y/N] ")?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("Nothing removed.");
            return Ok(());
        }
    }
    let devices: Vec<&Value> = std::iter::once(&device["id"]).chain(going.iter().map(|d| &d["id"])).collect();
    say(paths, json!({"op": "remove", "devices": devices})).await?;
    println!("Removed.");
    Ok(())
}

/// List the apps this device approved.
pub async fn apps(paths: &Paths) -> Result<()> {
    let state = state(paths).await?;
    for note in state["notes"].as_array().into_iter().flatten() {
        let standing = if note["revoked"] == true {
            " (revoked)"
        } else if note["counts"] != true {
            " (no longer counts)"
        } else {
            ""
        };
        println!("{}{standing}, approved {}", text(&note["name"]), ago(note["at"].as_i64().unwrap_or_default()));
        for (capability, grants) in note["uses"].as_object().into_iter().flatten() {
            let grants = strings(grants);
            let grants = if grants.is_empty() { String::new() } else { format!(": {}", grants.join(", ")) };
            println!("  uses     {capability}{grants}");
        }
        for served in note["serves"].as_array().into_iter().flatten() {
            println!("  serves   {}", text(&served["capability"]));
        }
        if let Some(program) = note["program"].as_str() {
            println!("  program  {program}");
        }
        println!("  note     {}", text(&note["id"]));
    }
    Ok(())
}

/// Revoke an app's note: by the start of its id, or by the app's name.
pub async fn revoke(paths: &Paths, which: &str) -> Result<()> {
    let state = state(paths).await?;
    let notes: Vec<&Value> = state["notes"].as_array().into_iter().flatten().filter(|n| n["revoked"] != true).collect();
    let named: Vec<&&Value> = notes.iter().filter(|n| n["name"] == which).collect();
    let matches: Vec<&&Value> = if named.is_empty() {
        notes.iter().filter(|n| which.len() >= 4 && text(&n["id"]).starts_with(which)).collect()
    } else {
        named
    };
    let note = match matches.as_slice() {
        [one] => one,
        [] => bail!("no approval of this device's is called {which:?} or has a note id starting so"),
        _ => bail!("more than one approval answers to {which:?}; give the start of its note id"),
    };
    say(paths, json!({"op": "revoke", "note": note["id"]})).await?;
    println!("{} is revoked, everywhere.", text(&note["name"]));
    Ok(())
}

/// Change the passphrase of the server of `capability`, or of the one server that takes a
/// passphrase, typing the current one.
pub async fn passphrase(paths: &Paths, capability: Option<&str>) -> Result<()> {
    let current = secret("Current passphrase: ")?;
    let new = if std::io::stdin().is_terminal() {
        loop {
            let first = secret("New passphrase: ")?;
            if secret("Again: ")? == first {
                break first;
            }
            eprintln!("Those differ; once more.");
        }
    } else {
        secret("")?
    };
    say(paths, json!({"op": "passphrase", "capability": capability, "current": current, "new": new})).await?;
    println!("The passphrase is changed.");
    Ok(())
}
