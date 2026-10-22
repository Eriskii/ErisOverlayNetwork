//! The approval page: a SvelteKit app built by `build.rs` from `web/`, embedded in the binary
//! and served by the daemon on loopback.
//!
//! The page's files hold no data and need no token. Its API does: a token the daemon makes when
//! it starts and hands over in the link it writes to `$XDG_RUNTIME_DIR/eon/page`, readable
//! only by this user, in the URL fragment, which the browser never sends.
//!
//! ```text
//! GET  /api/state     the state, as the commands describe it
//! POST /api/command   one command, answered as on the socket
//! ```

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde_json::{Value, json};

use super::Daemon;

pub(super) struct Asset {
    path: &'static str,
    kind: &'static str,
    body: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

const POLICY: &str = "frame-ancestors 'none'; base-uri 'none'; form-action 'none'; object-src 'none'";

#[derive(Clone)]
struct Page {
    daemon: Arc<Daemon>,
    token: Arc<String>,
}

/// Where the daemon writes the page's link under the runtime directory.
pub fn link_path(runtime: &Path) -> std::path::PathBuf {
    runtime.join("eon").join("page")
}

/// Serve the page on a free loopback port; returns its link, token included, once it is
/// listening, and serves until the process ends.
pub async fn listen(
    runtime: &Path,
    daemon: Arc<Daemon>,
) -> Result<(String, impl std::future::Future<Output = Result<()>> + use<>)> {
    let token = B64.encode(rand::random::<[u8; 32]>());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.context("binding the approval page")?;
    let link = format!("http://{}/#token={token}", listener.local_addr()?);
    let path = link_path(runtime);
    super::private_dir(path.parent().context("the link has a directory")?)?;
    erisoverlaynetwork::mesh::write_private(&path, format!("{link}\n").as_bytes())?;

    let page = Page { daemon, token: Arc::new(token) };
    let api = Router::new()
        .route("/api/state", get(state))
        .route("/api/command", post(command))
        .route_layer(middleware::from_fn_with_state(page.clone(), authorize))
        .with_state(page);
    let app = Router::new()
        .route("/", get(|| async { serve("index.html") }))
        .route("/{file}", get(|UrlPath(file): UrlPath<String>| async move { serve(&file) }))
        .route("/ui/{*path}", get(|UrlPath(path): UrlPath<String>| async move { serve(&format!("ui/{path}")) }))
        .merge(api);
    Ok((link, async move { Ok(axum::serve(listener, app).await?) }))
}

async fn authorize(State(page): State<Page>, request: Request, next: Next) -> Response {
    use subtle::ConstantTimeEq;
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if bool::from(given.as_bytes().ct_eq(page.token.as_bytes())) {
        let mut response = next.run(request).await;
        response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized", "message": "open this page with `eon open`"})))
            .into_response()
    }
}

async fn state(State(page): State<Page>) -> Json<Value> {
    Json(super::command::state(&page.daemon))
}

async fn command(State(page): State<Page>, Json(message): Json<Value>) -> Json<Value> {
    Json(super::command::run(&page.daemon, message).await)
}

fn serve(path: &str) -> Response {
    let Some(asset) = ASSETS.iter().find(|asset| asset.path == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = asset.body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(asset.kind));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(POLICY));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    // Hashed file names never change content; the page and its icon follow the binary.
    let cache = if path.starts_with("ui/immutable/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}
