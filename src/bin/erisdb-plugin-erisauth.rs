//! ErisAuth's plugin executable. ErisDB starts it once per call, as its
//! manifest (`deploy/erisauth.json`) says, and hands it the invocation on
//! stdin, with the core's address and a capability for this call.

use std::io::{BufRead, Write};

use serde_json::json;

fn main() {
    let mut line = String::new();
    let (status, body) = match std::io::stdin().lock().read_line(&mut line) {
        Err(e) => (500, json!({"error": "internal", "message": format!("reading the invocation: {e}")})),
        Ok(_) => match serde_json::from_str::<erisauth::plugin::Invocation>(&line) {
            Err(e) => (400, json!({"error": "bad_invocation", "message": e.to_string()})),
            Ok(invocation) => {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
                runtime.block_on(erisauth::plugin::answer(invocation)).unwrap_or_else(|e| {
                    // The operator's log gets the detail; the caller gets that it failed.
                    eprintln!("erisauth: {e:#}");
                    (500, json!({"error": "internal", "message": "ErisAuth could not answer; see the ErisDB log"}))
                })
            }
        },
    };
    let mut out = std::io::stdout().lock();
    let head = json!({"protocol": 1, "status": status, "content_type": "application/json"});
    let _ = writeln!(out, "{head}").and_then(|_| write!(out, "{body}")).and_then(|_| out.flush());
}
