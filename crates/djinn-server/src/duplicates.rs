//! Copies of songs (admin): groups from `djinn_library::duplicates`,
//! worked out when asked and kept until the library, fingerprints,
//! identifications or dismissals change. The jukebox asks which songs are
//! copies of one another so the auto mix doesn't play two close together.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::duplicates::{self, Group, Reason, MAX_SHIFT, MIN_OVERLAP};
use djinn_store::DuplicateCandidate;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const PAGE: usize = 50;
/// At most this many songs dismissed at once (one group).
const MAX_DISMISS: usize = 100;

/// Groups of copies, as of `version`.
pub struct Duplicates {
    version: (i64, i64, i64, i64),
    pub groups: Vec<Group>,
    /// Song id to its group's index.
    by_song: HashMap<i64, usize>,
    /// The songs in groups.
    songs: HashMap<i64, DuplicateCandidate>,
}

impl Duplicates {
    /// The other copies of a song, if it has any.
    pub fn copies_of(&self, id: i64) -> impl Iterator<Item = i64> + '_ {
        self.by_song.get(&id).into_iter().flat_map(move |&g| {
            self.groups[g]
                .songs
                .iter()
                .copied()
                .filter(move |&s| s != id)
        })
    }
}

/// The current groups, worked out again if anything changed.
pub async fn current(state: &AppState) -> anyhow::Result<Arc<Duplicates>> {
    let mut cached = state.duplicates.lock().await;
    let version = state.store.duplicates_version().await?;
    if let Some(found) = cached.as_ref().filter(|d| d.version == version) {
        return Ok(Arc::clone(found));
    }
    let songs = state
        .store
        .duplicate_candidates(djinn_engine::PRINT_VERSION)
        .await?;
    let dismissed = state.store.dismissed_duplicates().await?;
    let found = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let groups = duplicates::find(&songs, &dismissed, |a, b| {
            djinn_engine::print_similarity(a, b, MAX_SHIFT, MIN_OVERLAP)
        });
        let mut by_song = HashMap::new();
        for (index, group) in groups.iter().enumerate() {
            for &id in &group.songs {
                by_song.insert(id, index);
            }
        }
        let songs: HashMap<i64, DuplicateCandidate> = songs
            .into_iter()
            .filter(|s| by_song.contains_key(&s.id))
            .map(|mut s| {
                s.print = None;
                (s.id, s)
            })
            .collect();
        tracing::debug!(
            groups = groups.len(),
            ms = started.elapsed().as_millis(),
            "looked for copies of songs"
        );
        Duplicates {
            version,
            groups,
            by_song,
            songs,
        }
    })
    .await?;
    let found = Arc::new(found);
    *cached = Some(Arc::clone(&found));
    Ok(found)
}

/// Works the groups out again in the background if anything changed, so
/// the jukebox has them without waiting.
pub fn refresh(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = current(&state).await {
            tracing::warn!(error = %format!("{e:#}"), "looking for copies of songs failed");
        }
    });
}

/// The jukebox's view: the groups last worked out, never waiting for new
/// ones (while they're being worked out, no song has copies).
pub struct KnownCopies(Arc<tokio::sync::Mutex<Option<Arc<Duplicates>>>>);

impl KnownCopies {
    pub fn new(state: &AppState) -> Self {
        Self(Arc::clone(&state.duplicates))
    }
}

impl crate::jukebox::Copies for KnownCopies {
    fn copies_of(&self, id: i64) -> Vec<i64> {
        let Ok(cached) = self.0.try_lock() else {
            return Vec::new();
        };
        cached
            .as_ref()
            .map(|d| d.copies_of(id).collect())
            .unwrap_or_default()
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/duplicates", get(list))
        .route("/library/duplicates/dismiss", post(dismiss))
}

#[derive(Serialize)]
struct Copy {
    id: i64,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    album_id: Option<i64>,
    path: String,
    format: Option<String>,
    sample_rate: Option<i64>,
    /// Average bit rate from the file size.
    kbps: Option<i64>,
    duration_ms: Option<i64>,
    /// The copy DJINN suggests keeping.
    keep: bool,
}

#[derive(Serialize)]
struct GroupView {
    reasons: Vec<Reason>,
    songs: Vec<Copy>,
}

#[derive(Serialize)]
struct Listing {
    /// Groups in all.
    total: usize,
    /// Songs in groups.
    songs: usize,
    groups: Vec<GroupView>,
    /// Without sound analysis only names and identifications are compared.
    sound_analysis: bool,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    offset: usize,
}

async fn list(
    _: AdminUser,
    State(state): State<AppState>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Listing>> {
    let found = current(&state).await?;
    let groups = found
        .groups
        .iter()
        .skip(page.offset)
        .take(PAGE)
        .map(|group| GroupView {
            reasons: group.reasons.clone(),
            songs: group
                .songs
                .iter()
                .enumerate()
                .filter_map(|(i, id)| found.songs.get(id).map(|s| copy(s, i == 0)))
                .collect(),
        })
        .collect();
    Ok(Json(Listing {
        total: found.groups.len(),
        songs: found.by_song.len(),
        groups,
        sound_analysis: crate::audio_api::analysis_on(&state).await,
    }))
}

fn copy(song: &DuplicateCandidate, keep: bool) -> Copy {
    Copy {
        id: song.id,
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        album_id: song.album_id,
        path: song.path.clone(),
        format: song.codec.clone(),
        sample_rate: song.sample_rate,
        kbps: song
            .duration_ms
            .filter(|&ms| ms > 0)
            .map(|ms| song.size * 8 / ms),
        duration_ms: song.duration_ms,
        keep,
    }
}

#[derive(Deserialize)]
struct Dismiss {
    ids: Vec<i64>,
}

/// These songs aren't copies of each other.
async fn dismiss(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Dismiss>,
) -> ApiResult<StatusCode> {
    if body.ids.len() < 2 || body.ids.len() > MAX_DISMISS {
        return Err(ApiError::bad_request("name between 2 and 100 songs"));
    }
    state.store.dismiss_duplicates(&body.ids).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::{Role, ScannedTrack};

    #[tokio::test]
    async fn lists_copies_and_forgets_dismissed_ones() {
        let state = AppState::for_tests().await;
        let song = |path: &str, codec: &str| ScannedTrack {
            path: path.into(),
            title: "Song".into(),
            artist: Some("Band".into()),
            duration_ms: Some(200_000),
            codec: Some(codec.into()),
            size: 4_000_000,
            ..Default::default()
        };
        state
            .store
            .save_scanned(&[song("/m/a.mp3", "mpeg"), song("/m/b.flac", "flac")])
            .await
            .unwrap();
        let admin = session(&state, "ana", Role::Admin).await;

        let (status, body) = get_body(&state, "/api/library/duplicates", &admin).await;
        assert_eq!(status, StatusCode::OK);
        let listing: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(listing["total"], 1);
        let first = &listing["groups"][0]["songs"][0];
        assert_eq!(first["path"], "/m/b.flac");
        assert_eq!(first["keep"], true);
        assert_eq!(listing["groups"][0]["reasons"][0], "name");

        let ids = state.store.library_track_ids().await.unwrap();
        let found = current(&state).await.unwrap();
        assert_eq!(found.copies_of(ids[0]).collect::<Vec<_>>(), [ids[1]]);

        let body = format!(r#"{{"ids":[{},{}]}}"#, ids[0], ids[1]);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/duplicates/dismiss",
            Some(&body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = get_body(&state, "/api/library/duplicates", &admin).await;
        let listing: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(listing["total"], 0);
    }
}
