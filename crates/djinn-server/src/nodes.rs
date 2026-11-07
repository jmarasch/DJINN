//! Main's side of nodes: other DJINN boxes that play a room on their own
//! speakers, taking everything from here.
//!
//! Pairing: with joining switched on (`nodes_join`), a node connects over an
//! encrypted connection, notes this server's certificate, and asks to join
//! with a long random secret that only it knows; Main keeps only its hash.
//! Both screens show the same short code for the request, so the admin can
//! tell it's the right box, and the admin accepts with their password. From
//! then on the node signs every request with its secret (`Authorization:
//! Node <id>:<secret>`). Kicking deletes the node, so its next request is
//! refused as unknown, which is the node's cue to wipe its cache.

use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_store::{JoinRequest, Joined, Node};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::{AdminUser, CurrentSession};
use crate::client::{ClientIp, DirectTls};
use crate::error::{ApiError, ApiResult};
use crate::settings;
use crate::state::AppState;

/// Setting key: nodes may ask to join. Off unless "1".
pub const NODES_JOIN: &str = "nodes_join";
/// Join requests one address may send per limiter window.
const JOINS_PER_ADDRESS: usize = 10;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/nodes/join", post(join))
        .route("/nodes/me", get(me))
        .route("/nodes", get(list))
        .route("/nodes/settings", put(put_settings))
        .route("/nodes/{id}/accept", post(accept))
        .route("/nodes/{id}", axum::routing::patch(update).delete(remove))
}

/// SHA-256 of a secret, hex.
pub fn hash_secret(secret: &str) -> String {
    hex(&Sha256::digest(secret.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The code both screens show for a request: from the node's id and its
/// secret's hash, so the node works it out the same way.
pub fn pairing_code(node_id: &str, secret_hash: &str) -> String {
    let digest = hex(&Sha256::digest(
        format!("djinn-node:{node_id}:{secret_hash}").as_bytes(),
    ));
    format!(
        "{}-{}",
        digest[..4].to_uppercase(),
        digest[4..8].to_uppercase()
    )
}

/// Compares two strings in time that doesn't depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Node traffic only goes over the encrypted connection.
fn require_encrypted(parts_tls: bool) -> ApiResult<()> {
    if parts_tls {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UPGRADE_REQUIRED,
            "nodes connect over an encrypted connection",
        ))
    }
}

/// Encryption must be on while nodes can join or have joined.
pub async fn wants_encryption(state: &AppState) -> anyhow::Result<bool> {
    Ok(settings::flag(&state.store, NODES_JOIN, false).await? || state.store.has_nodes().await?)
}

fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[derive(Deserialize)]
struct JoinBody {
    node_id: String,
    name: String,
    secret: String,
    version: Option<String>,
    platform: Option<String>,
}

#[derive(Serialize)]
struct JoinAnswer {
    /// "pending" or "approved".
    status: &'static str,
    code: String,
    server_name: String,
    server_id: String,
}

async fn join(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    tls: Option<axum::Extension<DirectTls>>,
    Json(body): Json<JoinBody>,
) -> ApiResult<(StatusCode, Json<JoinAnswer>)> {
    require_encrypted(tls.is_some())?;
    if !settings::flag(&state.store, NODES_JOIN, false).await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "joining is turned off on this server. An admin can turn it on under Admin > Nodes.",
        ));
    }
    if state
        .limiter
        .allow("node-join", ip, JOINS_PER_ADDRESS)
        .is_err()
    {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many join requests; try again later",
        ));
    }
    let name = body.name.trim();
    if !valid_id(&body.node_id)
        || name.is_empty()
        || name.chars().count() > 60
        || !(32..=128).contains(&body.secret.len())
    {
        return Err(ApiError::bad_request("that isn't a valid join request"));
    }
    let secret_hash = hash_secret(&body.secret);
    let address = ip.map(|ip| ip.to_string());
    let joined = state
        .store
        .request_node_join(&JoinRequest {
            node_id: &body.node_id,
            name,
            secret_hash: &secret_hash,
            address: address.as_deref(),
            version: body.version.as_deref(),
            platform: body.platform.as_deref(),
        })
        .await?;
    let status = match joined {
        Joined::Pending => "pending",
        Joined::Approved => "approved",
        Joined::Taken => return Err(ApiError::conflict("that node id belongs to another node")),
        Joined::Full => {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too many nodes are waiting to join; ask the admin to answer them first",
            ))
        }
    };
    // Turning joining on already started encryption; a node joining keeps it.
    crate::go::apply_encryption(&state).await?;
    Ok((
        if status == "pending" {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        },
        Json(JoinAnswer {
            status,
            code: pairing_code(&body.node_id, &secret_hash),
            server_name: crate::go_announce::server_name(&state.store).await?,
            server_id: crate::go_announce::server_id(&state.store).await?,
        }),
    ))
}

/// A request signed by a node that has joined (or is waiting to).
pub struct NodeAuth(pub Node);

impl FromRequestParts<AppState> for NodeAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        require_encrypted(parts.extensions.get::<DirectTls>().is_some())?;
        let unknown = || ApiError::new(StatusCode::UNAUTHORIZED, "unknown node");
        let value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Node "))
            .ok_or_else(unknown)?;
        let (node_id, secret) = value.split_once(':').ok_or_else(unknown)?;
        let node = state
            .store
            .node_by_node_id(node_id)
            .await?
            .ok_or_else(unknown)?;
        if !same(&hash_secret(secret), &node.secret_hash) {
            return Err(unknown());
        }
        Ok(NodeAuth(node))
    }
}

#[derive(Serialize)]
struct MeAnswer {
    status: String,
    name: String,
    room_id: i64,
    server_name: String,
}

/// A node checking in: still waiting, accepted (and which room), or (401)
/// unknown, which means kicked or declined.
async fn me(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    NodeAuth(node): NodeAuth,
) -> ApiResult<Json<MeAnswer>> {
    let address = ip.map(|ip| ip.to_string());
    state
        .store
        .node_seen(node.id, address.as_deref(), None)
        .await?;
    Ok(Json(MeAnswer {
        status: node.status,
        name: node.name,
        room_id: node.room_id,
        server_name: crate::go_announce::server_name(&state.store).await?,
    }))
}

#[derive(Serialize)]
struct ListedNode {
    #[serde(flatten)]
    node: Node,
    /// The code the node's own screen shows, to compare.
    code: String,
}

#[derive(Serialize)]
struct NodeList {
    join_open: bool,
    /// This server's security code, which a node shows when it first connects.
    security_code: Option<String>,
    nodes: Vec<ListedNode>,
}

async fn list(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<NodeList>> {
    let nodes = state
        .store
        .nodes()
        .await?
        .into_iter()
        .map(|node| ListedNode {
            code: pairing_code(&node.node_id, &node.secret_hash),
            node,
        })
        .collect();
    Ok(Json(NodeList {
        join_open: settings::flag(&state.store, NODES_JOIN, false).await?,
        security_code: state
            .go
            .encryption
            .fingerprint()
            .map(|f| crate::tls::security_code(&f)),
        nodes,
    }))
}

#[derive(Deserialize)]
struct NodeSettings {
    join_open: bool,
}

async fn put_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NodeSettings>,
) -> ApiResult<StatusCode> {
    settings::set_flag(&state.store, NODES_JOIN, body.join_open).await?;
    crate::go::apply_encryption(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Password {
    password: String,
}

/// Accepting a node lets it play (and later cache) the library, so it takes
/// the admin's password, not just a session.
async fn accept(
    me: CurrentSession,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Password>,
) -> ApiResult<StatusCode> {
    if me.user.role != djinn_store::Role::Admin {
        return Err(ApiError::forbidden());
    }
    if !crate::auth::check_own_password(&state, &me.user, body.password).await? {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "wrong password"));
    }
    if !state.store.approve_node(id, me.user.id).await? {
        return Err(ApiError::not_found_with(
            "that request isn't waiting anymore",
        ));
    }
    tracing::info!(node = id, by = %me.user.username, "node accepted");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NodeChange {
    name: Option<String>,
    room_id: Option<i64>,
}

async fn update(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<NodeChange>,
) -> ApiResult<StatusCode> {
    if state.store.node(id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    if let Some(name) = body.name.as_deref().map(str::trim) {
        if name.is_empty() || name.chars().count() > 60 {
            return Err(ApiError::bad_request("the name needs 1 to 60 characters"));
        }
        state.store.rename_node(id, name).await?;
    }
    if let Some(room) = body.room_id {
        if !state.store.set_node_room(id, room).await? {
            return Err(ApiError::bad_request("there's no such room"));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Declines a request or kicks a node.
async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.remove_node(id).await? {
        return Err(ApiError::not_found());
    }
    tracing::info!(node = id, "node removed");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    /// Sends a request as it would come over DJINN's encrypted connection
    /// (or not), with an optional session cookie or node signature.
    async fn send(
        state: &AppState,
        method: Method,
        path: &str,
        body: Option<&str>,
        auth: Option<(&str, &str)>,
        encrypted: bool,
    ) -> (StatusCode, String) {
        let mut req = Request::builder().method(method).uri(path);
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some((name, value)) = auth {
            req = req.header(name, value);
        }
        let mut req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .unwrap();
        if encrypted {
            req.extensions_mut().insert(DirectTls);
        }
        let res = crate::app(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn admin_with_password(state: &AppState) -> String {
        use argon2::password_hash::PasswordHasher;
        let hash = argon2::Argon2::default()
            .hash_password(b"correct horse")
            .unwrap()
            .to_string();
        let user = state
            .store
            .create_user("boss", &hash, djinn_store::Role::Admin)
            .await
            .unwrap();
        let token = "token-boss";
        let token_hash = Sha256::digest(token.as_bytes()).to_vec();
        state
            .store
            .create_session(user.id, &token_hash, djinn_store::now_ms() + 60_000)
            .await
            .unwrap();
        format!("djinn_session={token}")
    }

    const SECRET: &str = "0123456789abcdef0123456789abcdef";
    const JOIN: &str = r#"{"node_id":"kitchen-0001","name":"Kitchen","secret":"0123456789abcdef0123456789abcdef","version":"2026.11.7","platform":"linux-aarch64"}"#;

    #[tokio::test]
    async fn a_node_joins_only_when_allowed_and_accepted_then_can_be_kicked() {
        let state = AppState::for_tests().await;
        let admin = admin_with_password(&state).await;
        let cookie = Some(("cookie", admin.as_str()));
        let signed = format!("Node kitchen-0001:{SECRET}");
        let node = Some(("authorization", signed.as_str()));

        // Off until the admin opens joining.
        let (status, _) = send(
            &state,
            Method::POST,
            "/api/nodes/join",
            Some(JOIN),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send(
            &state,
            Method::PUT,
            "/api/nodes/settings",
            Some(r#"{"join_open":true}"#),
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(state.go.encryption.is_on(), "joining needs encryption");

        // Never over plain HTTP.
        let (status, _) = send(
            &state,
            Method::POST,
            "/api/nodes/join",
            Some(JOIN),
            None,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::UPGRADE_REQUIRED);
        let (status, body) = send(
            &state,
            Method::POST,
            "/api/nodes/join",
            Some(JOIN),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let code = pairing_code("kitchen-0001", &hash_secret(SECRET));
        assert!(body.contains(&code), "the node is told its code: {body}");

        // Waiting, and the admin sees the same code.
        let (status, body) = send(&state, Method::GET, "/api/nodes/me", None, node, true).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#""status":"pending""#), "{body}");
        let (_, list) = send(&state, Method::GET, "/api/nodes", None, cookie, false).await;
        assert!(
            list.contains(&code) && !list.contains("secret_hash"),
            "{list}"
        );
        let wrong = format!("Node kitchen-0001:{}", "f".repeat(32));
        let (status, _) = send(
            &state,
            Method::GET,
            "/api/nodes/me",
            None,
            Some(("authorization", wrong.as_str())),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Accepting takes the password.
        let id = state
            .store
            .node_by_node_id("kitchen-0001")
            .await
            .unwrap()
            .unwrap()
            .id;
        let accept = format!("/api/nodes/{id}/accept");
        let (status, _) = send(
            &state,
            Method::POST,
            &accept,
            Some(r#"{"password":"nope"}"#),
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send(
            &state,
            Method::POST,
            &accept,
            Some(r#"{"password":"correct horse"}"#),
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = send(&state, Method::GET, "/api/nodes/me", None, node, true).await;
        assert!(body.contains(r#""status":"approved""#), "{body}");

        // Kicked: unknown from then on.
        let (status, _) = send(
            &state,
            Method::DELETE,
            &format!("/api/nodes/{id}"),
            None,
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = send(&state, Method::GET, "/api/nodes/me", None, node, true).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("unknown node"));
    }

    #[tokio::test]
    async fn only_admins_manage_nodes() {
        let state = AppState::for_tests().await;
        let sam = crate::test_util::session(&state, "sam", djinn_store::Role::User).await;
        let (status, _) = send(
            &state,
            Method::GET,
            "/api/nodes",
            None,
            Some(("cookie", sam.as_str())),
            false,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send(
            &state,
            Method::PUT,
            "/api/nodes/settings",
            Some(r#"{"join_open":true}"#),
            Some(("cookie", sam.as_str())),
            false,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn codes_and_hashes_are_stable() {
        let hash = hash_secret("s3cret");
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, hash_secret("s3cret"));
        let code = pairing_code("node-1234", &hash);
        assert_eq!(code.len(), 9);
        assert_eq!(&code[4..5], "-");
        assert_ne!(code, pairing_code("node-1235", &hash));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    #[test]
    fn ids_are_checked() {
        assert!(valid_id("a1b2c3d4-e5f6"));
        assert!(!valid_id("short"));
        assert!(!valid_id("has space in it"));
        assert!(!valid_id(&"x".repeat(65)));
    }
}
