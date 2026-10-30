//! The Subsonic API (with OpenSubsonic's additions) on the DJINN GO port,
//! so phone apps like Symfonium, DSub, play:Sub, Substreamer and Feishin
//! can browse and play the library.
//!
//! Apps sign in on every call with an app password made under Your account
//! (see `subsonic_keys`): as the password itself, a salted MD5 token, or
//! an API key. Account passwords are never accepted here. The same checks
//! as the rest of GO apply: the role must allow GO (and away from home),
//! and two-step sign-in must be on where it's required. Answers are XML,
//! or JSON with `f=json`. Songs are sent as their original files.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use djinn_store::{
    AlbumList, GoPlaylistDetail, LikeKind, LikedIds, SsAlbum, SsArtist, Track, TrackFile, User,
};
use md5::{Digest, Md5};

use crate::auth::{go_refusal, Device};
use crate::error::ApiError;
use crate::go::{send_song, Listening};
use crate::state::AppState;
use crate::two_step;

const API_VERSION: &str = "1.16.1";
const MAX_LIST: i64 = 500;

pub fn router() -> Router<AppState> {
    Router::new().route("/rest/{method}", any(handle))
}

/// One element of an answer: XML attributes and children, or JSON fields
/// and arrays.
struct Node {
    name: &'static str,
    attrs: Vec<(&'static str, Value)>,
    kids: Vec<Node>,
    /// Shown as an array in JSON, even alone.
    list: bool,
    /// Text content (XML), or "value" (JSON).
    text: Option<String>,
}

enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_owned())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl Node {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            attrs: Vec::new(),
            kids: Vec::new(),
            list: false,
            text: None,
        }
    }

    fn a(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.attrs.push((key, value.into()));
        self
    }

    fn opt<V: Into<Value>>(self, key: &'static str, value: Option<V>) -> Self {
        match value {
            Some(v) => self.a(key, v),
            None => self,
        }
    }

    fn kid(mut self, node: Node) -> Self {
        self.kids.push(node);
        self
    }

    /// Children that are always a list.
    fn items(mut self, nodes: impl IntoIterator<Item = Node>) -> Self {
        self.kids.extend(nodes.into_iter().map(|mut n| {
            n.list = true;
            n
        }));
        self
    }

    fn text(mut self, text: String) -> Self {
        self.text = Some(text);
        self
    }

    fn xml(&self, out: &mut String) {
        out.push('<');
        out.push_str(self.name);
        for (k, v) in &self.attrs {
            let _ = write!(out, " {k}=\"");
            match v {
                Value::Str(s) => escape(s, out),
                Value::Int(i) => {
                    let _ = write!(out, "{i}");
                }
                Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            }
            out.push('"');
        }
        if self.kids.is_empty() && self.text.is_none() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        if let Some(text) = &self.text {
            escape(text, out);
        }
        for kid in &self.kids {
            kid.xml(out);
        }
        let _ = write!(out, "</{}>", self.name);
    }

    fn json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (k, v) in &self.attrs {
            let value = match v {
                Value::Str(s) => serde_json::Value::from(s.as_str()),
                Value::Int(i) => serde_json::Value::from(*i),
                Value::Bool(b) => serde_json::Value::from(*b),
            };
            map.insert((*k).to_owned(), value);
        }
        if let Some(text) = &self.text {
            map.insert("value".to_owned(), serde_json::Value::from(text.as_str()));
        }
        for kid in &self.kids {
            let value = kid.json();
            if kid.list {
                let entry = map
                    .entry(kid.name.to_owned())
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                if let serde_json::Value::Array(items) = entry {
                    items.push(value);
                }
            } else {
                map.insert(kid.name.to_owned(), value);
            }
        }
        serde_json::Value::Object(map)
    }
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
}

/// A call's parameters, from the URL and a form body; names can repeat.
struct Params(Vec<(String, String)>);

impl Params {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, key: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.trim().parse().ok())
    }

    fn ints(&self, key: &str) -> Vec<i64> {
        self.all(key)
            .iter()
            .filter_map(|v| v.trim().parse().ok())
            .collect()
    }

    fn json(&self) -> bool {
        matches!(self.get("f"), Some("json" | "jsonp"))
    }
}

/// Subsonic's errors: still HTTP 200, with a code.
struct Failure {
    code: i64,
    message: String,
}

impl Failure {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn missing(name: &str) -> Self {
        Self::new(10, format!("required parameter is missing: {name}"))
    }

    fn not_found(what: &str) -> Self {
        Self::new(70, format!("{what} not found"))
    }
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %format!("{e:#}"), "subsonic call failed");
        Self::new(0, "something went wrong")
    }
}

impl From<djinn_store::StoreError> for Failure {
    fn from(e: djinn_store::StoreError) -> Self {
        anyhow::Error::from(e).into()
    }
}

impl From<ApiError> for Failure {
    fn from(_: ApiError) -> Self {
        Self::new(0, "something went wrong")
    }
}

type Answer = Result<Reply, Failure>;

/// What a call gives back: an answer to wrap, the list of OpenSubsonic
/// extensions, or a file.
enum Reply {
    Node(Option<Node>),
    Extensions,
    Raw(Response),
}

/// The OpenSubsonic extensions DJINN supports, with their versions.
const EXTENSIONS: &[(&str, i64)] = &[("formPost", 1), ("apiKeyAuthentication", 1)];

fn reply(node: Node) -> Answer {
    Ok(Reply::Node(Some(node)))
}

fn empty() -> Answer {
    Ok(Reply::Node(None))
}

fn wrap(params: &Params, ok: Result<Option<Node>, Failure>) -> Response {
    let mut root = Node::new("subsonic-response")
        .a("status", if ok.is_ok() { "ok" } else { "failed" })
        .a("version", API_VERSION)
        .a("type", "djinn")
        .a("serverVersion", env!("CARGO_PKG_VERSION"))
        .a("openSubsonic", true);
    match ok {
        Ok(Some(node)) => root = root.kid(node),
        Ok(None) => {}
        Err(f) => {
            root = root.kid(Node::new("error").a("code", f.code).a("message", f.message));
        }
    }
    if params.json() {
        let body = serde_json::json!({ "subsonic-response": root.json() });
        (
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response()
    } else {
        let mut out = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        let root = Node {
            attrs: {
                let mut attrs = vec![("xmlns", Value::from("http://subsonic.org/restapi"))];
                attrs.extend(root.attrs);
                attrs
            },
            ..root
        };
        root.xml(&mut out);
        ([(header::CONTENT_TYPE, "text/xml; charset=utf-8")], out).into_response()
    }
}

/// The extensions list: a bare array, with versions as numbers in JSON and
/// child elements in XML, so it doesn't fit `Node`.
fn extensions(params: &Params) -> Response {
    let base = |status: &str| {
        serde_json::json!({
            "status": status,
            "version": API_VERSION,
            "type": "djinn",
            "serverVersion": env!("CARGO_PKG_VERSION"),
            "openSubsonic": true,
        })
    };
    if params.json() {
        let mut root = base("ok");
        root["openSubsonicExtensions"] = EXTENSIONS
            .iter()
            .map(|(name, v)| serde_json::json!({ "name": name, "versions": [v] }))
            .collect();
        let body = serde_json::json!({ "subsonic-response": root });
        return (
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response();
    }
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><subsonic-response xmlns="http://subsonic.org/restapi" status="ok" version="{API_VERSION}" type="djinn" serverVersion="{}" openSubsonic="true">"#,
        env!("CARGO_PKG_VERSION")
    );
    for (name, v) in EXTENSIONS {
        let _ = write!(
            out,
            r#"<openSubsonicExtensions name="{name}"><versions>{v}</versions></openSubsonicExtensions>"#
        );
    }
    out.push_str("</subsonic-response>");
    ([(header::CONTENT_TYPE, "text/xml; charset=utf-8")], out).into_response()
}

async fn handle(
    State(state): State<AppState>,
    Path(method): Path<String>,
    request: Request,
) -> Response {
    let (mut parts, body) = request.into_parts();
    let mut pairs: Vec<(String, String)> = parts
        .uri
        .query()
        .and_then(|q| serde_urlencoded::from_str(q).ok())
        .unwrap_or_default();
    // OpenSubsonic's formPost: the same parameters in a form body.
    let form = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    if parts.method == Method::POST && form {
        if let Ok(bytes) = axum::body::to_bytes(body, 1024 * 1024).await {
            if let Ok(more) = serde_urlencoded::from_bytes::<Vec<(String, String)>>(&bytes) {
                pairs.extend(more);
            }
        }
    }
    let params = Params(pairs);
    let Ok(device) = Device::from_request_parts(&mut parts, &state).await;
    let user = match sign_in(&state, &params, &device).await {
        Ok(user) => user,
        Err(f) => return wrap(&params, Err(f)),
    };
    let method = method.trim_end_matches(".view").to_ascii_lowercase();
    let request = Request::from_parts(parts, Body::empty());
    match call(&state, &user, &method, &params, &device, request).await {
        Ok(Reply::Raw(response)) => response,
        Ok(Reply::Node(node)) => wrap(&params, Ok(node)),
        Ok(Reply::Extensions) => extensions(&params),
        Err(f) => wrap(&params, Err(f)),
    }
}

/// Checks the app password (plain, `enc:` hex, a salted MD5 token, or an
/// API key) and that the account may use GO from here.
async fn sign_in(state: &AppState, params: &Params, device: &Device) -> Result<User, Failure> {
    let account = params.get("u").unwrap_or("").trim().to_owned();
    if let Some(wait) = state.limiter.wait(&account, device.addr) {
        return Err(Failure::new(
            40,
            format!(
                "too many tries; wait {} minutes",
                wait.as_secs().div_ceil(60)
            ),
        ));
    }
    let found = if let Some(key) = params.get("apiKey") {
        state.store.subsonic_key_account(key).await?
    } else {
        let Some((user, secret)) = state.store.subsonic_account(&account).await? else {
            return Err(refuse(state, device, &account, None).await);
        };
        let ok = match (params.get("p"), params.get("t"), params.get("s")) {
            (Some(p), _, _) => {
                let given = match p.strip_prefix("enc:") {
                    Some(hex) => decode_hex(hex).unwrap_or_default(),
                    None => p.to_owned(),
                };
                same(given.as_bytes(), secret.as_bytes())
            }
            (None, Some(t), Some(s)) => {
                let digest = Md5::digest(format!("{secret}{s}").as_bytes());
                let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                same(hex.as_bytes(), t.to_ascii_lowercase().as_bytes())
            }
            _ => return Err(Failure::missing("p or t and s")),
        };
        if !ok {
            return Err(refuse(state, device, &account, Some(user.id)).await);
        }
        Some(user)
    };
    let Some(user) = found else {
        return Err(refuse(state, device, &account, None).await);
    };
    if let Ok(Some((_, message))) = go_refusal(state, &user, device).await {
        return Err(Failure::new(50, message));
    }
    if !user.two_step && two_step::required(state, &user).await? {
        return Err(Failure::new(
            50,
            "turn on two-step sign-in in DJINN GO first",
        ));
    }
    Ok(user)
}

async fn refuse(state: &AppState, device: &Device, account: &str, user_id: Option<i64>) -> Failure {
    state.limiter.failed(account, device.addr);
    let _ = crate::auth::log_sign_in(state, device, account, user_id, "wrong_password").await;
    Failure::new(40, "wrong username or app password")
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn decode_hex(hex: &str) -> Option<String> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

async fn call(
    state: &AppState,
    user: &User,
    method: &str,
    params: &Params,
    device: &Device,
    request: Request,
) -> Answer {
    match method {
        "ping" => empty(),
        "getlicense" => reply(Node::new("license").a("valid", true)),
        "getopensubsonicextensions" => Ok(Reply::Extensions),
        "getmusicfolders" => reply(
            Node::new("musicFolders")
                .items([Node::new("musicFolder").a("id", 1).a("name", "Music")]),
        ),
        "getindexes" => indexes(state).await,
        "getmusicdirectory" => music_directory(state, user, params).await,
        "getartists" => artists(state, user).await,
        "getartist" => artist(state, user, params).await,
        "getalbum" => album(state, user, params).await,
        "getsong" => song(state, user, params).await,
        "getalbumlist" | "getalbumlist2" => album_list(state, user, method, params).await,
        "getrandomsongs" => random_songs(state, user, params).await,
        "getsongsbygenre" => songs_by_genre(state, user, params).await,
        "getgenres" => genres(state).await,
        "search2" | "search3" => search(state, user, method, params).await,
        "getstarred" | "getstarred2" => starred(state, user, method).await,
        "star" => star(state, user, params, true).await,
        "unstar" => star(state, user, params, false).await,
        "scrobble" => scrobble(state, user, params).await,
        "getplaylists" => playlists(state, user).await,
        "getplaylist" => playlist(state, user, params).await,
        "createplaylist" => create_playlist(state, user, params).await,
        "updateplaylist" => update_playlist(state, user, params).await,
        "deleteplaylist" => {
            let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
            if !state.store.delete_go_playlist(user.id, id).await? {
                return Err(Failure::not_found("playlist"));
            }
            empty()
        }
        "getplayqueue" => play_queue(state, user).await,
        "saveplayqueue" => save_play_queue(state, user, params).await,
        "getuser" => user_info(state, user, params).await,
        "stream" => stream(state, user, params, device, request, false).await,
        "download" => stream(state, user, params, device, request, true).await,
        "getcoverart" => cover_art(state, params).await,
        "getsimilarsongs" | "getsimilarsongs2" => similar(state, user, method, params).await,
        "gettopsongs" => top_songs(state, user, params).await,
        "getscanstatus" => reply(
            Node::new("scanStatus")
                .a(
                    "scanning",
                    state
                        .scan
                        .running
                        .load(std::sync::atomic::Ordering::Relaxed),
                )
                .a("count", state.store.track_count().await?),
        ),
        "getartistinfo" | "getartistinfo2" => artist_info(state, method, params).await,
        "getalbuminfo" | "getalbuminfo2" => reply(Node::new("albumInfo")),
        "getnowplaying" => reply(Node::new("nowPlaying")),
        "getlyrics" => reply(Node::new("lyrics")),
        "getbookmarks" => reply(Node::new("bookmarks")),
        "getpodcasts" => reply(Node::new("podcasts")),
        "getnewestpodcasts" => reply(Node::new("newestPodcasts")),
        "getinternetradiostations" => reply(Node::new("internetRadioStations")),
        "getshares" => reply(Node::new("shares")),
        _ => Err(Failure::new(0, format!("{method} isn't supported"))),
    }
}

/// Unix ms as an ISO 8601 UTC time.
fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn suffix(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn content_type(suffix: &str) -> &'static str {
    match suffix {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "mp4" | "m4b" | "aac" | "alac" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// Context for writing songs: their files and what's liked.
struct Songs {
    files: HashMap<i64, TrackFile>,
    liked: LikedIds,
}

impl Songs {
    async fn load(state: &AppState, user: &User, tracks: &[Track]) -> Result<Self, Failure> {
        let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
        let files = state
            .store
            .track_files(&ids)
            .await?
            .into_iter()
            .map(|f| (f.id, f))
            .collect();
        Ok(Self {
            files,
            liked: state.store.liked_ids(user.id).await?,
        })
    }

    fn node(&self, name: &'static str, t: &Track) -> Node {
        let file = self.files.get(&t.id);
        let suffix = file.map(|f| suffix(&f.path)).unwrap_or_default();
        let file_name = file
            .and_then(|f| {
                std::path::Path::new(&f.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        Node::new(name)
            .a("id", t.id.to_string())
            .opt("parent", t.album_id.map(|id| id.to_string()))
            .a("isDir", false)
            .a("title", t.title.as_str())
            .opt("album", t.album.clone())
            .opt("artist", t.artist.clone())
            .opt("track", t.track_no)
            .opt("discNumber", t.disc_no)
            .opt("year", t.year)
            .opt("genre", t.genre.clone())
            .opt("coverArt", t.art_hash.clone())
            .opt("size", file.map(|f| f.size))
            .a("contentType", content_type(&suffix))
            .a("suffix", suffix.clone())
            .opt("duration", t.duration_ms.map(|ms| ms / 1000))
            .a(
                "path",
                format!(
                    "{}/{}/{file_name}",
                    t.artist.as_deref().unwrap_or("Unknown"),
                    t.album.as_deref().unwrap_or("Unknown")
                ),
            )
            .opt("albumId", t.album_id.map(|id| id.to_string()))
            .opt("artistId", t.artist_id.map(|id| id.to_string()))
            .a("type", "music")
            .a("mediaType", "song")
            .opt("created", file.map(|f| iso(f.added_at)))
            .opt(
                "starred",
                self.liked
                    .tracks
                    .contains(&t.id)
                    .then(|| iso(djinn_store::now_ms())),
            )
    }
}

fn album_node(name: &'static str, a: &SsAlbum, liked: &LikedIds) -> Node {
    Node::new(name)
        .a("id", a.id.to_string())
        .a("name", a.name.as_str())
        .a("title", a.name.as_str())
        .a("album", a.name.as_str())
        .a("artist", a.artist.as_str())
        .a("artistId", a.artist_id.to_string())
        .a("parent", a.artist_id.to_string())
        .a("isDir", true)
        .opt("coverArt", a.art_hash.clone())
        .a("songCount", a.song_count)
        .a("duration", a.duration_ms / 1000)
        .a("created", iso(a.created))
        .opt("year", a.year)
        .opt("genre", a.genre.clone())
        .opt(
            "starred",
            liked
                .albums
                .contains(&a.id)
                .then(|| iso(djinn_store::now_ms())),
        )
}

fn artist_node(a: &SsArtist, liked: &LikedIds) -> Node {
    Node::new("artist")
        .a("id", a.id.to_string())
        .a("name", a.name.as_str())
        .opt("coverArt", a.art_hash.clone())
        .opt("artistImageUrl", None::<String>)
        .a("albumCount", a.album_count)
        .opt(
            "starred",
            liked
                .artists
                .contains(&a.id)
                .then(|| iso(djinn_store::now_ms())),
        )
}

/// The letter an artist files under ("The Beatles" under B).
fn index_letter(name: &str) -> String {
    let trimmed = name
        .strip_prefix("The ")
        .or_else(|| name.strip_prefix("the "))
        .unwrap_or(name);
    match trimmed.chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
}

fn grouped(artists: &[SsArtist], liked: &LikedIds, folders: bool) -> Vec<Node> {
    let mut groups: Vec<(String, Vec<Node>)> = Vec::new();
    for a in artists {
        let letter = index_letter(&a.name);
        let node = if folders {
            Node::new("artist")
                .a("id", format!("ar{}", a.id))
                .a("name", a.name.as_str())
        } else {
            artist_node(a, liked)
        };
        match groups.iter_mut().find(|(l, _)| *l == letter) {
            Some((_, nodes)) => nodes.push(node),
            None => groups.push((letter, vec![node])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
        .into_iter()
        .map(|(letter, nodes)| Node::new("index").a("name", letter).items(nodes))
        .collect()
}

async fn artists(state: &AppState, user: &User) -> Answer {
    let artists = state.store.ss_artists().await?;
    let liked = state.store.liked_ids(user.id).await?;
    reply(
        Node::new("artists")
            .a("ignoredArticles", "The")
            .items(grouped(&artists, &liked, false)),
    )
}

async fn indexes(state: &AppState) -> Answer {
    let artists = state.store.ss_artists().await?;
    reply(
        Node::new("indexes")
            .a("ignoredArticles", "The")
            .a("lastModified", djinn_store::now_ms())
            .items(grouped(&artists, &LikedIds::default(), true)),
    )
}

/// Folder-style browsing (older apps): "ar<id>" is an artist with album
/// folders, "al<id>" an album with songs.
async fn music_directory(state: &AppState, user: &User, params: &Params) -> Answer {
    let id = params.get("id").ok_or_else(|| Failure::missing("id"))?;
    if let Some(artist) = id.strip_prefix("ar").and_then(|n| n.parse::<i64>().ok()) {
        let detail = state
            .store
            .artist(artist)
            .await?
            .ok_or_else(|| Failure::not_found("artist"))?;
        let albums = detail.albums.iter().map(|al| {
            Node::new("child")
                .a("id", format!("al{}", al.id))
                .a("parent", id)
                .a("isDir", true)
                .a("title", al.title.as_str())
                .a("album", al.title.as_str())
                .a("artist", al.artist.as_str())
                .opt("year", al.year)
                .opt("coverArt", al.art_hash.clone())
        });
        return reply(
            Node::new("directory")
                .a("id", id)
                .a("name", detail.artist.name.as_str())
                .items(albums.collect::<Vec<_>>()),
        );
    }
    let album_id = id
        .strip_prefix("al")
        .unwrap_or(id)
        .parse::<i64>()
        .map_err(|_| Failure::not_found("folder"))?;
    let detail = state
        .store
        .album(album_id)
        .await?
        .ok_or_else(|| Failure::not_found("album"))?;
    let songs = Songs::load(state, user, &detail.tracks).await?;
    let kids: Vec<Node> = detail
        .tracks
        .iter()
        .map(|t| songs.node("child", t).a("parent", id))
        .collect();
    reply(
        Node::new("directory")
            .a("id", id)
            .a("parent", format!("ar{}", detail.album.artist_id))
            .a("name", detail.album.title.as_str())
            .items(kids),
    )
}

async fn artist(state: &AppState, user: &User, params: &Params) -> Answer {
    let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
    let detail = state
        .store
        .artist(id)
        .await?
        .ok_or_else(|| Failure::not_found("artist"))?;
    let liked = state.store.liked_ids(user.id).await?;
    let mut albums = Vec::new();
    for al in &detail.albums {
        if let Some(a) = state.store.ss_album(al.id).await? {
            albums.push(album_node("album", &a, &liked));
        }
    }
    reply(
        Node::new("artist")
            .a("id", id.to_string())
            .a("name", detail.artist.name.as_str())
            .opt("coverArt", detail.artist.art_hash.clone())
            .a("albumCount", i64::try_from(albums.len()).unwrap_or(0))
            .opt(
                "starred",
                liked
                    .artists
                    .contains(&id)
                    .then(|| iso(djinn_store::now_ms())),
            )
            .items(albums),
    )
}

async fn album(state: &AppState, user: &User, params: &Params) -> Answer {
    let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
    let summary = state
        .store
        .ss_album(id)
        .await?
        .ok_or_else(|| Failure::not_found("album"))?;
    let detail = state
        .store
        .album(id)
        .await?
        .ok_or_else(|| Failure::not_found("album"))?;
    let songs = Songs::load(state, user, &detail.tracks).await?;
    let kids: Vec<Node> = detail
        .tracks
        .iter()
        .map(|t| songs.node("song", t))
        .collect();
    reply(album_node("album", &summary, &songs.liked).items(kids))
}

async fn song(state: &AppState, user: &User, params: &Params) -> Answer {
    let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
    let track = state
        .store
        .track(id)
        .await?
        .ok_or_else(|| Failure::not_found("song"))?;
    let songs = Songs::load(state, user, std::slice::from_ref(&track)).await?;
    reply(songs.node("song", &track))
}

async fn album_list(state: &AppState, user: &User, method: &str, params: &Params) -> Answer {
    let kind = params.get("type").ok_or_else(|| Failure::missing("type"))?;
    let list = match kind {
        "random" => AlbumList::Random,
        "newest" => AlbumList::Newest,
        "alphabeticalByName" => AlbumList::ByName,
        "alphabeticalByArtist" => AlbumList::ByArtist,
        "starred" => AlbumList::Starred(user.id),
        "recent" => AlbumList::Recent(user.id),
        "frequent" | "highest" => AlbumList::Frequent(user.id),
        "byYear" => AlbumList::ByYear {
            from: params.int("fromYear").unwrap_or(0),
            to: params.int("toYear").unwrap_or(9999),
        },
        "byGenre" => AlbumList::ByGenre(
            params
                .get("genre")
                .ok_or_else(|| Failure::missing("genre"))?
                .to_owned(),
        ),
        other => return Err(Failure::new(0, format!("unknown list type {other}"))),
    };
    let size = params.int("size").unwrap_or(10).clamp(1, MAX_LIST);
    let offset = params.int("offset").unwrap_or(0).max(0);
    let albums = state.store.ss_albums(&list, offset, size).await?;
    let liked = state.store.liked_ids(user.id).await?;
    let name = if method == "getalbumlist" {
        "albumList"
    } else {
        "albumList2"
    };
    reply(
        Node::new(name).items(
            albums
                .iter()
                .map(|a| album_node("album", a, &liked))
                .collect::<Vec<_>>(),
        ),
    )
}

async fn songs_node(state: &AppState, user: &User, name: &'static str, ids: &[i64]) -> Answer {
    let tracks = state.store.tracks_by_ids(ids).await?;
    let songs = Songs::load(state, user, &tracks).await?;
    reply(
        Node::new(name).items(
            tracks
                .iter()
                .map(|t| songs.node("song", t))
                .collect::<Vec<_>>(),
        ),
    )
}

async fn random_songs(state: &AppState, user: &User, params: &Params) -> Answer {
    let size = params.int("size").unwrap_or(10).clamp(1, MAX_LIST);
    let ids = state
        .store
        .random_song_ids(
            size,
            params.get("genre"),
            params.int("fromYear"),
            params.int("toYear"),
        )
        .await?;
    songs_node(state, user, "randomSongs", &ids).await
}

async fn songs_by_genre(state: &AppState, user: &User, params: &Params) -> Answer {
    let genre = params
        .get("genre")
        .ok_or_else(|| Failure::missing("genre"))?;
    let count = params.int("count").unwrap_or(10).clamp(1, MAX_LIST);
    let offset = params.int("offset").unwrap_or(0).max(0);
    let ids = state.store.song_ids_by_genre(genre, offset, count).await?;
    songs_node(state, user, "songsByGenre", &ids).await
}

async fn genres(state: &AppState) -> Answer {
    let genres = state.store.ss_genres().await?;
    reply(
        Node::new("genres").items(
            genres
                .into_iter()
                .map(|g| {
                    Node::new("genre")
                        .a("songCount", g.songs)
                        .a("albumCount", g.albums)
                        .text(g.name)
                })
                .collect::<Vec<_>>(),
        ),
    )
}

/// Search, or, with an empty query, everything a page at a time (apps like
/// Symfonium sync the library this way).
async fn search(state: &AppState, user: &User, method: &str, params: &Params) -> Answer {
    let query = params
        .get("query")
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim();
    let count = |key: &str| params.int(key).unwrap_or(20).clamp(0, MAX_LIST);
    let offset = |key: &str| params.int(key).unwrap_or(0).max(0);
    let (artist_n, album_n, song_n) = (
        count("artistCount"),
        count("albumCount"),
        count("songCount"),
    );
    let (artist_at, album_at, song_at) = (
        offset("artistOffset"),
        offset("albumOffset"),
        offset("songOffset"),
    );
    let liked = state.store.liked_ids(user.id).await?;

    let (artists, albums, tracks) = if query.is_empty() {
        let all = state.store.ss_artists().await?;
        let artists: Vec<SsArtist> = all
            .into_iter()
            .skip(usize::try_from(artist_at).unwrap_or(0))
            .take(usize::try_from(artist_n).unwrap_or(0))
            .collect();
        let albums = if album_n > 0 {
            state
                .store
                .ss_albums(&AlbumList::ByName, album_at, album_n)
                .await?
        } else {
            Vec::new()
        };
        let tracks = if song_n > 0 {
            state
                .store
                .tracks_page(djinn_store::TrackSort::Title, song_at, song_n)
                .await?
                .items
        } else {
            Vec::new()
        };
        (artists, albums, tracks)
    } else {
        let most = artist_at.max(album_at).max(song_at) + artist_n.max(album_n).max(song_n);
        let found = state
            .store
            .search(query, most.clamp(1, MAX_LIST * 2))
            .await?;
        let artist_ids: HashSet<i64> = found.artists.iter().map(|a| a.id).collect();
        let artists: Vec<SsArtist> = state
            .store
            .ss_artists()
            .await?
            .into_iter()
            .filter(|a| artist_ids.contains(&a.id))
            .skip(usize::try_from(artist_at).unwrap_or(0))
            .take(usize::try_from(artist_n).unwrap_or(0))
            .collect();
        let mut albums = Vec::new();
        for al in found
            .albums
            .iter()
            .skip(usize::try_from(album_at).unwrap_or(0))
            .take(usize::try_from(album_n).unwrap_or(0))
        {
            if let Some(a) = state.store.ss_album(al.id).await? {
                albums.push(a);
            }
        }
        let tracks: Vec<Track> = found
            .tracks
            .into_iter()
            .skip(usize::try_from(song_at).unwrap_or(0))
            .take(usize::try_from(song_n).unwrap_or(0))
            .collect();
        (artists, albums, tracks)
    };
    let songs = Songs::load(state, user, &tracks).await?;
    let name = if method == "search2" {
        "searchResult2"
    } else {
        "searchResult3"
    };
    reply(
        Node::new(name)
            .items(
                artists
                    .iter()
                    .map(|a| artist_node(a, &liked))
                    .collect::<Vec<_>>(),
            )
            .items(
                albums
                    .iter()
                    .map(|a| album_node("album", a, &liked))
                    .collect::<Vec<_>>(),
            )
            .items(
                tracks
                    .iter()
                    .map(|t| songs.node("song", t))
                    .collect::<Vec<_>>(),
            ),
    )
}

async fn starred(state: &AppState, user: &User, method: &str) -> Answer {
    let liked = state.store.liked(user.id).await?;
    let ids = state.store.liked_ids(user.id).await?;
    let all_artists = state.store.ss_artists().await?;
    let artists: Vec<Node> = all_artists
        .iter()
        .filter(|a| ids.artists.contains(&a.id))
        .map(|a| artist_node(a, &ids))
        .collect();
    let mut albums = Vec::new();
    for al in &liked.albums {
        if let Some(a) = state.store.ss_album(al.id).await? {
            albums.push(album_node("album", &a, &ids));
        }
    }
    let songs = Songs::load(state, user, &liked.tracks).await?;
    let name = if method == "getstarred" {
        "starred"
    } else {
        "starred2"
    };
    reply(
        Node::new(name).items(artists).items(albums).items(
            liked
                .tracks
                .iter()
                .map(|t| songs.node("song", t))
                .collect::<Vec<_>>(),
        ),
    )
}

async fn star(state: &AppState, user: &User, params: &Params, on: bool) -> Answer {
    for id in params.ints("id") {
        state
            .store
            .set_like(user.id, LikeKind::Track, id, on)
            .await?;
    }
    for id in params.ints("albumId") {
        state
            .store
            .set_like(user.id, LikeKind::Album, id, on)
            .await?;
    }
    for id in params.ints("artistId") {
        state
            .store
            .set_like(user.id, LikeKind::Artist, id, on)
            .await?;
    }
    empty()
}

/// A finished play goes into your history; "now playing" ones are ignored.
async fn scrobble(state: &AppState, user: &User, params: &Params) -> Answer {
    if params.get("submission") == Some("false") {
        return empty();
    }
    for id in params.ints("id") {
        let length = state
            .store
            .track(id)
            .await?
            .and_then(|t| t.duration_ms)
            .unwrap_or(djinn_store::SKIP_BEFORE_MS);
        state.store.record_go_play(user.id, id, length).await?;
    }
    empty()
}

fn playlist_node(p: &GoPlaylistDetail) -> Node {
    let duration: i64 = p.songs.iter().filter_map(|t| t.duration_ms).sum::<i64>() / 1000;
    Node::new("playlist")
        .a("id", p.playlist.id.to_string())
        .a("name", p.playlist.name.as_str())
        .a("owner", p.playlist.owner.as_str())
        .a("public", p.playlist.shared)
        .a("songCount", i64::try_from(p.songs.len()).unwrap_or(0))
        .a("duration", duration)
        .a("created", iso(p.playlist.updated_at))
        .a("changed", iso(p.playlist.updated_at))
        .opt("coverArt", p.songs.iter().find_map(|t| t.art_hash.clone()))
}

async fn playlists(state: &AppState, user: &User) -> Answer {
    let mut nodes = Vec::new();
    for summary in state.store.go_playlists(user.id).await? {
        if let Some(detail) = state.store.go_playlist(user.id, summary.id).await? {
            nodes.push(playlist_node(&detail));
        }
    }
    reply(Node::new("playlists").items(nodes))
}

async fn playlist(state: &AppState, user: &User, params: &Params) -> Answer {
    let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
    let detail = state
        .store
        .go_playlist(user.id, id)
        .await?
        .ok_or_else(|| Failure::not_found("playlist"))?;
    let songs = Songs::load(state, user, &detail.songs).await?;
    let entries: Vec<Node> = detail
        .songs
        .iter()
        .map(|t| songs.node("entry", t))
        .collect();
    let node = Node {
        list: false,
        ..playlist_node(&detail).items(entries)
    };
    reply(node)
}

async fn create_playlist(state: &AppState, user: &User, params: &Params) -> Answer {
    let songs = params.ints("songId");
    let id = match params.int("playlistId") {
        Some(id) => {
            if !state
                .store
                .set_go_playlist_tracks(user.id, id, &songs)
                .await?
            {
                return Err(Failure::not_found("playlist"));
            }
            id
        }
        None => {
            let name = params
                .get("name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .ok_or_else(|| Failure::missing("name"))?;
            let name: String = name.chars().take(100).collect();
            let id = state.store.create_go_playlist(user.id, &name).await?;
            state
                .store
                .set_go_playlist_tracks(user.id, id, &songs)
                .await?;
            id
        }
    };
    let detail = state
        .store
        .go_playlist(user.id, id)
        .await?
        .ok_or_else(|| Failure::not_found("playlist"))?;
    let loaded = Songs::load(state, user, &detail.songs).await?;
    let entries: Vec<Node> = detail
        .songs
        .iter()
        .map(|t| loaded.node("entry", t))
        .collect();
    reply(playlist_node(&detail).items(entries))
}

async fn update_playlist(state: &AppState, user: &User, params: &Params) -> Answer {
    let id = params
        .int("playlistId")
        .ok_or_else(|| Failure::missing("playlistId"))?;
    let detail = state
        .store
        .go_playlist(user.id, id)
        .await?
        .filter(|d| d.playlist.user_id == user.id)
        .ok_or_else(|| Failure::not_found("playlist"))?;
    let shared = match params.get("public") {
        Some("true") => {
            if !state.store.permissions(user).await?.share_playlists {
                return Err(Failure::new(50, "your account can't share playlists"));
            }
            Some(true)
        }
        Some("false") => Some(false),
        _ => None,
    };
    let name: Option<String> = params
        .get("name")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(100).collect());
    if name.is_some() || shared.is_some() {
        state
            .store
            .update_go_playlist(user.id, id, name.as_deref(), shared)
            .await?;
    }
    let remove: HashSet<usize> = params
        .ints("songIndexToRemove")
        .into_iter()
        .filter_map(|i| usize::try_from(i).ok())
        .collect();
    let add = params.ints("songIdToAdd");
    if !remove.is_empty() || !add.is_empty() {
        let mut songs: Vec<i64> = detail
            .songs
            .iter()
            .enumerate()
            .filter(|(i, _)| !remove.contains(i))
            .map(|(_, t)| t.id)
            .collect();
        songs.extend(add);
        state
            .store
            .set_go_playlist_tracks(user.id, id, &songs)
            .await?;
    }
    empty()
}

async fn play_queue(state: &AppState, user: &User) -> Answer {
    let Some(resume) = state.store.resume(user.id).await? else {
        return empty();
    };
    let tracks = state.store.tracks_by_ids(&resume.queue).await?;
    let songs = Songs::load(state, user, &tracks).await?;
    let by_id: HashMap<i64, &Track> = tracks.iter().map(|t| (t.id, t)).collect();
    let entries: Vec<Node> = resume
        .queue
        .iter()
        .filter_map(|id| by_id.get(id))
        .map(|t| songs.node("entry", t))
        .collect();
    let current = usize::try_from(resume.position)
        .ok()
        .and_then(|i| resume.queue.get(i));
    reply(
        Node::new("playQueue")
            .opt("current", current.map(i64::to_string))
            .a("position", resume.position_ms)
            .a("username", user.username.as_str())
            .a("changed", iso(resume.updated_at))
            .a("changedBy", "DJINN")
            .items(entries),
    )
}

async fn save_play_queue(state: &AppState, user: &User, params: &Params) -> Answer {
    let queue = params.ints("id");
    let current = params.int("current");
    let position = current
        .and_then(|c| queue.iter().position(|id| *id == c))
        .and_then(|i| i64::try_from(i).ok())
        .unwrap_or(0);
    state
        .store
        .save_resume(
            user.id,
            &djinn_store::Resume {
                queue,
                position,
                position_ms: params.int("position").unwrap_or(0),
                updated_at: 0,
            },
        )
        .await?;
    empty()
}

async fn user_info(state: &AppState, user: &User, params: &Params) -> Answer {
    if let Some(name) = params.get("username") {
        if !name.eq_ignore_ascii_case(&user.username) && !user.is_admin() {
            return Err(Failure::new(50, "you can only see your own account"));
        }
    }
    let permissions = state.store.permissions(user).await?;
    reply(
        Node::new("user")
            .a("username", user.username.as_str())
            .a("scrobblingEnabled", true)
            .a("adminRole", false)
            .a("settingsRole", false)
            .a("downloadRole", permissions.downloads)
            .a("uploadRole", false)
            .a("playlistRole", true)
            .a("coverArtRole", false)
            .a("commentRole", false)
            .a("podcastRole", false)
            .a("streamRole", true)
            .a("jukeboxRole", false)
            .a("shareRole", false)
            .a("videoConversionRole", false)
            .items([Node::new("folder").text("1".into())]),
    )
}

/// The original file, whatever bit rate or format the app asks for.
async fn stream(
    state: &AppState,
    user: &User,
    params: &Params,
    device: &Device,
    request: Request,
    download: bool,
) -> Answer {
    let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
    let permissions = state.store.permissions(user).await?;
    if download && !permissions.downloads {
        return Err(Failure::new(50, "your account can't download songs"));
    }
    let track = state
        .store
        .track(id)
        .await?
        .ok_or_else(|| Failure::not_found("song"))?;
    let path = state
        .store
        .track_path(id)
        .await?
        .ok_or_else(|| Failure::not_found("song"))?;
    // Apps don't have sessions: each app on each address counts as one
    // device.
    let app = format!(
        "{}|{}",
        params.get("c").unwrap_or("app"),
        device.ip.as_deref().unwrap_or("")
    );
    let session_id = -i64::from_le_bytes(
        Md5::digest(app.as_bytes())[..8]
            .try_into()
            .unwrap_or([0; 8]),
    )
    .abs();
    let who = Listening {
        user_id: user.id,
        username: user.name().to_owned(),
        session_id,
        title: match &track.artist {
            Some(artist) => format!("{artist} - {}", track.title),
            None => track.title.clone(),
        },
        max_streams: permissions.max_streams,
    };
    Ok(Reply::Raw(
        send_song(state, &who, std::path::Path::new(&path), download, request).await,
    ))
}

async fn cover_art(state: &AppState, params: &Params) -> Answer {
    let id = params.get("id").ok_or_else(|| Failure::missing("id"))?;
    // Our cover ids are art hashes; some apps send an album id instead.
    let hash = if id.chars().all(|c| c.is_ascii_digit()) {
        let album = id.parse::<i64>().map_err(|_| Failure::not_found("cover"))?;
        state
            .store
            .ss_album(album)
            .await?
            .and_then(|a| a.art_hash)
            .ok_or_else(|| Failure::not_found("cover"))?
    } else {
        id.trim_start_matches("al-").to_owned()
    };
    let size = params.int("size").map(|s| if s <= 128 { 128 } else { 512 });
    match crate::library_api::serve_art(
        state,
        hash,
        crate::library_api::ArtQuery {
            size: size.and_then(|s| u32::try_from(s).ok()),
        },
    )
    .await
    {
        Ok(response) => Ok(Reply::Raw(response.into_response())),
        Err(_) => Ok(Reply::Raw(StatusCode::NOT_FOUND.into_response())),
    }
}

/// Songs that sound alike: from a song, or (`getSimilarSongs2`) from an
/// artist's best-known song.
async fn similar(state: &AppState, user: &User, method: &str, params: &Params) -> Answer {
    let id = params.int("id").ok_or_else(|| Failure::missing("id"))?;
    let count = usize::try_from(params.int("count").unwrap_or(50).clamp(1, 200)).unwrap_or(50);
    let seed = if method == "getsimilarsongs2" || state.store.track(id).await?.is_none() {
        state.store.artist(id).await?.and_then(|a| {
            a.top_tracks
                .first()
                .map(|t| t.track.id)
                .or(a.other_tracks.first().map(|t| t.id))
        })
    } else {
        Some(id)
    };
    let ids = match seed {
        Some(seed) => state
            .sound
            .read()
            .map(|index| index.similar_tracks(seed, count))
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let name = if method == "getsimilarsongs" {
        "similarSongs"
    } else {
        "similarSongs2"
    };
    songs_node(state, user, name, &ids).await
}

async fn top_songs(state: &AppState, user: &User, params: &Params) -> Answer {
    let name = params
        .get("artist")
        .ok_or_else(|| Failure::missing("artist"))?;
    let count = params.int("count").unwrap_or(50).clamp(1, 200);
    let found = state.store.search(name, 5).await?;
    let ids: Vec<i64> = match found
        .artists
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(name))
    {
        Some(a) => state
            .store
            .artist_top_tracks(a.id, count)
            .await?
            .iter()
            .map(|t| t.track.id)
            .collect(),
        None => Vec::new(),
    };
    songs_node(state, user, "topSongs", &ids).await
}

async fn artist_info(state: &AppState, method: &str, params: &Params) -> Answer {
    let name = if method == "getartistinfo" {
        "artistInfo"
    } else {
        "artistInfo2"
    };
    let id = params
        .get("id")
        .and_then(|v| v.trim_start_matches("ar").parse::<i64>().ok());
    let mut node = Node::new(name);
    if let Some(detail) = match id {
        Some(id) => state.store.artist(id).await?,
        None => None,
    } {
        if let Some(bio) = detail.bio {
            node = node.kid(Node::new("biography").text(bio));
        }
    }
    reply(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_escaping() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_700_000_000_000), "2023-11-14T22:13:20.000Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
        let mut out = String::new();
        escape("a<b & \"c\"", &mut out);
        assert_eq!(out, "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(decode_hex("736563726574").as_deref(), Some("secret"));
        assert_eq!(index_letter("The Beatles"), "B");
        assert_eq!(index_letter("2Pac"), "#");
    }

    /// A GET to the GO port; returns status, content type and body.
    async fn rest(state: &AppState, query: &str) -> (StatusCode, String, String) {
        use tower::ServiceExt;
        let req = Request::builder()
            .uri(format!("/rest/{query}"))
            .body(Body::empty())
            .unwrap();
        let res = crate::go::router(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let kind = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, kind, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn phone_apps_browse_and_play() {
        use djinn_store::{Permissions, Role, ScannedTrack};

        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-library/tests/fixtures/tagged.flac")
            .canonicalize()
            .unwrap();
        state
            .store
            .save_scanned(&[ScannedTrack {
                path: fixture.to_string_lossy().into_owned(),
                size: 1,
                title: "Signal".into(),
                artist: Some("The Testers".into()),
                album: Some("Fixtures".into()),
                genre: Some("Test".into()),
                duration_ms: Some(2000),
                ..ScannedTrack::default()
            }])
            .await
            .unwrap();
        let song = state.store.library_track_ids().await.unwrap()[0];
        let album = state
            .store
            .track(song)
            .await
            .unwrap()
            .unwrap()
            .album_id
            .unwrap();

        let role = state
            .store
            .create_role(
                "Away",
                &Permissions {
                    go: true,
                    go_remote: true,
                    ..Permissions::default()
                },
            )
            .await
            .unwrap();
        let sam = state
            .store
            .create_account(
                "sam",
                "x",
                Role::User,
                role.id,
                djinn_store::AccountStatus::Active,
            )
            .await
            .unwrap();
        state
            .store
            .set_subsonic_key(sam.id, "AppPass123")
            .await
            .unwrap();
        let token: String = Md5::digest(b"AppPass123salty")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let auth = format!("u=sam&t={token}&s=salty&v=1.16.1&c=test");

        // Token sign-in, JSON and XML.
        let (status, kind, body) = rest(&state, &format!("ping.view?{auth}&f=json")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(kind.starts_with("application/json"));
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["subsonic-response"]["status"], "ok", "{body}");
        let (_, kind, body) = rest(&state, &format!("ping?{auth}")).await;
        assert!(kind.starts_with("text/xml"));
        assert!(body.contains(r#"status="ok""#), "{body}");

        // A wrong password, or the account password, fails with code 40.
        let (_, _, body) = rest(&state, "ping?u=sam&p=x&c=test&f=json").await;
        assert!(body.contains("\"code\":40"), "{body}");
        let (_, _, body) = rest(
            &state,
            "ping?u=sam&p=enc:41707050617373313233&c=test&f=json",
        )
        .await;
        assert!(
            body.contains("\"status\":\"ok\""),
            "hex-encoded password: {body}"
        );

        let (_, _, body) = rest(&state, &format!("getOpenSubsonicExtensions?{auth}&f=json")).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["subsonic-response"]["openSubsonicExtensions"][0]["versions"][0],
            1
        );

        // Syncing everything with an empty search, then an album.
        let (_, _, body) = rest(
            &state,
            &format!("search3?{auth}&f=json&query=&songCount=500&albumCount=500&artistCount=500"),
        )
        .await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let result = &json["subsonic-response"]["searchResult3"];
        assert_eq!(result["song"][0]["title"], "Signal", "{body}");
        assert_eq!(result["song"][0]["suffix"], "flac");
        assert_eq!(result["album"][0]["songCount"], 1);
        assert_eq!(result["artist"][0]["name"], "The Testers");
        let (_, _, body) = rest(&state, &format!("getAlbum?{auth}&id={album}")).await;
        assert!(body.contains("<song "), "{body}");
        let (_, _, body) = rest(&state, &format!("getArtists?{auth}&f=json")).await;
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["subsonic-response"]["artists"]["index"][0]["name"],
            "T"
        );

        // Playing the file.
        let (status, kind, _) =
            rest(&state, &format!("stream?{auth}&id={song}&maxBitRate=128")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(kind, "audio/flac");
        let (_, _, body) = rest(&state, &format!("download?{auth}&id={song}&f=json")).await;
        assert!(
            body.contains("\"code\":50"),
            "no downloads for this role: {body}"
        );

        // Hearts and playlists are the same ones GO's page shows.
        rest(&state, &format!("star?{auth}&id={song}")).await;
        assert_eq!(state.store.liked_ids(sam.id).await.unwrap().tracks, [song]);
        let (_, _, body) = rest(
            &state,
            &format!("createPlaylist?{auth}&name=Car&songId={song}&songId={song}"),
        )
        .await;
        assert!(body.contains(r#"songCount="2""#), "{body}");
        assert_eq!(
            state.store.go_playlists(sam.id).await.unwrap()[0].name,
            "Car"
        );
        rest(&state, &format!("scrobble?{auth}&id={song}")).await;
        assert_eq!(
            state.store.recently_played(sam.id, 5).await.unwrap().len(),
            1
        );

        // A role that's home-only can't use it from away (tests count as away).
        let mut home = role.permissions.clone();
        home.go_remote = false;
        state
            .store
            .update_role(role.id, "Away", &home)
            .await
            .unwrap();
        let (_, _, body) = rest(&state, &format!("ping?{auth}&f=json")).await;
        assert!(body.contains("\"code\":50"), "{body}");
    }
}
