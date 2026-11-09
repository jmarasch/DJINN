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
use djinn_store::{JoinRequest, Joined, Node, NodeAudio};

use crate::jukebox::EqualizerSettings;
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
        .route("/nodes/me/audio", put(put_my_audio))
        .route("/nodes/me/devices", post(put_my_devices))
        .route("/nodes/song/{id}", get(song))
        .route("/nodes/{id}/audio", put(put_audio))
        .route("/nodes/{id}/screen", get(screen_of))
        .route("/nodes/{id}/manage", post(manage_link))
        .route("/nodes/me/build", get(crate::node_builds::serve))
        .route("/nodes/me/manage-check", post(manage_check))
        .route("/nodes/clock", get(crate::node_audio::clock))
        .route("/nodes/stream", get(crate::node_audio::stream))
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

/// A node's speaker settings as pages and nodes see them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioSettings {
    /// Device id; None for the system default.
    pub device: Option<String>,
    pub delay_ms: i64,
    /// Volume trim, 0 to 1.
    pub trim: f64,
    pub equalizer: EqualizerSettings,
    /// "stop" or "cached".
    pub offline: String,
    /// Plays straight through instead of correcting itself against Main.
    #[serde(default)]
    pub free_run: bool,
    /// What a screen plugged into it shows: `SCREENS`.
    #[serde(default = "default_screen")]
    pub screen: String,
    /// Songs, playing and coming up, it keeps copies of (1 to 50).
    #[serde(default = "default_cache_ahead")]
    pub cache_ahead: i64,
}

fn default_cache_ahead() -> i64 {
    12
}

/// What a node's screen can show.
const SCREENS: &[&str] = &["tv", "lyrics", "status", "off"];

fn default_screen() -> String {
    "tv".to_owned()
}

impl AudioSettings {
    fn of(node: &Node) -> AudioSettings {
        AudioSettings {
            device: node.device.clone(),
            delay_ms: node.delay_ms,
            trim: node.trim,
            equalizer: node
                .equalizer
                .as_deref()
                .and_then(|e| serde_json::from_str(e).ok())
                .unwrap_or_default(),
            offline: node.offline.clone(),
            free_run: node.free_run,
            screen: node.screen.clone(),
            cache_ahead: node.cache_ahead,
        }
    }
}

async fn save_audio(state: &AppState, id: i64, audio: &AudioSettings) -> ApiResult<()> {
    if !audio.equalizer.valid()
        || !(0..=2000).contains(&audio.delay_ms)
        || !(0.0..=1.0).contains(&audio.trim)
    {
        return Err(ApiError::bad_request(
            "those speaker settings are out of range",
        ));
    }
    if !SCREENS.contains(&audio.screen.as_str()) {
        return Err(ApiError::bad_request(
            "that's not something a screen can show",
        ));
    }
    if !matches!(audio.offline.as_str(), "stop" | "cached") {
        return Err(ApiError::bad_request("offline must be stop or cached"));
    }
    let equalizer = serde_json::to_string(&audio.equalizer)?;
    let device = audio.device.as_deref().filter(|d| !d.is_empty());
    state
        .store
        .set_node_audio(
            id,
            &NodeAudio {
                device,
                delay_ms: audio.delay_ms,
                trim: audio.trim,
                equalizer: Some(&equalizer),
                offline: &audio.offline,
                free_run: audio.free_run,
                screen: &audio.screen,
                cache_ahead: audio.cache_ahead,
            },
        )
        .await?;
    Ok(())
}

/// A song coming up, for the node to cache.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Upcoming {
    pub id: i64,
    /// The file's extension, so the cached copy decodes the same.
    pub ext: String,
}

/// Songs playing and coming up in Main's room, in order, at most `limit`.
/// Past the requests, the auto mix only plans a few songs ahead, so a big
/// limit only fills up while people have asked for plenty.
async fn upcoming(state: &AppState, room: i64, limit: usize) -> ApiResult<Vec<Upcoming>> {
    let room = state.rooms.leader(room);
    let now_playing = state.jukebox_for(room).lock().await.now_playing();
    let queue = state.store.queue(room).await?;
    let mut ids: Vec<i64> = Vec::new();
    let mut push = |id: i64| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    if let Some(entry) = &now_playing.current {
        push(entry.track.id);
    }
    for item in &queue {
        push(item.track.id);
    }
    if let Some(entry) = &now_playing.up_next {
        push(entry.track.id);
    }
    for entry in &now_playing.later {
        push(entry.track.id);
    }
    ids.truncate(limit.clamp(1, 50));
    let paths = state.store.track_paths(&ids).await?;
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            let path = paths.get(&id)?;
            let ext = std::path::Path::new(path)
                .extension()?
                .to_str()?
                .to_ascii_lowercase();
            Some(Upcoming { id, ext })
        })
        .collect())
}

#[derive(Serialize)]
struct MeAnswer {
    status: String,
    name: String,
    room_id: i64,
    server_name: String,
    audio: AudioSettings,
    /// Songs to cache, in play order (only once accepted).
    upcoming: Vec<Upcoming>,
    /// The room whose music it plays now (its room's leader, or Main's while
    /// a quiet room follows Main); a change means reconnecting.
    audio_room: i64,
    /// The page a screen plugged into the node shows; None for the node's
    /// own page (its status, or blank).
    screen: Option<String>,
    /// A newer DJINN for the node to put in place, when it's behind.
    update: Option<crate::node_builds::NodeUpdate>,
}

/// The header a node's check-in says its version in.
pub const NODE_VERSION_HEADER: &str = "x-djinn-version";

/// The page on Main that a node's screen should show, or None for the
/// node's own page. Pages opened this way check back (`screen_of`), so a
/// change of setting reaches a screen already showing one.
fn screen_url(state: &AppState, node: &Node) -> Option<String> {
    let page = match node.screen.as_str() {
        "tv" => "tv",
        "lyrics" => "lyrics",
        _ => return None,
    };
    Some(format!(
        "{}/{page}?room={}&screen_for={}",
        state.join_url.trim_end_matches('/'),
        node.room_id,
        node.id
    ))
}

#[derive(Serialize)]
struct ScreenAnswer {
    url: Option<String>,
}

/// One-time links to manage a node on its own page: (SHA-256 of the token,
/// node id, good until in ms). Kept in memory: a restart ends them, which
/// only means asking for another.
static MANAGE_LINKS: std::sync::Mutex<Vec<(String, i64, i64)>> = std::sync::Mutex::new(Vec::new());
/// How long a manage link works, unused.
const MANAGE_LINK_MS: i64 = 10 * 60 * 1000;
/// The port a node's own page listens on (the install images' setting).
const NODE_PAGE_PORT: u16 = 8066;

#[derive(Serialize)]
struct ManageLink {
    url: String,
}

/// A one-time link that opens a node's own page ready to manage it. The
/// node checks it with this server (`manage_check`), over the connection
/// it already trusts, before letting that browser change anything.
async fn manage_link(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ManageLink>> {
    let node = state
        .store
        .node(id)
        .await?
        .filter(|node| node.status == "approved")
        .ok_or_else(ApiError::not_found)?;
    let address = node.address.ok_or_else(|| {
        ApiError::bad_request("this node hasn't checked in yet, so its address isn't known")
    })?;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    let token = hex(&bytes);
    let now = djinn_store::now_ms();
    if let Ok(mut links) = MANAGE_LINKS.lock() {
        links.retain(|link| link.2 > now);
        links.push((hash_secret(&token), id, now + MANAGE_LINK_MS));
    }
    let host = if address.contains(':') {
        format!("[{address}]")
    } else {
        address
    };
    Ok(Json(ManageLink {
        url: format!("http://{host}:{NODE_PAGE_PORT}/node?manage={token}"),
    }))
}

#[derive(Deserialize)]
struct ManageCheck {
    token: String,
}

/// A node asking whether a manage link someone opened on it is real (and
/// for it). Each link works once.
async fn manage_check(
    NodeAuth(node): NodeAuth,
    Json(body): Json<ManageCheck>,
) -> ApiResult<StatusCode> {
    let hash = hash_secret(&body.token);
    let now = djinn_store::now_ms();
    let mut links = MANAGE_LINKS
        .lock()
        .map_err(|_| anyhow::anyhow!("manage links unavailable"))?;
    links.retain(|link| link.2 > now);
    match links
        .iter()
        .position(|link| link.0 == hash && link.1 == node.id)
    {
        Some(at) => {
            links.remove(at);
            Ok(StatusCode::NO_CONTENT)
        }
        None => Err(ApiError::forbidden()),
    }
}

/// What a node's screen should show now. Public, like the pages themselves:
/// the browser on the node's screen has no sign-in.
async fn screen_of(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ScreenAnswer>> {
    let node = state
        .store
        .node(id)
        .await?
        .filter(|node| node.status == "approved")
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(ScreenAnswer {
        url: screen_url(&state, &node),
    }))
}

/// A node checking in: still waiting, accepted (and which room), or (401)
/// unknown, which means kicked or declined.
async fn me(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    NodeAuth(node): NodeAuth,
    headers: axum::http::HeaderMap,
) -> ApiResult<Json<MeAnswer>> {
    let address = ip.map(|ip| ip.to_string());
    // Which version the node runs now (it changes when it updates).
    let version = headers
        .get(NODE_VERSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(40).collect::<String>());
    state
        .store
        .node_seen(node.id, address.as_deref(), version.as_deref())
        .await?;
    let node = Node {
        version: version.or(node.version),
        ..node
    };
    let audio = AudioSettings::of(&node);
    let upcoming = if node.status == "approved" {
        upcoming(&state, node.room_id, node.cache_ahead as usize).await?
    } else {
        Vec::new()
    };
    Ok(Json(MeAnswer {
        audio,
        upcoming,
        audio_room: state.rooms.audio_leader(node.room_id),
        screen: screen_url(&state, &node),
        update: crate::node_builds::offer(&state, &node),
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
    audio: AudioSettings,
    /// The speakers the node reported.
    speakers: Vec<serde_json::Value>,
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
            audio: AudioSettings::of(&node),
            speakers: node
                .devices
                .as_deref()
                .and_then(|d| serde_json::from_str(d).ok())
                .unwrap_or_default(),
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
    give_own_room(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A node just accepted gets a room of its own, named after it (unless an
/// admin already placed it). The room plays along with Main until someone
/// asks for a song there, so the node isn't silent.
async fn give_own_room(state: &AppState, id: i64) -> anyhow::Result<()> {
    let Some(node) = state.store.node(id).await? else {
        return Ok(());
    };
    if node.room_id != djinn_store::MAIN_ROOM {
        return Ok(());
    }
    let mut room = None;
    for attempt in 1..=20 {
        let name = if attempt == 1 {
            node.name.clone()
        } else {
            format!("{} {attempt}", node.name)
        };
        match state.store.create_room(&name).await {
            Ok(made) => {
                room = Some(made);
                break;
            }
            Err(djinn_store::RoomError::NameTaken) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let Some(room) = room else {
        return Ok(());
    };
    state
        .store
        .set_setting(
            &crate::jukebox::room_key(room.id, crate::rooms_rt::FOLLOW_MAIN),
            "1",
        )
        .await?;
    state.store.set_node_room(id, room.id).await?;
    crate::rooms_rt::sync(state).await?;
    tracing::info!(node = %node.name, room = room.id, "node has its own room");
    Ok(())
}

/// Lets boxes being set up find this server, and whether nodes can ask to
/// join it. Announced once it's set up, unless the admin turned off being
/// found on the network (joining being on still announces). Nodes never
/// announce this, so they don't list each other.
pub fn watch_joining(state: AppState, port: u16) {
    tokio::spawn(async move {
        // What's being announced, and whether it said nodes can join.
        let mut announcing: Option<(crate::go_announce::Announcer, bool)> = None;
        loop {
            let joining = settings::flag(&state.store, NODES_JOIN, false)
                .await
                .unwrap_or(false);
            let findable = settings::flag(&state.store, crate::go_announce::GO_DISCOVERABLE, true)
                .await
                .unwrap_or(true);
            let set_up = state.store.admin_exists().await.unwrap_or(false);
            let name = crate::go_announce::server_name(&state.store)
                .await
                .unwrap_or_default();
            let wanted = set_up && (joining || findable);
            let stale = announcing
                .as_ref()
                .is_some_and(|(now, said)| !wanted || *said != joining || now.name != name);
            if stale {
                if let Some((stopping, _)) = announcing.take() {
                    stopping.stop();
                }
            }
            if wanted && announcing.is_none() {
                match crate::go_announce::Announcer::start_main(&state.store, port, joining).await {
                    Ok(started) => announcing = Some((started, joining)),
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "couldn't announce to nodes");
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    });
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

async fn put_audio(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AudioSettings>,
) -> ApiResult<StatusCode> {
    if state.store.node(id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    save_audio(&state, id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Settings changed on the node's own page.
async fn put_my_audio(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    Json(body): Json<AudioSettings>,
) -> ApiResult<StatusCode> {
    if node.status != "approved" {
        return Err(ApiError::forbidden());
    }
    save_audio(&state, node.id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Serialize)]
struct Speaker {
    id: String,
    name: String,
    #[serde(default)]
    is_default: bool,
}

#[derive(Deserialize)]
struct Speakers {
    devices: Vec<Speaker>,
}

/// The node's speakers, so Admin can offer them.
async fn put_my_devices(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    Json(body): Json<Speakers>,
) -> ApiResult<StatusCode> {
    let devices: Vec<Speaker> = body
        .devices
        .into_iter()
        .take(32)
        .map(|d| Speaker {
            id: d.id.chars().take(300).collect(),
            name: d.name.chars().take(120).collect(),
            is_default: d.is_default,
        })
        .collect();
    state
        .store
        .set_node_devices(node.id, &serde_json::to_string(&devices)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A song's file, for a joined node's cache.
async fn song(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    Path(id): Path<i64>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    use tower::ServiceExt;
    if node.status != "approved" {
        return Err(ApiError::forbidden());
    }
    let paths = state.store.track_paths(&[id]).await?;
    let path = paths.get(&id).ok_or_else(ApiError::not_found)?;
    let response = tower_http::services::ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(axum::response::IntoResponse::into_response(response))
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

    async fn ask_to_join(state: &AppState, node_id: &str, name: &str) -> i64 {
        state
            .store
            .request_node_join(&djinn_store::JoinRequest {
                node_id,
                name,
                secret_hash: "hash",
                address: None,
                version: None,
                platform: None,
            })
            .await
            .unwrap();
        state
            .store
            .node_by_node_id(node_id)
            .await
            .unwrap()
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn a_nodes_screen_shows_what_its_settings_say() {
        let state = AppState::for_tests()
            .await
            .with_join_url("http://main:8066/".into());
        let id = ask_to_join(&state, "ccc", "Den").await;
        // Only for accepted nodes.
        let path = format!("/api/nodes/{id}/screen");
        let (status, _) = send(&state, Method::GET, &path, None, None, false).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let node = state.store.node(id).await.unwrap().unwrap();
        assert_eq!(
            screen_url(&state, &node).unwrap(),
            format!("http://main:8066/tv?room=1&screen_for={id}")
        );
        let with = |screen: &str| Node {
            screen: screen.to_owned(),
            ..node.clone()
        };
        assert!(screen_url(&state, &with("lyrics"))
            .unwrap()
            .contains("/lyrics?room=1"));
        assert_eq!(screen_url(&state, &with("status")), None);
        assert_eq!(screen_url(&state, &with("off")), None);
    }

    #[tokio::test]
    async fn an_accepted_node_gets_its_own_room_that_follows_main() {
        let state = AppState::for_tests().await;
        let first = ask_to_join(&state, "aaa", "Kitchen").await;
        give_own_room(&state, first).await.unwrap();
        let room = state.store.node(first).await.unwrap().unwrap().room_id;
        assert_ne!(room, djinn_store::MAIN_ROOM);
        assert_eq!(
            state.store.room(room).await.unwrap().unwrap().name,
            "Kitchen"
        );
        assert!(
            crate::rooms_rt::quiet_rules(&state.store, room)
                .await
                .unwrap()
                .follow_main
        );

        // A second node with that name gets a room of its own too.
        let second = ask_to_join(&state, "bbb", "Kitchen").await;
        give_own_room(&state, second).await.unwrap();
        let other = state.store.node(second).await.unwrap().unwrap().room_id;
        assert_ne!(other, room);
        assert_eq!(
            state.store.room(other).await.unwrap().unwrap().name,
            "Kitchen 2"
        );

        // One an admin already placed stays where it is.
        give_own_room(&state, first).await.unwrap();
        assert_eq!(
            state.store.node(first).await.unwrap().unwrap().room_id,
            room
        );
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
    async fn a_node_behind_is_offered_the_update_once_main_has_it() {
        let state = AppState::for_tests().await;
        admin_with_password(&state).await;
        settings::set_flag(&state.store, NODES_JOIN, true)
            .await
            .unwrap();
        let old = r#"{"node_id":"den-0001","name":"Den","secret":"0123456789abcdef0123456789abcdef","version":"2020.1.1","platform":"linux-aarch64"}"#;
        let (status, _) = send(
            &state,
            Method::POST,
            "/api/nodes/join",
            Some(old),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let id = state
            .store
            .node_by_node_id("den-0001")
            .await
            .unwrap()
            .unwrap()
            .id;
        state.store.approve_node(id, 1).await.unwrap();
        let signed = format!("Node den-0001:{SECRET}");
        let node = Some(("authorization", signed.as_str()));
        // The file isn't here yet (fetching it can't work in a test): nothing offered.
        let (_, body) = send(&state, Method::GET, "/api/nodes/me", None, node, true).await;
        assert!(body.contains(r#""update":null"#), "{body}");
        // Once it's here, it's offered with its sum, and served to the node.
        let dir = state
            .art_dir
            .parent()
            .unwrap()
            .join("node-builds")
            .join(env!("DJINN_RELEASE_VERSION"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("djinn-linux-aarch64"), b"new program").unwrap();
        std::fs::write(dir.join("djinn-linux-aarch64.sha256"), "abc123").unwrap();
        let (_, body) = send(&state, Method::GET, "/api/nodes/me", None, node, true).await;
        assert!(body.contains(r#""sha256":"abc123""#), "{body}");
        let (status, file) =
            send(&state, Method::GET, "/api/nodes/me/build", None, node, true).await;
        assert_eq!((status, file.as_str()), (StatusCode::OK, "new program"));
        // A node that says it's up to date isn't offered anything.
        let mut req = axum::http::Request::builder()
            .uri("/api/nodes/me")
            .header("authorization", signed.as_str())
            .header(NODE_VERSION_HEADER, env!("DJINN_RELEASE_VERSION"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(DirectTls);
        let res = crate::app(state.clone()).oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains(r#""update":null"#));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_manage_link_works_once_and_only_for_its_node() {
        let state = AppState::for_tests().await;
        let admin = admin_with_password(&state).await;
        let cookie = Some(("cookie", admin.as_str()));
        let signed = format!("Node kitchen-0001:{SECRET}");
        let node = Some(("authorization", signed.as_str()));
        settings::set_flag(&state.store, NODES_JOIN, true)
            .await
            .unwrap();
        let (status, _) = send(
            &state,
            Method::POST,
            "/api/nodes/join",
            Some(JOIN),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let id = state
            .store
            .node_by_node_id("kitchen-0001")
            .await
            .unwrap()
            .unwrap()
            .id;
        let manage = format!("/api/nodes/{id}/manage");
        // Not before it's accepted.
        let (status, _) = send(&state, Method::POST, &manage, None, cookie, false).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        state.store.approve_node(id, 1).await.unwrap();
        state
            .store
            .node_seen(id, Some("192.168.1.30"), None)
            .await
            .unwrap();
        // Only admins get one.
        let (status, _) = send(&state, Method::POST, &manage, None, None, false).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, body) = send(&state, Method::POST, &manage, None, cookie, false).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let url: String = serde_json::from_str::<serde_json::Value>(&body).unwrap()["url"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            url.starts_with("http://192.168.1.30:8066/node?manage="),
            "{url}"
        );
        let token = url.split("manage=").nth(1).unwrap();

        let check = format!(r#"{{"token":"{token}"}}"#);
        let path = "/api/nodes/me/manage-check";
        // Someone guessing gets nothing.
        let (status, _) = send(
            &state,
            Method::POST,
            path,
            Some(r#"{"token":"guess"}"#),
            node,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // The node checks it once; a second time it's spent.
        let (status, _) = send(&state, Method::POST, path, Some(&check), node, true).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = send(&state, Method::POST, path, Some(&check), node, true).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

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
    async fn speaker_settings_go_both_ways_and_are_checked() {
        let state = AppState::for_tests().await;
        let admin = admin_with_password(&state).await;
        let cookie = Some(("cookie", admin.as_str()));
        state.store.set_setting(NODES_JOIN, "1").await.unwrap();
        let (status, _) = send(
            &state,
            Method::POST,
            "/api/nodes/join",
            Some(JOIN),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let id = state
            .store
            .node_by_node_id("kitchen-0001")
            .await
            .unwrap()
            .unwrap()
            .id;
        let signed = format!("Node kitchen-0001:{SECRET}");
        let node = Some(("authorization", signed.as_str()));
        let audio = |delay: i64, offline: &str| {
            format!(
                r#"{{"device":"hw:1","delay_ms":{delay},"trim":0.8,"offline":"{offline}","equalizer":{{"enabled":true,"bands_db":[1,0,0,0,0,0,0,0,0,0,0,0],"output_gain_db":0}}}}"#
            )
        };
        // Not before it's accepted.
        let (status, _) = send(
            &state,
            Method::PUT,
            "/api/nodes/me/audio",
            Some(&audio(10, "stop")),
            node,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        state.store.approve_node(id, 1).await.unwrap();

        let path = format!("/api/nodes/{id}/audio");
        let (status, _) = send(
            &state,
            Method::PUT,
            &path,
            Some(&audio(150, "cached")),
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, me) = send(&state, Method::GET, "/api/nodes/me", None, node, true).await;
        assert!(
            me.contains(r#""delay_ms":150"#) && me.contains(r#""offline":"cached""#),
            "{me}"
        );
        assert!(me.contains(r#""device":"hw:1""#), "{me}");

        // The node's page changes them too.
        let (status, _) = send(
            &state,
            Method::PUT,
            "/api/nodes/me/audio",
            Some(&audio(40, "stop")),
            node,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, list) = send(&state, Method::GET, "/api/nodes", None, cookie, false).await;
        assert!(list.contains(r#""delay_ms":40"#), "{list}");

        // Out of range.
        let (status, _) = send(
            &state,
            Method::PUT,
            &path,
            Some(&audio(5000, "stop")),
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = send(
            &state,
            Method::PUT,
            &path,
            Some(&audio(10, "sometimes")),
            cookie,
            false,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Its speakers, reported for Admin.
        let speakers = r#"{"devices":[{"id":"hw:1","name":"USB DAC","is_default":false}]}"#;
        let (status, _) = send(
            &state,
            Method::POST,
            "/api/nodes/me/devices",
            Some(speakers),
            node,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, list) = send(&state, Method::GET, "/api/nodes", None, cookie, false).await;
        assert!(list.contains("USB DAC"), "{list}");
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
