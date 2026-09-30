//! ErisAuth's plugin executable. ErisDB starts it once per call, as its
//! manifest (`deploy/erisauth.json`) says: `erisdb-plugin-erisauth --home DIR`.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = match (args.next().as_deref(), args.next()) {
        (Some("--home"), Some(dir)) => PathBuf::from(dir),
        _ => {
            eprintln!("usage: erisdb-plugin-erisauth --home DIR");
            std::process::exit(2);
        }
    };
    let mut line = String::new();
    let (status, body) = match std::io::stdin().lock().read_line(&mut line) {
        Err(e) => (500, json!({"error": "internal", "message": format!("reading the invocation: {e}")})),
        Ok(_) => match serde_json::from_str::<erisauth::plugin::Invocation>(&line) {
            Err(e) => (400, json!({"error": "bad_invocation", "message": e.to_string()})),
            Ok(invocation) => {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
                runtime.block_on(erisauth::plugin::answer(&dir, invocation)).unwrap_or_else(|e| {
                    // The operator's log gets the detail; the caller gets that it failed.
                    eprintln!("erisauth: {e:#}");
                    (500, json!({"error": "internal", "message": "home could not answer; see the ErisDB log"}))
                })
            }
        },
    };
    let mut out = std::io::stdout().lock();
    let head = json!({"protocol": 1, "status": status, "content_type": "application/json"});
    let _ = writeln!(out, "{head}").and_then(|_| write!(out, "{body}")).and_then(|_| out.flush());
}
