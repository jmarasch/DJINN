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
    /// What the AI model heard in each song (when it's on).
    heard: HashMap<i64, Vec<f32>>,
}

/// Genres the AI hears in a song with none of its own: the best two, if
/// they're clear enough.
const HEARD_GENRES: usize = 2;
const HEARD_GENRE_MIN: f32 = 0.35;
/// A description matched by sound alone keeps songs this close to the
/// best match, and at least this close to the description.
const SOUND_MARGIN: f32 = 0.08;
const SOUND_MIN: f32 = 0.2;

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
        let heard = state.store.heard_rows(djinn_ai::models::VERSION).await?;
        Ok::<_, djinn_store::StoreError>((rows, facts, artists, heard))
    }
    .await;
    let genre_vectors = state.ai.genre_embeddings();
    let (rows, facts, artists, heard) = match loaded {
        Ok(loaded) => loaded,
        Err(e) => {
            tracing::error!(error = %e, "loading song facts failed");
            return;
        }
    };
    let built = tokio::task::spawn_blocking(move || {
        // What the analyzer heard, by song: tempo, punch, mode.
        let sounded: HashMap<i64, (f32, f32, f32)> = rows
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
        let heard: HashMap<i64, Vec<f32>> = heard.into_iter().collect();
        let songs: Vec<SongFacts> = facts
            .into_iter()
            .map(|f| {
                let mut genres = split(&f.genres);
                // No genres from tags or MusicBrainz: use what the AI hears.
                if genres.is_empty() {
                    if let Some(embedding) = heard.get(&f.id) {
                        let mut scored: Vec<(f32, &String)> = genre_vectors
                            .iter()
                            .map(|(name, v)| (djinn_ai::similarity(v, embedding), name))
                            .collect();
                        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                        genres.extend(
                            scored
                                .into_iter()
                                .take(HEARD_GENRES)
                                .filter(|(score, _)| *score >= HEARD_GENRE_MIN)
                                .map(|(_, name)| vibe::normalize(name)),
                        );
                    }
                }
                let sounded = sounded.get(&f.id);
                genres.sort();
                genres.dedup();
                SongFacts {
                    id: f.id,
                    year: f.year,
                    genres,
                    popularity: f.popularity,
                    bpm: sounded.map(|h| h.0),
                    punch: sounded.map(|h| h.1),
                    mode: sounded.map(|h| h.2),
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
                heard,
            },
        )
    })
    .await;
    let Ok((index, catalog)) = built else { return };
    tracing::info!(
        analyzed = index.len(),
        heard = catalog.heard.len(),
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

/// If `query` describes music, the songs and artists that fit. With AI
/// listening on, matches are ordered by how well they sound like the
/// description, genres nobody tagged are found by sound, and when
/// `plain_found` is false a query the phrase reader can't place is
/// matched by sound alone ("music for cooking dinner").
pub async fn describe(
    state: &AppState,
    query: &str,
    plain_found: bool,
) -> ApiResult<Option<Described>> {
    let intent = {
        let Ok(catalog) = state.catalog.read() else {
            return Ok(None);
        };
        vibe::parse(query, &catalog.vocab)
    };
    if intent.is_none() && plain_found {
        return Ok(None);
    }
    // What the description sounds like, if the model is loaded.
    let sound = match state.ai.model() {
        Some(model) => {
            let text = query.to_owned();
            tokio::task::spawn_blocking(move || model.embed_text(&text).ok())
                .await
                .ok()
                .flatten()
        }
        None => None,
    };
    let found = {
        let (Ok(catalog), Ok(index)) = (state.catalog.read(), state.sound.read()) else {
            return Ok(None);
        };
        match (intent, sound) {
            (Some(intent), sound) => {
                let (mut ids, mut total) = vibe::rank(&intent, &catalog.songs, &index, usize::MAX);
                let mut summary = intent.summary.clone();
                if let Some(sound) = &sound {
                    if ids.is_empty() && !intent.genres.is_empty() {
                        // Nobody tagged that genre: keep the other limits
                        // (decade, tempo) and find it by sound.
                        let mut loose = intent.clone();
                        loose.genres.clear();
                        let candidates = if loose.is_empty() {
                            catalog.songs.iter().map(|s| s.id).collect()
                        } else {
                            vibe::rank(&loose, &catalog.songs, &index, usize::MAX).0
                        };
                        ids = by_sound(&candidates, sound, &catalog.heard);
                        total = ids.len();
                        if !ids.is_empty() {
                            summary.push("by sound".into());
                        }
                    } else if soft_only(&intent) {
                        // Only mood words ("dark", "sad"): they don't narrow
                        // anything, so keep what sounds like the description.
                        ids = by_sound(&ids, sound, &catalog.heard);
                        total = ids.len();
                    } else if intent.like_artists.is_empty() {
                        // Best-sounding first; songs not heard yet after.
                        ids.sort_by(|a, b| {
                            let score = |id: &i64| {
                                catalog
                                    .heard
                                    .get(id)
                                    .map_or(f32::MIN, |v| djinn_ai::similarity(v, sound))
                            };
                            score(b).total_cmp(&score(a))
                        });
                    }
                }
                ids.truncate(DESCRIBED_SONGS);
                Some((summary, intent.unknown, ids, total))
            }
            (None, Some(sound)) => {
                let all: Vec<i64> = catalog.heard.keys().copied().collect();
                let mut ids = by_sound(&all, &sound, &catalog.heard);
                let total = ids.len();
                ids.truncate(DESCRIBED_SONGS);
                (!ids.is_empty()).then(|| {
                    (
                        vec![format!("sounds like \"{}\"", query.trim())],
                        Vec::new(),
                        ids,
                        total,
                    )
                })
            }
            (None, None) => None,
        }
    };
    let Some((summary, unknown, ids, total)) = found else {
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
        summary,
        unknown,
        tracks,
        total,
        artists: state.store.artists_by_ids(&artist_ids).await?,
    }))
}

/// True if nothing in the description narrows the songs down (only mood
/// words, or words the phrase reader didn't know).
fn soft_only(intent: &vibe::Intent) -> bool {
    intent.genres.is_empty()
        && intent.years.is_none()
        && intent.tempo.is_none()
        && intent.energy.is_none()
        && intent.like_artists.is_empty()
}

/// Of `candidates`, the songs that sound most like `sound`, best first:
/// those close to the best match and not too far from the description.
fn by_sound(candidates: &[i64], sound: &[f32], heard: &HashMap<i64, Vec<f32>>) -> Vec<i64> {
    let mut scored: Vec<(f32, i64)> = candidates
        .iter()
        .filter_map(|id| heard.get(id).map(|v| (djinn_ai::similarity(v, sound), *id)))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let best = scored.first().map_or(0.0, |s| s.0);
    scored
        .into_iter()
        .take_while(|(score, _)| *score >= SOUND_MIN && *score >= best - SOUND_MARGIN)
        .map(|(_, id)| id)
        .collect()
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
