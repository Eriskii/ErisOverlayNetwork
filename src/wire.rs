//! Requests and answers over Iroh: one bi-stream each, JSON both ways.
//!
//! The asker writes its request and finishes the stream; the answerer writes an [`Answer`] —
//! the result, or a [`Refusal`] with an HTTP-style status — and finishes its side. A
//! connection carries as many as either side likes.

use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The most either side reads of one message.
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;

/// Why a participant would not answer: a status in HTTP's terms, and a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub status: u16,
    pub detail: String,
}

pub type Answer<T> = std::result::Result<T, Refusal>;

impl From<&Error> for Refusal {
    fn from(e: &Error) -> Self {
        let status = match e {
            Error::Unauthorized => 401,
            Error::Forbidden { .. } => 403,
            Error::BadRequest(_) => 400,
            Error::Conflict(_) => 409,
            Error::Expired => 410,
            Error::Cancelled(_) => 409,
            Error::Unreachable(_) => 502,
            Error::Io(_) | Error::Internal(_) => 500,
        };
        Refusal { status, detail: e.to_string() }
    }
}

impl From<Refusal> for Error {
    fn from(r: Refusal) -> Self {
        match r.status {
            401 => Error::Unauthorized,
            403 => Error::Forbidden { permission: r.detail },
            400 => Error::BadRequest(r.detail),
            409 => Error::Conflict(r.detail),
            410 => Error::Expired,
            _ => Error::Internal(r.detail),
        }
    }
}

/// Ask `conn`'s other side `request` and read its answer.
pub async fn ask<Q: Serialize, A: DeserializeOwned>(conn: &Connection, request: &Q) -> Result<A> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(unreachable)?;
    let bytes = serde_json::to_vec(request).map_err(|e| Error::Internal(e.to_string()))?;
    send.write_all(&bytes).await.map_err(unreachable)?;
    send.finish().map_err(unreachable)?;
    let reply = recv.read_to_end(MAX_MESSAGE).await.map_err(unreachable)?;
    let answer: Answer<A> = serde_json::from_slice(&reply)
        .map_err(|e| Error::Unreachable(format!("the other side answered something unreadable: {e}")))?;
    answer.map_err(Error::from)
}

/// Read one request from a stream the other side opened.
pub async fn request<Q: DeserializeOwned>(recv: &mut RecvStream) -> Result<Q> {
    let bytes = recv.read_to_end(MAX_MESSAGE).await.map_err(unreachable)?;
    serde_json::from_slice(&bytes).map_err(|e| Error::BadRequest(format!("an unreadable request: {e}")))
}

/// Write the answer to a request and finish the stream.
pub async fn reply<A: Serialize>(send: &mut SendStream, answer: Result<A>) -> Result<()> {
    let answer: Answer<A> = answer.map_err(|e| Refusal::from(&e));
    let bytes = serde_json::to_vec(&answer).map_err(|e| Error::Internal(e.to_string()))?;
    send.write_all(&bytes).await.map_err(unreachable)?;
    send.finish().map_err(unreachable)?;
    Ok(())
}

fn unreachable(e: impl std::fmt::Display) -> Error {
    Error::Unreachable(e.to_string())
}
