//! Helpers for HTTP-level tests.

use axum::body::Body;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{Method, Request, StatusCode};
use djinn_store::{now_ms, Role};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use crate::app;
use crate::state::AppState;

/// Sends one request through the full router. Returns the status and the
/// `name=value` part of any Set-Cookie.
pub async fn call(
    state: &AppState,
    method: Method,
    path: &str,
    body: Option<&str>,
    cookie: Option<&str>,
) -> (StatusCode, Option<String>) {
    let mut req = Request::builder().method(method).uri(path);
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    if let Some(cookie) = cookie {
        req = req.header(COOKIE, cookie);
    }
    let req = req
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
        .unwrap();
    let res = app(state.clone()).oneshot(req).await.unwrap();
    let cookie = res
        .headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::to_owned);
    (res.status(), cookie)
}

/// GETs `path` as the session in `cookie`; returns the status and body.
pub async fn get_body(state: &AppState, path: &str, cookie: &str) -> (StatusCode, String) {
    let (status, bytes) = send_bytes(state, Method::GET, path, Vec::new(), cookie).await;
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// Sends `body` as is to `path` as the session in `cookie`; returns the
/// status and the response body.
pub async fn send_bytes(
    state: &AppState,
    method: Method,
    path: &str,
    body: Vec<u8>,
    cookie: &str,
) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(COOKIE, cookie)
        .body(Body::from(body))
        .unwrap();
    let res = app(state.clone()).oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

/// Creates a user with a live session and returns its cookie.
pub async fn session(state: &AppState, username: &str, role: Role) -> String {
    let user = state
        .store
        .create_user(username, "unused", role)
        .await
        .unwrap();
    let token = format!("token-{username}");
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    state
        .store
        .create_session(user.id, &hash, now_ms() + 60_000)
        .await
        .unwrap();
    format!("djinn_session={token}")
}
