//! Smart playlists: songs and artists that sound alike, and search by
//! description ("80's hair metal"). The sound index and the song facts
//! live in memory and are rebuilt when analysis or artist lookups finish.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::sound::SoundIndex;
use djinn_library::vibe::{self, SongFacts, Vocabulary};
use djinn_store::{ArtistSummary, Track};
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 30;
const MAX_LIMIT: usize = 100;
/// Songs shown for a description.
const DESCRIBED_SONGS: usize = 60;
const SIMILAR_ARTISTS: usize = 12;
/// Artists need this many songs to be suggested on sound alone.
const MIN_SOUND_SONGS: usize = 2;

/// What search by description works from.
#[derive(Default)]
pub struct Catalog {
    songs: Vec<SongFacts>,
    vocab: Vocabulary,
    /// Each artist's genres, normalized.
    artist_genres: HashMap<i64, HashSet<String>>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks/{id}/similar", get(similar_tracks))
        .route("/artists/{id}/similar", get(similar_artists))
}

/// Rebuilds the sound index and song facts from the database.
pub async fn refresh(state: &AppState) {
    let loaded = async {
        let rows = state
            .store
            .sound_rows(djinn_engine::FEATURES_VERSION)
            .await?;
        let facts = state.store.track_facts().await?;
        let artists = state.store.artist_genres().await?;
        Ok::<_, djinn_store::StoreError>((rows, facts, artists))
    }
    .await;
    let (rows, facts, artists) = match loaded {
        Ok(loaded) => loaded,
        Err(e) => {
            tracing::error!(error = %e, "loading song facts failed");
            return;
        }
    };
    let built = tokio::task::spawn_blocking(move || {
        // What the analyzer heard, by song: tempo, punch, mode.
        let heard: HashMap<i64, (f32, f32, f32)> = rows
            .iter()
            .filter(|r| r.features.len() > 11)
            .map(|r| {
                (
                    r.track_id,
                    (r.features[0].exp2(), r.features[1], r.features[11]),
                )
            })
            .collect();
        let split = |genres: &str| -> Vec<String> {
            genres
                .split(',')
                .map(vibe::normalize)
                .filter(|g| !g.is_empty())
                .collect()
        };
        let songs: Vec<SongFacts> = facts
            .into_iter()
            .map(|f| {
                let heard = heard.get(&f.id);
                let mut genres = split(&f.genres);
                genres.sort();
                genres.dedup();
                SongFacts {
                    id: f.id,
                    year: f.year,
                    genres,
                    popularity: f.popularity,
                    bpm: heard.map(|h| h.0),
                    punch: heard.map(|h| h.1),
                    mode: heard.map(|h| h.2),
                }
            })
            .collect();
        let all_genres: HashSet<String> = songs.iter().flat_map(|s| s.genres.clone()).collect();
        let vocab = Vocabulary::new(
            all_genres.iter().map(String::as_str),
            artists.iter().map(|a| (a.id, a.name.as_str())),
        );
        let artist_genres = artists
            .iter()
            .map(|a| (a.id, split(&a.genres).into_iter().collect()))
            .collect();
        (
            SoundIndex::build(rows),
            Catalog {
                songs,
                vocab,
                artist_genres,
            },
        )
    })
    .await;
    let Ok((index, catalog)) = built else { return };
    tracing::info!(
        analyzed = index.len(),
        songs = catalog.songs.len(),
        "smart search ready"
    );
    if let Ok(mut current) = state.sound.write() {
        *current = index;
    }
    if let Ok(mut current) = state.catalog.write() {
        *current = catalog;
    }
}

/// Songs and artists matching a description.
#[derive(Serialize)]
pub struct Described {
    /// What was understood, like ["1980s", "hair metal"].
    pub summary: Vec<String>,
    /// Words that weren't understood.
    pub unknown: Vec<String>,
    pub tracks: Vec<Track>,
    /// Songs that fit in all (more than shown).
    pub total: usize,
    pub artists: Vec<ArtistSummary>,
}

/// If `query` describes music, the songs and artists that fit.
pub async fn describe(state: &AppState, query: &str) -> ApiResult<Option<Described>> {
    let found = {
        let (Ok(catalog), Ok(sound)) = (state.catalog.read(), state.sound.read()) else {
            return Ok(None);
        };
        vibe::parse(query, &catalog.vocab).map(|intent| {
            let (ids, total) = vibe::rank(&intent, &catalog.songs, &sound, DESCRIBED_SONGS);
            (intent, ids, total)
        })
    };
    let Some((intent, ids, total)) = found else {
        return Ok(None);
    };
    let tracks = state.store.tracks_by_ids(&ids).await?;
    // The artists behind the songs, most songs first.
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for track in &tracks {
        if let Some(main) = track.credits.iter().find(|c| !c.featured) {
            *counts.entry(main.id).or_default() += 1;
        }
    }
    let mut artist_ids: Vec<i64> = counts.keys().copied().collect();
    artist_ids.sort_by_key(|id| std::cmp::Reverse(counts[id]));
    artist_ids.truncate(12);
    Ok(Some(Described {
        summary: intent.summary,
        unknown: intent.unknown,
        tracks,
        total,
        artists: state.store.artists_by_ids(&artist_ids).await?,
    }))
}

#[derive(Deserialize)]
struct Limit {
    limit: Option<usize>,
}

impl Limit {
    fn get(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }
}

#[derive(Serialize)]
struct SimilarTracks {
    tracks: Vec<Track>,
    /// False until the song has been analyzed.
    analyzed: bool,
}

async fn similar_tracks(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(limit): Query<Limit>,
) -> ApiResult<Json<SimilarTracks>> {
    let ids = state
        .sound
        .read()
        .map(|index| index.similar_tracks(id, limit.get()))
        .unwrap_or_default();
    Ok(Json(SimilarTracks {
        analyzed: !ids.is_empty(),
        tracks: state.store.tracks_by_ids(&ids).await?,
    }))
}

/// Artists like this one: genres in common (from MusicBrainz) and sound,
/// together. Genres say what scene an artist is in; sound covers artists
/// without genres.
async fn similar_artists(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<ArtistSummary>>> {
    let ids = {
        let (Ok(catalog), Ok(sound)) = (state.catalog.read(), state.sound.read()) else {
            return Ok(Json(Vec::new()));
        };
        rank_artists(id, &catalog, &sound)
    };
    Ok(Json(state.store.artists_by_ids(&ids).await?))
}

fn rank_artists(id: i64, catalog: &Catalog, sound: &SoundIndex) -> Vec<i64> {
    let empty = HashSet::new();
    let mine = catalog.artist_genres.get(&id).unwrap_or(&empty);
    // Sound closeness as 1 (closest) down to 0 (farthest).
    let mut distances = sound.artist_distances(id);
    distances.sort_by(|a, b| a.1.total_cmp(&b.1));
    let n = distances.len().max(1) as f32;
    let closeness: HashMap<i64, (f32, usize)> = distances
        .iter()
        .enumerate()
        .map(|(rank, (other, _, songs))| (*other, (1.0 - rank as f32 / n, *songs)))
        .collect();
    let mut scored: Vec<(f32, i64)> = catalog
        .artist_genres
        .iter()
        .filter(|(other, _)| **other != id)
        .filter_map(|(other, genres)| {
            let shared = mine.intersection(genres).count() as f32;
            let union = mine.union(genres).count() as f32;
            let genre = if union > 0.0 { shared / union } else { 0.0 };
            let (sound_score, songs) = closeness.get(other).copied().unwrap_or((0.0, 0));
            let score = if !mine.is_empty() && !genres.is_empty() {
                0.65 * genre + 0.35 * sound_score
            } else if songs >= MIN_SOUND_SONGS {
                // No genres to compare: sound alone, trusted a bit less.
                0.6 * sound_score
            } else {
                return None;
            };
            (score > 0.15).then_some((score, *other))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .take(SIMILAR_ARTISTS)
        .map(|(_, other)| other)
        .collect()
}
