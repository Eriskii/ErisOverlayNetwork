//! The host's side of the ceremony in a terminal: show the ticket, then put
//! a request to the person and read their answer.

use std::io::Write;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::error::{Error, Result};
use crate::qr;
use crate::ticket::Ticket;

/// What a person is asked to answer.
#[derive(Debug, Clone)]
pub struct Request {
    /// The redeemer's name for itself.
    pub client: String,
    pub fingerprint: String,
    pub asked: Vec<String>,
    /// Unix seconds past which no answer counts.
    pub expires: i64,
}

/// Print a ticket every way a person might want it: as a code for a camera,
/// and as text for anyone who would rather type.
pub fn show(out: &mut impl Write, ticket: &Ticket, encoded: &str) -> std::io::Result<()> {
    match qr::to_blocks(encoded) {
        Ok(blocks) => writeln!(out, "\n{blocks}")?,
        // Too much data for any QR version is the only realistic failure,
        // and it should not cost the operator the ticket.
        Err(e) => writeln!(out, "(the ticket {e})\n")?,
    }
    writeln!(out, "Scan this with the new device, or with your phone's camera.\n")?;
    if let Some(name) = &ticket.name {
        writeln!(out, "  device      {name}")?;
    }
    writeln!(out, "  endpoint id {}", ticket.eid)?;
    if let Some(relay) = &ticket.relay {
        writeln!(out, "  relay       {relay}")?;
    }
    writeln!(out, "  code        {}", ticket.token)?;
    writeln!(out, "\n  ticket      {encoded}")?;
    writeln!(out, "\nThe code grants nothing on its own — you approve what asks to join.")
}

/// Lines typed on stdin, read off a blocking thread so that waiting and
/// typing can happen at once.
pub fn stdin_lines() -> UnboundedReceiver<String> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines().map_while(std::result::Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    rx
}

/// Write `text` with no newline and flush it, as a prompt for the next line.
pub fn prompt(out: &mut impl Write, text: &str) -> Result<()> {
    write!(out, "{text}")?;
    out.flush()?;
    Ok(())
}

/// The next line, unless input closes, ctrl-c arrives, or `expires` passes.
pub async fn answer_before(lines: &mut UnboundedReceiver<String>, expires: i64) -> Result<String> {
    let remaining = expires.saturating_sub(crate::now()).max(0) as u64;
    tokio::select! {
        line = lines.recv() => line.ok_or_else(|| Error::Cancelled("cancelled: input closed".into())),
        _ = tokio::signal::ctrl_c() => Err(Error::Cancelled("cancelled".into())),
        _ = tokio::time::sleep(std::time::Duration::from_secs(remaining)) => Err(Error::Cancelled("expired".into())),
    }
}

/// Put `request` to the person: show the fingerprint to compare and what is
/// asked, then [`choose`]. Returns the grants approved, or `None` for a denial.
pub async fn decide(
    lines: &mut UnboundedReceiver<String>,
    out: &mut impl Write,
    request: &Request,
) -> Result<Option<Vec<String>>> {
    writeln!(out, "\n\nCompare this code with the other screen: {}", request.fingerprint)?;
    writeln!(out, "Approve only if both displays match.\n{} wants:", request.client)?;
    choose(lines, out, &request.asked, request.expires).await
}

/// List `asked`, then let the person approve it as asked, select some, or
/// deny, before `expires`. Returns the grants approved, or `None` for a
/// denial.
///
/// Blank or unrecognized input asks again. Only `d`, or an explicitly empty
/// selection after `s`, denies; invalid numbers ask again rather than
/// approving part of a selection. There is no grant-everything shortcut.
pub async fn choose(
    lines: &mut UnboundedReceiver<String>,
    out: &mut impl Write,
    asked: &[String],
    expires: i64,
) -> Result<Option<Vec<String>>> {
    for (i, grant) in asked.iter().enumerate() {
        writeln!(out, "  {}. {grant}", i + 1)?;
    }
    writeln!(out, "\n[a] approve as asked   [s] select   [d] deny")?;
    prompt(out, "> ")?;
    loop {
        let answer = answer_before(lines, expires).await?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "a" => return Ok(Some(asked.to_vec())),
            "d" => return Ok(None),
            "s" => loop {
                writeln!(out, "Numbers to approve, comma separated (empty denies):")?;
                prompt(out, "> ")?;
                let picks = answer_before(lines, expires).await?;
                if picks.trim().is_empty() {
                    return Ok(None);
                }
                let grants: Option<Vec<String>> = picks
                    .split(',')
                    .map(|p| {
                        let index = p.trim().parse::<usize>().ok()?.checked_sub(1)?;
                        asked.get(index).cloned()
                    })
                    .collect();
                if let Some(grants) = grants {
                    return Ok(Some(grants));
                }
                writeln!(out, "Enter valid permission numbers from 1 to {}.", asked.len())?;
            },
            _ => {
                writeln!(out, "Enter a, s, or d.")?;
                prompt(out, "> ")?;
            }
        }
    }
}

/// Drop whatever was typed before a request appeared. Keystrokes entered
/// while waiting are not an answer to a request the person has not seen; in
/// particular, a queued newline must not deny it. Call this before [`decide`] or [`choose`].
pub fn drain(lines: &mut UnboundedReceiver<String>) {
    while lines.try_recv().is_ok() {}
}
