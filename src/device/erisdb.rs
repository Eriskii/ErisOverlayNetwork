//! Speaking to the mesh's ErisDB: HTTP/1.1 on a QUIC bi-stream under ALPN `erisdb/0`, from this
//! device's own endpoint, so that ErisDB knows the device by its key. A device asks two things
//! of it: to enroll an app's note with the passphrase, and to change the passphrase.

use anyhow::{Context, Result};
use erisoverlaynetwork::mesh::Node;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use iroh::endpoint::Connection;
use serde_json::{Value, json};

use super::agent::{ERISDB, failed};

/// ErisDB's ALPN.
pub const ALPN: &[u8] = b"erisdb/0";

/// One request on a bi-stream of its own: the response's status and its JSON body.
pub async fn request(conn: &Connection, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value)> {
    let (send, recv) = conn.open_bi().await.context("opening a stream to ErisDB")?;
    let (mut sender, driver) = hyper::client::conn::http1::handshake(TokioIo::new(tokio::io::join(recv, send))).await?;
    let driver = tokio::spawn(driver);
    let mut request = hyper::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "erisdb")
        .header("accept", "application/json");
    let payload = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Bytes::from(serde_json::to_vec(body)?)
        }
        None => Bytes::new(),
    };
    let response = sender.send_request(request.body(Full::new(payload))?).await.context("asking ErisDB")?;
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.context("reading ErisDB's answer")?.to_bytes();
    driver.abort();
    Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
}

/// POST `body` to `path` at the mesh's ErisDB, as this device.
pub async fn post(node: &Node, path: &str, body: &Value) -> Result<(u16, Value)> {
    let erisdb = node.provider(ERISDB).context("this mesh's ErisDB has not been heard from")?;
    let conn = node.connect(erisdb.id, ALPN).await?;
    let answer = request(&conn, "POST", path, Some(body)).await;
    conn.close(0u32.into(), b"done");
    answer
}

/// What ErisDB refused, as the person's screens show it: `{error, message, retry_after?}`.
pub fn refusal(status: u16, body: &Value) -> Value {
    let code = body["error"].as_str().unwrap_or("refused");
    let said = body["message"].as_str().or(body["detail"].as_str());
    match code {
        "wrong_passphrase" => failed(code, "That is not the passphrase."),
        "locked" => {
            let wait = body["retry_after"].as_i64().unwrap_or(60).max(1);
            let mut refused =
                failed(code, format!("Too many wrong passphrases from this device; try again in {}.", how_long(wait)));
            refused["retry_after"] = json!(wait);
            refused
        }
        "no_passphrase" => {
            failed(code, "ErisDB has no passphrase yet; set one where it runs, with `erisdb passphrase set`.")
        }
        "not_a_device" => failed(code, "ErisDB does not count this device among the mesh's devices."),
        _ => failed(code, said.map(str::to_string).unwrap_or_else(|| format!("ErisDB refused ({status}): {body}"))),
    }
}

/// `seconds` as a person says it.
fn how_long(seconds: i64) -> String {
    let (count, unit) = match seconds {
        ..60 => (seconds, "second"),
        60..3600 => ((seconds + 59) / 60, "minute"),
        _ => ((seconds + 3599) / 3600, "hour"),
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

impl super::Daemon {
    /// Change ErisDB's passphrase, from this device.
    pub async fn change_passphrase(&self, current: &str, new: &str) -> Value {
        if new.chars().count() < 8 {
            return failed("bad_request", "a passphrase has at least 8 characters");
        }
        if self.node.provider(ERISDB).is_none() {
            return self.no_erisdb();
        }
        match post(&self.node, "/v1/passphrase", &json!({"current": current, "new": new})).await {
            Ok((status, _)) if (200..300).contains(&status) => json!({"ok": true}),
            Ok((status, body)) => refusal(status, &body),
            Err(e) => failed("unreachable", format!("reaching this mesh's ErisDB: {e:#}")),
        }
    }
}
