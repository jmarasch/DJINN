//! The library editor's lyrics tools and its look-up by sound, for library
//! songs and for files waiting in an import alike.
//!
//! - **Find lyrics**: the song's own (a `.lrc` beside it or its tags), then
//!   LRCLIB. A library song's go into DJINN's lyrics like the background
//!   lookup's; an import's are written as a `.lrc` beside the waiting file,
//!   which goes into the library with it.
//! - **Generate** (songs with none) and **Make timed** (songs with words
//!   only; LRCLIB's timed version first): asks the AI listener for those
//!   songs ahead of its own rounds (`ai_asked`). An import's wait until it's
//!   in the library, since the listener works on library songs.
//! - **Identify by sound**: the song's fingerprint to AcoustID, then
//!   MusicBrainz, for songs whose tags are missing or wrong. What it finds
//!   comes back to the grid as changes to approve; nothing is written here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use djinn_library::compare;
use djinn_library::identify::{Hint, Identifier};
use djinn_library::lyrics::{self as found, Found};
use djinn_store::NewLyrics;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::protected::Elevated;
use crate::settings;
use crate::state::AppState;

/// The most songs one ask covers (LRCLIB and AcoustID go one at a time).
const MAX_SONGS: usize = 200;
/// Between LRCLIB lookups, as the background lookup does.
const LOOKUP_PAUSE: Duration = Duration::from_millis(400);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/editor/lyrics/find", post(find))
        .route("/editor/lyrics/timed", post(timed))
        .route("/editor/lyrics/generate", post(generate))
        .route("/editor/identify", post(identify))
}

#[derive(Deserialize)]
struct Ask {
    ids: Vec<i64>,
    /// The ids are files waiting in an import.
    #[serde(default)]
    staged: bool,
}

/// A song as these tools need it.
struct Song {
    id: i64,
    path: PathBuf,
    artist: String,
    title: String,
    album: Option<String>,
    duration_ms: Option<i64>,
    /// 0 none, 1 words, 2 timed.
    lyrics: i64,
}

fn lyrics_level(found: Option<&Found>) -> i64 {
    match found {
        Some(f) if f.synced.is_some() => 2,
        Some(f) if f.plain.is_some() => 1,
        _ => 0,
    }
}

async fn songs_of(state: &AppState, ask: &Ask) -> ApiResult<Vec<Song>> {
    if ask.ids.is_empty() || ask.ids.len() > MAX_SONGS {
        return Err(ApiError::bad_request(format!(
            "choose 1 to {MAX_SONGS} songs"
        )));
    }
    if ask.staged {
        let files = state.store.import_files_by_id(&ask.ids).await?;
        let original_years = djinn_library::original_years(&state.store).await?;
        return Ok(tokio::task::spawn_blocking(move || {
            files
                .into_iter()
                .map(|file| {
                    let path = PathBuf::from(&file.path);
                    let tags = compare::read(&path, original_years).unwrap_or_default();
                    Song {
                        id: file.id,
                        lyrics: lyrics_level(found::local(&path).as_ref()),
                        duration_ms: compare::length_ms(&path),
                        artist: tags.artist.or(tags.album_artist).unwrap_or_default(),
                        title: tags.title,
                        album: tags.album,
                        path,
                    }
                })
                .collect()
        })
        .await
        .map_err(anyhow::Error::from)?);
    }
    Ok(state
        .store
        .editor_songs_by_id(&ask.ids)
        .await?
        .into_iter()
        .map(|s| Song {
            id: s.id,
            path: PathBuf::from(&s.path),
            artist: s.artist.or(s.album_artist).unwrap_or_default(),
            title: s.title,
            album: s.album,
            duration_ms: s.duration_ms,
            lyrics: s.lyrics,
        })
        .collect())
}

fn to_new(found: &Found) -> NewLyrics<'_> {
    NewLyrics {
        synced: found.synced.as_deref(),
        plain: found.plain.as_deref(),
        instrumental: found.instrumental,
        source: found.source.as_str(),
    }
}

/// Lyrics found for a waiting file go beside it, as the library reads them.
fn write_sidecar(song: &Path, found: &Found) -> Result<(), String> {
    let Some(text) = found::export_text(found.synced.as_deref(), found.plain.as_deref()) else {
        return Ok(());
    };
    std::fs::write(song.with_extension("lrc"), text).map_err(|e| e.to_string())
}

async fn lrclib_get(lrclib: &found::Lrclib, song: &Song) -> Result<Option<Found>, String> {
    if song.artist.trim().is_empty() || song.title.trim().is_empty() {
        return Ok(None);
    }
    let query = found::Query {
        artist: &song.artist,
        title: &song.title,
        album: song.album.as_deref(),
        duration_ms: song.duration_ms,
    };
    let answer = lrclib.get(&query).await.map_err(|e| e.to_string());
    tokio::time::sleep(LOOKUP_PAUSE).await;
    answer
}

#[derive(Serialize)]
struct Outcome {
    id: i64,
    /// "had", "timed", "words", "instrumental", "none", "asked" (the AI),
    /// "skipped" or "failed".
    result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct Outcomes {
    songs: Vec<Outcome>,
    /// The AI listener takes work (Admin > AI > Lyrics listening); asked
    /// songs wait until it does.
    ai_on: bool,
}

async fn ai_on(state: &AppState) -> ApiResult<bool> {
    Ok(settings::flag(&state.store, crate::ai_work::WORK_ON, false).await?)
}

fn outcome(id: i64, result: &'static str) -> Outcome {
    Outcome {
        id,
        result,
        error: None,
    }
}

/// Finds lyrics: the song's own, then LRCLIB.
async fn find(
    _: Elevated,
    State(state): State<AppState>,
    Json(ask): Json<Ask>,
) -> ApiResult<Json<Outcomes>> {
    let songs = songs_of(&state, &ask).await?;
    let lrclib = found::Lrclib::new().map_err(anyhow::Error::from)?;
    let mut out = Vec::new();
    for song in songs {
        if song.lyrics > 0 {
            out.push(outcome(song.id, "had"));
            continue;
        }
        let found = match lrclib_get(&lrclib, &song).await {
            Ok(found) => found,
            Err(error) => {
                out.push(Outcome {
                    id: song.id,
                    result: "failed",
                    error: Some(error),
                });
                continue;
            }
        };
        let result = match &found {
            Some(f) if f.instrumental => "instrumental",
            Some(f) if f.synced.is_some() => "timed",
            Some(_) => "words",
            None => "none",
        };
        if ask.staged {
            if let Some(f) = found.as_ref().filter(|f| !f.instrumental) {
                if let Err(error) = write_sidecar(&song.path, f) {
                    out.push(Outcome {
                        id: song.id,
                        result: "failed",
                        error: Some(error),
                    });
                    continue;
                }
            }
        } else {
            state
                .store
                .save_looked_up_lyrics(song.id, found.as_ref().map(to_new).as_ref())
                .await?;
        }
        out.push(outcome(song.id, result));
    }
    Ok(Json(Outcomes {
        songs: out,
        ai_on: ai_on(&state).await?,
    }))
}

/// Asks the AI for these songs (an import's: once it's in the library).
async fn ask_ai(state: &AppState, staged: bool, ids: &[i64], kind: &str) -> ApiResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    if staged {
        state.store.set_import_ai_want(ids, kind).await?;
    } else {
        state.store.ai_ask(ids, kind).await?;
    }
    Ok(())
}

/// Timed lyrics for songs with words only: LRCLIB's when it has them,
/// otherwise the AI times the words.
async fn timed(
    _: Elevated,
    State(state): State<AppState>,
    Json(ask): Json<Ask>,
) -> ApiResult<Json<Outcomes>> {
    let songs = songs_of(&state, &ask).await?;
    let lrclib = found::Lrclib::new().map_err(anyhow::Error::from)?;
    let mut out = Vec::new();
    let mut for_ai = Vec::new();
    for song in songs {
        match song.lyrics {
            2 => out.push(outcome(song.id, "had")),
            0 => out.push(outcome(song.id, "skipped")),
            _ => {
                let synced = lrclib_get(&lrclib, &song)
                    .await
                    .ok()
                    .flatten()
                    .filter(|f| f.synced.is_some());
                let saved = match &synced {
                    Some(f) if ask.staged => write_sidecar(&song.path, f).is_ok(),
                    Some(f) => state.store.save_timed_lyrics(song.id, &to_new(f)).await?,
                    None => false,
                };
                if saved {
                    out.push(outcome(song.id, "timed"));
                } else {
                    for_ai.push(song.id);
                    out.push(outcome(song.id, "asked"));
                }
            }
        }
    }
    ask_ai(&state, ask.staged, &for_ai, "sync").await?;
    Ok(Json(Outcomes {
        songs: out,
        ai_on: ai_on(&state).await?,
    }))
}

/// Lyrics made by the AI listener for songs that have none.
async fn generate(
    _: Elevated,
    State(state): State<AppState>,
    Json(ask): Json<Ask>,
) -> ApiResult<Json<Outcomes>> {
    let songs = songs_of(&state, &ask).await?;
    let mut out = Vec::new();
    let mut for_ai = Vec::new();
    for song in songs {
        if song.lyrics > 0 {
            out.push(outcome(song.id, "had"));
        } else {
            for_ai.push(song.id);
            out.push(outcome(song.id, "asked"));
        }
    }
    ask_ai(&state, ask.staged, &for_ai, "listen").await?;
    Ok(Json(Outcomes {
        songs: out,
        ai_on: ai_on(&state).await?,
    }))
}

// --- Identify by sound ---

#[derive(Serialize)]
struct Heard {
    id: i64,
    /// What AcoustID and MusicBrainz say it is; None if nothing matched.
    found: Option<HeardAs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct HeardAs {
    /// How sure AcoustID is, 0 to 1.
    score: f32,
    title: String,
    artist: String,
    album: Option<String>,
    year: Option<i64>,
    genre: Option<String>,
}

/// Each song's fingerprint looked up. Unlike the background identifying,
/// a match needn't agree with the song's tags: this is for fixing them, and
/// the admin sees every change before approving.
async fn identify(
    _: Elevated,
    State(state): State<AppState>,
    Json(ask): Json<Ask>,
) -> ApiResult<Json<Vec<Heard>>> {
    let key = crate::identify_api::key(&state).await?;
    let contact = crate::identify_api::contact(&state).await?;
    if key.is_empty() || contact.is_empty() {
        return Err(ApiError::bad_request(
            "identifying by sound needs an AcoustID key and the MusicBrainz contact (Admin > Library)",
        ));
    }
    let songs = songs_of(&state, &ask).await?;
    let mut identifier = Identifier::new(&key, &contact).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut out = Vec::new();
    for song in songs {
        let path = song.path.clone();
        let print = tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(|| djinn_engine::fingerprint(&path)).unwrap_or(Ok(None))
        })
        .await
        .map_err(anyhow::Error::from)?;
        let print = match print {
            Ok(Some(print)) => print,
            Ok(None) => {
                out.push(Heard {
                    id: song.id,
                    found: None,
                    error: Some("too short to identify".into()),
                });
                continue;
            }
            Err(e) => {
                out.push(Heard {
                    id: song.id,
                    found: None,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let seconds = u64::try_from(song.duration_ms.unwrap_or(0).max(0) / 1000).unwrap_or(0);
        match identifier.identify(&print, seconds, &Hint::default()).await {
            Ok(found) => out.push(Heard {
                id: song.id,
                found: found.map(|f| HeardAs {
                    score: f.score,
                    title: f.title,
                    artist: f.artist,
                    album: f.album,
                    year: f.year,
                    genre: f.genres.into_iter().next(),
                }),
                error: None,
            }),
            // A bad key or the service down: say so for the rest too.
            Err(e) => {
                return Err(ApiError::bad_request(e.to_string()));
            }
        }
    }
    Ok(Json(out))
}
