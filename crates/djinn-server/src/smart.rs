//! Smart playlists: songs and artists that sound alike, and search by
//! description ("80's hair metal"). The sound index and the song facts
//! live in memory and are rebuilt when analysis or artist lookups finish.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use djinn_ai::Reading;
use djinn_library::genres::GenreMap;
use djinn_library::sound::SoundIndex;
use djinn_library::vibe::{self, SongFacts, Vocabulary};
use djinn_store::{ArtistSummary, Track};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::jukebox::SongLinks;
use crate::settings;
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
    /// Each artist's genres, normalized: MusicBrainz's, else the ones most
    /// of their songs have.
    artist_genres: HashMap<i64, HashSet<String>>,
    /// Genres from their songs, for artists MusicBrainz has none for; best
    /// first, for showing.
    song_genres: HashMap<i64, Vec<String>>,
    /// Each song's own styles, from AI listening.
    styles: HashMap<i64, Vec<String>>,
    /// Each song's genres and styles together.
    tags: HashMap<i64, Vec<String>>,
    /// Each artist's songs (the ones they lead, else the ones they're on).
    artist_songs: HashMap<i64, Vec<i64>>,
    /// What the AI model heard in each song (when it's on).
    heard: HashMap<i64, Vec<f32>>,
    /// Each song's artists and genres, for learning from parties.
    links: std::sync::Arc<SongLinks>,
}

/// Genres the AI hears in a song with none of its own: the best two, if
/// they're clear enough.
const HEARD_GENRES: usize = 2;
const HEARD_GENRE_MIN: f32 = 0.35;
/// A song's styles: up to this many genres the AI hears, each this close
/// to the clearest one.
const STYLES: usize = 3;
const STYLE_MARGIN: f32 = 0.05;
/// A description matched by sound alone keeps songs this close to the
/// best match, and at least this close to the description.
const SOUND_MARGIN: f32 = 0.08;
const SOUND_MIN: f32 = 0.2;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks/{id}/similar", get(similar_tracks))
        .route("/tracks/{id}/tags", get(track_tags))
        .route(
            "/tracks/{id}/styles/{genre}",
            put(restore_style).delete(remove_style),
        )
        .route("/artists/{id}/similar", get(similar_artists))
        .route("/genres", get(genres))
        .route("/genres/{name}", get(genre))
}

impl Catalog {
    /// Songs whose genres or styles include `genre` (normalized).
    fn songs_in(&self, genre: &str) -> Vec<i64> {
        self.tags
            .iter()
            .filter(|(_, tags)| tags.iter().any(|t| vibe::genre_fits(t, genre)))
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Radio's song picker, working from the same sound index and catalog as
/// search.
pub struct SmartMixer {
    sound: std::sync::Arc<std::sync::RwLock<SoundIndex>>,
    catalog: std::sync::Arc<std::sync::RwLock<Catalog>>,
}

impl SmartMixer {
    pub fn new(state: &AppState) -> Self {
        Self {
            sound: std::sync::Arc::clone(&state.sound),
            catalog: std::sync::Arc::clone(&state.catalog),
        }
    }
}

impl crate::jukebox::Mixer for SmartMixer {
    fn links(&self) -> std::sync::Arc<SongLinks> {
        self.catalog
            .read()
            .map(|catalog| std::sync::Arc::clone(&catalog.links))
            .unwrap_or_default()
    }

    fn radio(
        &self,
        seed: &crate::jukebox::RadioSeed,
        played: &[i64],
        skipped: &[i64],
    ) -> Vec<(i64, f32)> {
        use crate::jukebox::RadioSeed;
        use djinn_library::radio::{rank, Seeds, Session};
        let (Ok(catalog), Ok(index)) = (self.catalog.read(), self.sound.read()) else {
            return Vec::new();
        };
        let seeds = match seed {
            RadioSeed::Track { id } => Seeds {
                tracks: vec![*id],
                only: None,
                genres: catalog
                    .tags
                    .get(id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            },
            RadioSeed::Artist { id } => Seeds {
                tracks: catalog.artist_songs.get(id).cloned().unwrap_or_default(),
                only: None,
                genres: catalog.artist_genres.get(id).cloned().unwrap_or_default(),
            },
            RadioSeed::Genre { name } => {
                let name = vibe::normalize(name);
                let songs = catalog.songs_in(&name);
                Seeds {
                    only: Some(songs.iter().copied().collect()),
                    tracks: songs,
                    genres: [name].into_iter().collect(),
                }
            }
        };
        let session = Session {
            played: played.to_vec(),
            skipped: skipped.to_vec(),
        };
        rank(&index, &seeds, &catalog.tags, &session, RADIO_SONGS)
    }
}

/// Songs radio considers at a time.
const RADIO_SONGS: usize = 150;
/// Songs and artists shown on a genre's page.
const GENRE_SONGS: usize = 100;
const GENRE_ARTISTS: usize = 24;

/// Genres shown next to a song in lists.
const LIST_GENRES: usize = 2;

/// The first genres (or, failing those, styles) of each song, for lists.
pub fn list_genres(
    state: &AppState,
    ids: impl IntoIterator<Item = i64>,
) -> HashMap<i64, Vec<String>> {
    let Ok(catalog) = state.catalog.read() else {
        return HashMap::new();
    };
    ids.into_iter()
        .filter_map(|id| {
            let tags = catalog.tags.get(&id)?;
            (!tags.is_empty()).then(|| (id, tags.iter().take(LIST_GENRES).cloned().collect()))
        })
        .collect()
}

#[derive(Serialize)]
struct TrackTags {
    /// From tags, the album, the artists, and identification.
    genres: Vec<String>,
    /// What AI listening heard in this song.
    styles: Vec<String>,
}

async fn track_tags(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Json<TrackTags> {
    let Ok(catalog) = state.catalog.read() else {
        return Json(TrackTags {
            genres: Vec::new(),
            styles: Vec::new(),
        });
    };
    let genres = catalog
        .songs
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.genres.clone())
        .unwrap_or_default();
    let styles = catalog
        .styles
        .get(&id)
        .into_iter()
        .flatten()
        .filter(|s| !genres.contains(s))
        .cloned()
        .collect();
    Json(TrackTags { genres, styles })
}

/// Takes a "Sounds like" genre off a song. It stays off when the AI
/// listens again.
async fn remove_style(
    _: AdminUser,
    State(state): State<AppState>,
    Path((id, genre)): Path<(i64, String)>,
) -> ApiResult<StatusCode> {
    set_style_removed(&state, id, &genre, true).await
}

/// Puts a removed "Sounds like" genre back.
async fn restore_style(
    _: AdminUser,
    State(state): State<AppState>,
    Path((id, genre)): Path<(i64, String)>,
) -> ApiResult<StatusCode> {
    set_style_removed(&state, id, &genre, false).await
}

async fn set_style_removed(
    state: &AppState,
    id: i64,
    genre: &str,
    removed: bool,
) -> ApiResult<StatusCode> {
    let genre = vibe::normalize(genre);
    if genre.is_empty() {
        return Err(ApiError::bad_request("No genre given"));
    }
    if !state.store.set_style_removed(id, &genre, removed).await? {
        return Err(ApiError::not_found());
    }
    refresh(state).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct GenrePage {
    name: String,
    /// Songs with it (more than shown).
    total: usize,
    tracks: Vec<Track>,
    artists: Vec<ArtistSummary>,
    /// Genres linked to it on the genre cloud, strongest first.
    related: Vec<djinn_library::genres::Related>,
}

/// Every genre in the library and how they relate, for browsing by genre
/// and the genre cloud.
async fn genres(_: CurrentUser, State(state): State<AppState>) -> Json<GenreMap> {
    let map = match (state.catalog.read(), state.sound.read()) {
        (Ok(catalog), Ok(index)) => djinn_library::genres::map(&catalog.tags, &index),
        _ => GenreMap::default(),
    };
    Json(map)
}

/// A genre's songs (by tags or by sound) and artists, most popular first.
async fn genre(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<GenrePage>> {
    let name = vibe::normalize(&name);
    let (track_ids, total, artist_ids) = {
        let Ok(catalog) = state.catalog.read() else {
            return Ok(Json(GenrePage {
                name,
                total: 0,
                tracks: Vec::new(),
                artists: Vec::new(),
                related: Vec::new(),
            }));
        };
        let songs = catalog.songs_in(&name);
        let in_genre: HashSet<i64> = songs.iter().copied().collect();
        let popularity: HashMap<i64, i64> =
            catalog.songs.iter().map(|s| (s.id, s.popularity)).collect();
        let mut ranked = songs;
        ranked.sort_by_key(|id| {
            (
                std::cmp::Reverse(popularity.get(id).copied().unwrap_or(0)),
                *id,
            )
        });
        let total = ranked.len();
        ranked.truncate(GENRE_SONGS);
        // Artists tagged with it first, then by how many of their songs
        // have it.
        let mut artists: Vec<(bool, usize, i64)> = catalog
            .artist_songs
            .iter()
            .map(|(artist, ids)| {
                let tagged = catalog
                    .artist_genres
                    .get(artist)
                    .is_some_and(|g| g.iter().any(|t| vibe::genre_fits(t, &name)));
                let count = ids.iter().filter(|id| in_genre.contains(id)).count();
                (tagged, count, *artist)
            })
            .filter(|(tagged, count, _)| *tagged || *count > 0)
            .collect();
        artists.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        let artist_ids: Vec<i64> = artists
            .into_iter()
            .take(GENRE_ARTISTS)
            .map(|(_, _, id)| id)
            .collect();
        (ranked, total, artist_ids)
    };
    let related = match (state.catalog.read(), state.sound.read()) {
        (Ok(catalog), Ok(index)) => {
            djinn_library::genres::map(&catalog.tags, &index).related(&name)
        }
        _ => Vec::new(),
    };
    Ok(Json(GenrePage {
        name,
        total,
        tracks: state.store.tracks_by_ids(&track_ids).await?,
        artists: state.store.artists_by_ids(&artist_ids).await?,
        related,
    }))
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
        let led = state.store.artist_tracks().await?;
        let removed = state.store.removed_styles().await?;
        Ok::<_, djinn_store::StoreError>((rows, facts, artists, heard, led, removed))
    }
    .await;
    let genre_vectors = state.ai.genre_embeddings();
    let tag_untagged = settings::flag(&state.store, crate::ai::GENRE_TAGS, true)
        .await
        .unwrap_or(false);
    let (rows, facts, artists, heard, led, removed) = match loaded {
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
        // Genres taken off songs in the app. They come off after the AI's
        // picks are made, so removing one never brings in another.
        let mut off: HashMap<i64, HashSet<String>> = HashMap::new();
        for (id, genre) in removed {
            off.entry(id).or_default().insert(genre);
        }
        let kept = |id: i64, genre: &String| !off.get(&id).is_some_and(|g| g.contains(genre));
        // What the AI hears in each song, best first.
        let heard_genres: HashMap<i64, Vec<(f32, String)>> = heard
            .iter()
            .filter(|_| !genre_vectors.is_empty())
            .map(|(id, embedding)| {
                let mut scored: Vec<(f32, String)> = genre_vectors
                    .iter()
                    .map(|(name, v)| (djinn_ai::similarity(v, embedding), vibe::normalize(name)))
                    .collect();
                scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                (*id, scored)
            })
            .collect();
        // Each song's own styles: the genres the AI hears clearly in it.
        let styles: HashMap<i64, Vec<String>> = heard_genres
            .iter()
            .map(|(id, scored)| {
                let best = scored.first().map_or(0.0, |s| s.0);
                let found: Vec<String> = scored
                    .iter()
                    .take(STYLES)
                    .filter(|(score, _)| *score >= HEARD_GENRE_MIN && *score >= best - STYLE_MARGIN)
                    .filter(|(_, name)| kept(*id, name))
                    .map(|(_, name)| name.clone())
                    .collect();
                (*id, found)
            })
            .filter(|(_, found)| !found.is_empty())
            .collect();
        let songs: Vec<SongFacts> = facts
            .into_iter()
            .map(|f| {
                let mut genres = split(&f.genres);
                // No genres from tags or MusicBrainz: use what the AI hears.
                if genres.is_empty() && tag_untagged {
                    if let Some(scored) = heard_genres.get(&f.id) {
                        genres.extend(
                            scored
                                .iter()
                                .take(HEARD_GENRES)
                                .filter(|(score, _)| *score >= HEARD_GENRE_MIN)
                                .filter(|(_, name)| kept(f.id, name))
                                .map(|(_, name)| name.clone()),
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
        let mut artist_genres: HashMap<i64, HashSet<String>> = artists
            .iter()
            .map(|a| (a.id, split(&a.genres).into_iter().collect()))
            .collect();
        // No genres from MusicBrainz: the ones most of their songs have
        // (tags, identified songs, what the AI heard).
        let by_id: HashMap<i64, &SongFacts> = songs.iter().map(|s| (s.id, s)).collect();
        // Songs they lead; artists only ever featured go by those songs.
        let mut led_songs: HashMap<i64, Vec<&Vec<String>>> = HashMap::new();
        let mut featured_songs: HashMap<i64, Vec<&Vec<String>>> = HashMap::new();
        let mut artist_songs: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut featured_ids: HashMap<i64, Vec<i64>> = HashMap::new();
        for (artist, track, featured) in &led {
            if let Some(song) = by_id.get(track) {
                let (genres, ids) = if *featured {
                    (&mut featured_songs, &mut featured_ids)
                } else {
                    (&mut led_songs, &mut artist_songs)
                };
                genres.entry(*artist).or_default().push(&song.genres);
                ids.entry(*artist).or_default().push(*track);
            }
        }
        for (artist, ids) in featured_ids {
            artist_songs.entry(artist).or_insert(ids);
        }
        let mut song_genres = HashMap::new();
        for (artist, genres) in artist_genres.iter_mut() {
            if !genres.is_empty() {
                continue;
            }
            let theirs = led_songs.get(artist).or_else(|| featured_songs.get(artist));
            let found = genres_from_songs(theirs.map_or(&[][..], Vec::as_slice));
            genres.extend(found.iter().cloned());
            if !found.is_empty() {
                song_genres.insert(*artist, found);
            }
        }
        let tags: HashMap<i64, Vec<String>> = songs
            .iter()
            .map(|song| {
                let mut all = song.genres.clone();
                for style in styles.get(&song.id).into_iter().flatten() {
                    if !all.contains(style) {
                        all.push(style.clone());
                    }
                }
                (song.id, all)
            })
            .collect();
        let mut song_artists: HashMap<i64, Vec<i64>> = HashMap::new();
        for (artist, track, _) in &led {
            song_artists.entry(*track).or_default().push(*artist);
        }
        let links = std::sync::Arc::new(SongLinks {
            artists: song_artists,
            genres: tags.clone(),
        });
        (
            SoundIndex::build(rows),
            Catalog {
                songs,
                vocab,
                artist_genres,
                song_genres,
                tags,
                styles,
                artist_songs,
                heard,
                links,
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
    /// The AI reader read the query.
    pub read_by_ai: bool,
}

/// True if the AI reader is on and `query` has words the phrase reader
/// didn't understand (or reads as nothing and found nothing by name).
pub fn worth_asking(state: &AppState, query: &str, plain_found: bool) -> bool {
    if !state.ai.reader.ready() || query.split_whitespace().count() < 2 {
        return false;
    }
    let Ok(catalog) = state.catalog.read() else {
        return false;
    };
    match vibe::parse(query, &catalog.vocab) {
        Some(intent) => !intent.unknown.is_empty(),
        None => !plain_found,
    }
}

/// If `query` describes music, the songs and artists that fit. With AI
/// listening on, matches are ordered by how well they sound like the
/// description, genres nobody tagged are found by sound, and when
/// `plain_found` is false a query the phrase reader can't place is
/// matched by sound alone ("music for cooking dinner"). `read` is what
/// the AI reader made of it, if it read it.
pub async fn describe(
    state: &AppState,
    query: &str,
    plain_found: bool,
    read: Option<&Reading>,
) -> ApiResult<Option<Described>> {
    let intent = {
        let Ok(catalog) = state.catalog.read() else {
            return Ok(None);
        };
        let phrase = vibe::parse(query, &catalog.vocab);
        match read {
            Some(read) => vibe::merge(query, phrase, &asked(read), &catalog.vocab),
            None => phrase,
        }
    };
    if intent.is_none() && plain_found && read.is_none() {
        return Ok(None);
    }
    // What the description sounds like, if the model is loaded.
    let sound = match state.ai.model() {
        Some(model) => {
            // Lowercase, so "Chill" and "chill" find the same songs.
            let text = match read.and_then(|r| r.sound.as_deref()) {
                Some(sound) => format!("{}. {sound}", query.trim()),
                None => query.trim().to_owned(),
            }
            .to_lowercase();
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
        read_by_ai: read.is_some(),
    }))
}

fn asked(read: &Reading) -> vibe::Asked {
    vibe::Asked {
        genres: read.genres.clone(),
        not_genres: read.not_genres.clone(),
        years: read.years,
        tempo: read.tempo.clone(),
        mood: read.mood.clone(),
        like_artists: read.like_artists.clone(),
    }
}

/// Genres shown for an artist with none from MusicBrainz, from their
/// songs; `None` if they have their own or none can be told.
pub fn song_genres(state: &AppState, artist: i64) -> Option<String> {
    let catalog = state.catalog.read().ok()?;
    catalog.song_genres.get(&artist).map(|g| g.join(", "))
}

/// The genres most of an artist's songs share, best first: up to five,
/// each on at least a quarter of the songs (and two, with four or more).
fn genres_from_songs(songs: &[&Vec<String>]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for genres in songs {
        for genre in genres.iter() {
            *counts.entry(genre.as_str()).or_default() += 1;
        }
    }
    let needed = songs
        .len()
        .div_ceil(4)
        .max(if songs.len() >= 4 { 2 } else { 1 });
    let mut found: Vec<(&str, usize)> = counts.into_iter().filter(|(_, n)| *n >= needed).collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    found
        .into_iter()
        .take(5)
        .map(|(g, _)| g.to_owned())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_genres_from_their_songs() {
        let g = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let songs = [
            g(&["house", "edm"]),
            g(&["house"]),
            g(&["house", "dance pop"]),
            g(&["edm"]),
            g(&["trance"]),
        ];
        let refs: Vec<&Vec<String>> = songs.iter().collect();
        assert_eq!(
            genres_from_songs(&refs),
            ["house", "edm"],
            "trance and dance pop are one-offs"
        );
        let one = [g(&["synth pop"])];
        assert_eq!(
            genres_from_songs(&one.iter().collect::<Vec<_>>()),
            ["synth pop"]
        );
        assert!(genres_from_songs(&[]).is_empty());
    }

    #[tokio::test]
    async fn admins_take_styles_off_songs() {
        use crate::test_util::{call, session};
        use axum::http::Method;
        use djinn_store::Role;

        let state = AppState::for_tests().await;
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: "/music/interlude.flac".into(),
                title: "Interlude".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.library_track_ids().await.unwrap()[0];
        let admin = session(&state, "host", Role::Admin).await;
        let guest = session(&state, "lee", Role::User).await;
        let path = format!("/api/tracks/{id}/styles/Trap");

        let (status, _) = call(&state, Method::DELETE, &path, None, Some(&guest)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::DELETE, &path, None, Some(&admin)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            state.store.removed_styles().await.unwrap(),
            [(id, "trap".to_owned())],
            "kept normalized"
        );
        let (status, _) = call(&state, Method::PUT, &path, None, Some(&admin)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(state.store.removed_styles().await.unwrap().is_empty());

        let missing = "/api/tracks/999/styles/trap";
        let (status, _) = call(&state, Method::DELETE, missing, None, Some(&admin)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
