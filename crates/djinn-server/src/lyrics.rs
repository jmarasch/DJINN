//! Lyrics: a background job finds them (a `.lrc` beside each song, its tags,
//! then LRCLIB when the admin allows), and routes serve them to the party
//! app, GO, the public lyrics page (`/lyrics`, which can be turned off) and
//! an export of `.lrc` files laid out like the library.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::lyrics::{self as found, Found, Line};
use djinn_store::{LyricsCounts, NewLyrics, StoredLyrics};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::events::{NowPlaying, PlayerState};
use crate::settings;
use crate::state::AppState;

/// Setting key: ask LRCLIB about songs without lyrics. Off unless "1".
pub const LYRICS_LOOKUP: &str = "lyrics_lookup";
/// Setting key: the public lyrics page; on unless "0".
pub const LYRICS_PAGE: &str = "lyrics_page";

/// Files read between checks on whether heavy work may run.
const READ_BATCH: i64 = 200;
/// Songs asked about per batch, and the pause between questions, so a free
/// service isn't hammered.
const LOOKUP_BATCH: i64 = 50;
const LOOKUP_PAUSE: Duration = Duration::from_millis(400);

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Asked to run again while running (a library check, a setting turned on).
static AGAIN: AtomicBool = AtomicBool::new(false);
/// What the job is doing, for Admin: "", "reading" or "looking up".
static PHASE: std::sync::Mutex<&'static str> = std::sync::Mutex::new("");

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks/{id}/lyrics", get(track_lyrics))
        .route("/lyrics/status", get(status))
        .route("/lyrics/settings", get(get_settings).put(put_settings))
        .route("/lyrics/recheck", post(recheck))
        .route("/lyrics/export", get(export))
        .route("/lyrics-page", get(page))
        .route("/lyrics-page/events", get(crate::sse::lyrics_events))
        .route("/lyrics-page/art/{hash}", get(page_art))
}

// --- The job ---

/// Starts looking for lyrics in the background, or has a running search go
/// round again when it's done.
pub fn start(state: &AppState) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        AGAIN.store(true, Ordering::Release);
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        tell(&state, true).await;
        loop {
            AGAIN.store(false, Ordering::Release);
            if let Err(e) = read_files(&state).await {
                tracing::warn!(error = %format!("{e:#}"), "reading lyrics failed");
            }
            if settings::flag(&state.store, LYRICS_LOOKUP, true)
                .await
                .unwrap_or(false)
            {
                if let Err(e) = look_up(&state).await {
                    tracing::warn!(error = %format!("{e:#}"), "looking up lyrics failed");
                }
            }
            if !AGAIN.load(Ordering::Acquire) {
                break;
            }
        }
        set_phase("");
        RUNNING.store(false, Ordering::Release);
        tell(&state, true).await;
    });
}

fn set_phase(phase: &'static str) {
    if let Ok(mut p) = PHASE.lock() {
        *p = phase;
    }
}

/// How finding lyrics is going, for the Library tab's progress box.
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub running: bool,
    /// "reading", "looking up", or "" (idle, or waiting its turn: `held`).
    pub phase: &'static str,
    pub held: Option<crate::background::Held>,
    pub songs: i64,
    /// Songs with lyrics, timed or not.
    pub words: i64,
    pub timed: i64,
    /// Files still to read.
    pub unread: i64,
    /// Songs without lyrics still to ask LRCLIB about; 0 with lookups off.
    pub to_ask: i64,
}

pub async fn progress(state: &AppState) -> ApiResult<Progress> {
    let counts = state.store.lyrics_counts().await?;
    let running = RUNNING.load(Ordering::Acquire);
    let phase = PHASE.lock().map(|p| *p).unwrap_or("");
    let lookups = settings::flag(&state.store, LYRICS_LOOKUP, true).await?;
    Ok(Progress {
        running,
        phase,
        held: (running && phase.is_empty())
            .then(|| crate::background::waiting(state))
            .flatten(),
        songs: counts.songs,
        words: counts.synced + counts.plain,
        timed: counts.synced,
        unread: counts.unread,
        to_ask: if lookups { counts.not_looked_up } else { 0 },
    })
}

/// When progress was last sent, so a quick batch doesn't flood pages.
static SENT_AT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
const SEND_EVERY_MS: i64 = 3_000;

/// Tells open pages how it's going; `now` skips the pause between updates.
async fn tell(state: &AppState, now: bool) {
    let at = djinn_store::now_ms();
    if !now && at - SENT_AT.load(Ordering::Relaxed) < SEND_EVERY_MS {
        return;
    }
    SENT_AT.store(at, Ordering::Relaxed);
    if let Ok(p) = progress(state).await {
        let _ = state.events.send(crate::events::ServerEvent::Lyrics(p));
    }
}

fn to_new(found: &Found) -> NewLyrics<'_> {
    NewLyrics {
        synced: found.synced.as_deref(),
        plain: found.plain.as_deref(),
        instrumental: found.instrumental,
        source: found.source.as_str(),
    }
}

async fn read_files(state: &AppState) -> anyhow::Result<()> {
    loop {
        let batch = state.store.lyrics_to_read(READ_BATCH).await?;
        if batch.is_empty() {
            return Ok(());
        }
        let go_on = crate::background::wait_turn(
            state,
            || async { true },
            || async {
                set_phase("");
                tell(state, true).await;
            },
        )
        .await;
        if !go_on || *state.shutdown.borrow() {
            return Ok(());
        }
        set_phase("reading");
        let paths: Vec<PathBuf> = batch.iter().map(|t| PathBuf::from(&t.path)).collect();
        let results = tokio::task::spawn_blocking(move || {
            paths.iter().map(|p| found::local(p)).collect::<Vec<_>>()
        })
        .await?;
        for (song, result) in batch.iter().zip(results) {
            let new = result.as_ref().map(to_new).unwrap_or(NewLyrics::NONE);
            state
                .store
                .save_read_lyrics(song.id, song.mtime, &new)
                .await?;
        }
        tell(state, false).await;
    }
}

async fn look_up(state: &AppState) -> anyhow::Result<()> {
    let lrclib = found::Lrclib::new()?;
    loop {
        let batch = state.store.lyrics_to_look_up(LOOKUP_BATCH).await?;
        if batch.is_empty() {
            return Ok(());
        }
        set_phase("looking up");
        for song in batch {
            if *state.shutdown.borrow()
                || !settings::flag(&state.store, LYRICS_LOOKUP, true).await?
            {
                return Ok(());
            }
            let query = found::Query {
                artist: &song.artist,
                title: &song.title,
                album: song.album.as_deref(),
                duration_ms: song.duration_ms,
            };
            // A failed lookup (offline, LRCLIB down) stops this round
            // without marking anything, so the songs are asked again later.
            let answer = lrclib.get(&query).await?;
            state
                .store
                .save_looked_up_lyrics(song.id, answer.as_ref().map(to_new).as_ref())
                .await?;
            tell(state, false).await;
            tokio::time::sleep(LOOKUP_PAUSE).await;
        }
    }
}

// --- What pages get ---

/// A song's lyrics as pages show them: timed lines when there are some,
/// otherwise the plain words.
#[derive(Serialize)]
pub struct LyricsView {
    lines: Option<Vec<Line>>,
    plain: Option<String>,
    instrumental: bool,
    source: String,
}

impl From<StoredLyrics> for LyricsView {
    fn from(l: StoredLyrics) -> Self {
        let lines = l
            .synced
            .as_deref()
            .map(found::parse)
            .filter(|lines| !lines.is_empty());
        LyricsView {
            lines,
            plain: l.plain,
            instrumental: l.instrumental,
            source: l.source,
        }
    }
}

pub async fn lyrics_for(state: &AppState, track_id: i64) -> ApiResult<Option<LyricsView>> {
    Ok(state.store.lyrics(track_id).await?.map(LyricsView::from))
}

async fn track_lyrics(
    _: CurrentUser,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
) -> ApiResult<Json<Option<LyricsView>>> {
    Ok(Json(lyrics_for(&state, id).await?))
}

/// A song whose words match a search, with the line that matched best.
#[derive(Serialize)]
pub struct LyricHit {
    pub track: djinn_store::Track,
    pub line: String,
}

/// Songs whose lyrics have every word typed, for search; only for two or
/// more words, since one word matches too much.
pub async fn search(state: &AppState, query: &str, limit: i64) -> ApiResult<Vec<LyricHit>> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.len() < 2 {
        return Ok(Vec::new());
    }
    let hits = state.store.search_lyrics(query, limit).await?;
    let ids: Vec<i64> = hits.iter().map(|h| h.0).collect();
    let mut tracks = state.store.tracks_by_ids(&ids).await?.into_iter();
    let mut out = Vec::new();
    for (id, plain) in hits {
        let Some(track) = tracks.find(|t| t.id == id) else {
            continue;
        };
        out.push(LyricHit {
            line: best_line(&plain, &words),
            track,
        });
    }
    Ok(out)
}

/// The line with the most of the words, the earliest of equals.
fn best_line(plain: &str, words: &[String]) -> String {
    let mut best = ("", 0usize);
    for line in plain.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let lower = line.to_lowercase();
        let score = words.iter().filter(|w| lower.contains(w.as_str())).count();
        if score > best.1 {
            best = (line, score);
        }
    }
    best.0.to_owned()
}

// --- Admin ---

#[derive(Serialize)]
struct Status {
    #[serde(flatten)]
    counts: LyricsCounts,
    running: bool,
    /// "reading", "looking up", or "" when idle.
    phase: &'static str,
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    Ok(Json(Status {
        counts: state.store.lyrics_counts().await?,
        running: RUNNING.load(Ordering::Acquire),
        phase: PHASE.lock().map(|p| *p).unwrap_or(""),
    }))
}

#[derive(Serialize, Deserialize)]
struct LyricsSettings {
    lookup: bool,
    page: bool,
}

async fn get_settings(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<LyricsSettings>> {
    Ok(Json(LyricsSettings {
        lookup: settings::flag(&state.store, LYRICS_LOOKUP, true).await?,
        page: page_on(&state).await?,
    }))
}

async fn put_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<LyricsSettings>,
) -> ApiResult<StatusCode> {
    let was = settings::flag(&state.store, LYRICS_LOOKUP, true).await?;
    settings::set_flag(&state.store, LYRICS_LOOKUP, body.lookup).await?;
    settings::set_flag(&state.store, LYRICS_PAGE, body.page).await?;
    if body.lookup && !was {
        start(&state);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn recheck(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.recheck_lyrics().await?;
    start(&state);
    Ok(StatusCode::ACCEPTED)
}

/// Where a song's `.lrc` goes in the export: under its music folder's name,
/// at the same place as the song, named like it.
fn export_name(song: &Path, roots: &[PathBuf]) -> String {
    let lrc = song.with_extension("lrc");
    let relative = roots
        .iter()
        .filter(|root| lrc.starts_with(root))
        .max_by_key(|root| root.components().count())
        .and_then(|root| {
            let inside = lrc.strip_prefix(root).ok()?;
            let name = root.file_name().map(PathBuf::from).unwrap_or_default();
            Some(name.join(inside))
        })
        .unwrap_or_else(|| {
            lrc.components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .collect()
        });
    relative
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn export(_: AdminUser, State(state): State<AppState>) -> ApiResult<Response> {
    let roots = crate::folders::library_roots(&state).await?;
    let songs = state.store.lyrics_for_export().await?;
    let zip = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        use zip::write::SimpleFileOptions;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut used = std::collections::HashSet::new();
        for (path, synced, plain) in songs {
            let Some(text) = found::export_text(synced.as_deref(), plain.as_deref()) else {
                continue;
            };
            let name = export_name(Path::new(&path), &roots);
            // Two songs differing only by extension share a .lrc name.
            if name.is_empty() || !used.insert(name.clone()) {
                continue;
            }
            zip.start_file(name, options)?;
            zip.write_all(text.as_bytes())?;
        }
        Ok(zip.finish()?.into_inner())
    })
    .await??;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"djinn-lyrics.zip\"",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        zip,
    )
        .into_response())
}

// --- The lyrics page ---

pub async fn page_on(state: &AppState) -> ApiResult<bool> {
    Ok(settings::flag(&state.store, LYRICS_PAGE, true).await?)
}

pub async fn check_page(state: &AppState) -> ApiResult<()> {
    if page_on(state).await? {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "the lyrics page is turned off. The host can turn it on under Admin > Library.",
        ))
    }
}

#[derive(Serialize)]
struct PageSnapshot {
    #[serde(flatten)]
    now_playing: NowPlaying,
    player: PlayerState,
    lyrics: Option<LyricsView>,
    /// Requests waiting, for the page's Coming up.
    queue: Vec<djinn_store::QueueItem>,
    /// The join link, for the page's QR code.
    join_url: String,
}

#[derive(Deserialize)]
struct PageQuery {
    /// Just the lyrics of this song (the page asks when the song changes).
    track: Option<i64>,
    /// Which room to show (Main when missing).
    room: Option<i64>,
}

async fn page(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Response> {
    check_page(&state).await?;
    let room = state
        .rooms
        .leader(query.room.unwrap_or(djinn_store::MAIN_ROOM));
    let (now_playing, player) = {
        let jukebox = state.sound_jukebox_for(room);
        let jukebox = jukebox.lock().await;
        (jukebox.now_playing(), jukebox.player_state())
    };
    let playing = now_playing.current.as_ref().map(|e| e.track.id);
    if let Some(track) = query.track {
        // Only the song playing, so the page shows nothing it couldn't.
        if Some(track) != playing {
            return Err(ApiError::not_found());
        }
        return Ok(Json(lyrics_for(&state, track).await?).into_response());
    }
    let lyrics = match playing {
        Some(id) => lyrics_for(&state, id).await?,
        None => None,
    };
    Ok(Json(PageSnapshot {
        now_playing,
        player,
        lyrics,
        queue: state.store.queue(room).await?,
        join_url: state.join_url.get(),
    })
    .into_response())
}

async fn page_art(
    State(state): State<AppState>,
    UrlPath(hash): UrlPath<String>,
    Query(query): Query<crate::library_api::ArtQuery>,
) -> ApiResult<impl IntoResponse> {
    check_page(&state).await?;
    crate::library_api::serve_art(&state, hash, query).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn lyrics_reach_members_search_and_the_page_until_its_off() {
        let state = AppState::for_tests().await;
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: "/music/Band/Song.flac".into(),
                size: 1,
                mtime: 1,
                title: "Song".into(),
                artist: Some("Band".into()),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.tracks_by_ids(&[1]).await.unwrap()[0].id;
        let words = NewLyrics {
            synced: Some("[00:01.00]Is this the real life\n[00:05.00]Is this just fantasy"),
            plain: Some("Is this the real life\nIs this just fantasy"),
            instrumental: false,
            source: "lrc",
        };
        state.store.save_read_lyrics(id, 1, &words).await.unwrap();

        let path = format!("/api/tracks/{id}/lyrics");
        let (status, _) = call(&state, Method::GET, &path, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let sam = session(&state, "sam", Role::User).await;
        let (status, body) = get_body(&state, &path, &sam).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains(r#"{"ms":5000,"text":"Is this just fantasy"}"#),
            "{body}"
        );

        let (_, body) = get_body(&state, "/api/search?q=just%20fantas", &sam).await;
        assert!(body.contains(r#""line":"Is this just fantasy""#), "{body}");
        let (_, body) = get_body(&state, "/api/search?q=fantasy", &sam).await;
        assert!(
            body.contains(r#""lyrics":[]"#),
            "one word finds nothing: {body}"
        );

        let (status, _) = call(&state, Method::GET, "/api/lyrics-page", None, None).await;
        assert_eq!(status, StatusCode::OK, "no sign-in needed");
        // Only the song playing.
        let asked = format!("/api/lyrics-page?track={id}");
        let (status, _) = call(&state, Method::GET, &asked, None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(&state, Method::GET, "/api/lyrics/status", None, Some(&sam)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        state.store.set_setting(LYRICS_PAGE, "0").await.unwrap();
        let (status, _) = call(&state, Method::GET, "/api/lyrics-page", None, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::GET, "/api/lyrics-page/events", None, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn exports_follow_the_library_layout() {
        let roots = vec![PathBuf::from("/music"), PathBuf::from("/data/More Music")];
        assert_eq!(
            export_name(
                Path::new("/music/Queen/Opera/11 Bohemian Rhapsody.flac"),
                &roots
            ),
            "music/Queen/Opera/11 Bohemian Rhapsody.lrc"
        );
        assert_eq!(
            export_name(Path::new("/data/More Music/A/b.mp3"), &roots),
            "More Music/A/b.lrc"
        );
        assert_eq!(
            export_name(Path::new("/elsewhere/x.ogg"), &roots),
            "elsewhere/x.lrc"
        );
    }

    #[test]
    fn the_matching_line_has_the_most_words() {
        let plain = "Is this the real life\nIs this just fantasy\nCaught in a landslide";
        let words = vec!["just".to_owned(), "fantasy".to_owned()];
        assert_eq!(best_line(plain, &words), "Is this just fantasy");
    }
}
