//! Browsing the library: search, tracks, albums, artists, art, rescans,
//! and tag corrections.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_library::{art, ScanProgress};
use djinn_store::{
    Album, AlbumDetail, AlbumSort, ArtistDetail, ArtistSummary, Jump, Page, SearchResults,
    TagChange, TagEdit, Track, TrackSort,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::events::{LookupStatus, ServerEvent};
use crate::settings;
use crate::state::AppState;

const DEFAULT_SEARCH_LIMIT: i64 = 20;
const MAX_SEARCH_LIMIT: i64 = 100;
const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search))
        .route("/search/ask", get(ask))
        .route("/tracks", get(tracks))
        .route("/tracks/jumps", get(track_jumps))
        .route("/tracks/{id}", get(track))
        .route("/tracks/{id}/edit", get(track_edit))
        .route("/tracks/edit", put(edit_tracks))
        .route("/albums", get(albums))
        .route("/albums/jumps", get(album_jumps))
        .route("/albums/{id}", get(album))
        .route("/artists", get(artists))
        .route("/artists/jumps", get(artist_jumps))
        .route("/artists/{id}", get(artist))
        .route("/artists/{id}/edit", put(edit_artist))
        .route("/art/{hash}", get(cover_art))
        .route("/library/rescan", post(rescan))
        .route("/library/artist-info", post(artist_info))
        .route("/library/open-art-folder", post(open_art_folder))
        .route("/library/status", get(status))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct SearchResponse {
    #[serde(flatten)]
    results: SearchResults,
    /// Songs fitting the query read as a description ("80's hair metal").
    described: Option<crate::smart::Described>,
    /// The AI reader might understand it better: ask `/search/ask`.
    ask: bool,
}

async fn search(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<SearchResponse>> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT);
    let results = state.store.search(&query.q, limit).await?;
    let plain_found =
        !results.tracks.is_empty() || !results.albums.is_empty() || !results.artists.is_empty();
    let allowed = crate::reader::allowed_for(&state, &user).await;
    let read = allowed.then(|| state.ai.reader.cached(&query.q)).flatten();
    let ask =
        allowed && read.is_none() && crate::smart::worth_asking(&state, &query.q, plain_found);
    Ok(Json(SearchResponse {
        described: crate::smart::describe(&state, &query.q, plain_found, read.as_ref()).await?,
        results,
        ask,
    }))
}

/// The query read by the AI reader (a few seconds), as a description.
async fn ask(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Option<crate::smart::Described>>> {
    if !crate::reader::allowed_for(&state, &user).await {
        return Ok(Json(None));
    }
    let Some(read) = crate::reader::read(&state, &query.q).await else {
        return Ok(Json(None));
    };
    Ok(Json(
        crate::smart::describe(&state, &query.q, false, Some(&read)).await?,
    ))
}

#[derive(Deserialize)]
struct PageQuery {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
    #[serde(default)]
    sort: TrackSort,
}

impl PageQuery {
    fn bounds(&self) -> (i64, i64) {
        (
            self.offset.max(0),
            self.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE),
        )
    }
}

async fn artists(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Page<ArtistSummary>>> {
    let (offset, limit) = page.bounds();
    Ok(Json(state.store.artists_page(offset, limit).await?))
}

#[derive(Deserialize)]
struct AlbumQuery {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
    #[serde(default)]
    sort: AlbumSort,
}

async fn albums(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<AlbumQuery>,
) -> ApiResult<Json<Page<Album>>> {
    let page = PageQuery {
        offset: query.offset,
        limit: query.limit,
        sort: TrackSort::default(),
    };
    let (offset, limit) = page.bounds();
    Ok(Json(
        state.store.albums_page(query.sort, offset, limit).await?,
    ))
}

/// Where each letter starts in the artist list, for the jump strip.
async fn artist_jumps(_: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Jump>>> {
    Ok(Json(state.store.artist_jumps().await?))
}

/// Where each letter or year starts in the album list.
async fn album_jumps(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<AlbumQuery>,
) -> ApiResult<Json<Vec<Jump>>> {
    Ok(Json(state.store.album_jumps(query.sort).await?))
}

/// Where each letter or year starts in the song list.
async fn track_jumps(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Vec<Jump>>> {
    Ok(Json(state.store.track_jumps(page.sort).await?))
}

/// A song in the library's song list, with a genre or two to show.
#[derive(Serialize)]
struct ListedTrack {
    #[serde(flatten)]
    track: Track,
    genres: Vec<String>,
}

async fn tracks(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Page<ListedTrack>>> {
    let (offset, limit) = page.bounds();
    let found = state.store.tracks_page(page.sort, offset, limit).await?;
    let mut genres = crate::smart::list_genres(&state, found.items.iter().map(|t| t.id));
    Ok(Json(Page {
        total: found.total,
        items: found
            .items
            .into_iter()
            .map(|track| ListedTrack {
                genres: genres.remove(&track.id).unwrap_or_default(),
                track,
            })
            .collect(),
    }))
}

async fn track(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Track>> {
    state
        .store
        .track(id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

#[derive(Serialize)]
struct TrackEdit {
    track: Track,
    /// The fields corrected in the app; the rest come from the file.
    edited: TagEdit,
}

async fn track_edit(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<TrackEdit>> {
    let track = state
        .store
        .track(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let edited = state.store.tag_edit(id).await?;
    Ok(Json(TrackEdit { track, edited }))
}

/// Corrections for some songs. A field left out stays as it is, null goes
/// back to the file's value, and a value replaces it.
#[derive(Deserialize)]
struct EditTracks {
    track_ids: Vec<i64>,
    #[serde(default, deserialize_with = "present")]
    title: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    artist: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    album: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    album_artist: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    year: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present")]
    genre: Option<Option<String>>,
}

/// Tells a field sent as null (`Some(None)`) from one left out (`None`).
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// A text correction, trimmed. Empty goes back to the file's value.
fn text_field(field: Option<Option<String>>, name: &str) -> ApiResult<Option<Option<String>>> {
    let Some(value) = field else {
        return Ok(None);
    };
    let value = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    if let Some(v) = &value {
        if v.chars().count() > 300 || v.chars().any(char::is_control) {
            return Err(ApiError::bad_request(format!(
                "the {name} should be plain text, at most 300 characters"
            )));
        }
    }
    Ok(Some(value))
}

async fn edit_tracks(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<EditTracks>,
) -> ApiResult<StatusCode> {
    if body.track_ids.is_empty() || body.track_ids.len() > 5_000 {
        return Err(ApiError::bad_request("pick 1 to 5000 songs"));
    }
    if let Some(Some(year)) = body.year {
        if !(1..=9999).contains(&year) {
            return Err(ApiError::bad_request("the year should be 1 to 9999"));
        }
    }
    let change = TagChange {
        title: text_field(body.title, "title")?,
        artist: text_field(body.artist, "artist")?,
        album: text_field(body.album, "album")?,
        album_artist: text_field(body.album_artist, "album artist")?,
        year: body.year,
        genre: text_field(body.genre, "genre")?,
    };
    let paths = state.store.edit_tags(&body.track_ids, &change).await?;
    reread(&state, &paths).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Corrections for an artist: a new name on every song and album they're
/// on, and genres for the songs they lead. Left out stays, null goes back
/// to the files' values.
#[derive(Deserialize)]
struct EditArtist {
    #[serde(default, deserialize_with = "present")]
    name: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    genre: Option<Option<String>>,
}

#[derive(Serialize)]
struct EditedArtist {
    /// Where the artist is now; a new name can make them a new artist.
    id: i64,
}

async fn edit_artist(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<EditArtist>,
) -> ApiResult<Json<EditedArtist>> {
    let old = state
        .store
        .artist(id)
        .await?
        .ok_or_else(ApiError::not_found)?
        .artist
        .name;
    let name = text_field(body.name, "name")?;
    let genre = text_field(body.genre, "genre")?;
    let songs = state.store.artist_songs(id).await?;
    let mut paths = Vec::new();
    match &name {
        None => {}
        Some(None) => {
            let ids: Vec<i64> = songs.iter().map(|s| s.id).collect();
            let change = TagChange {
                artist: Some(None),
                album_artist: Some(None),
                ..Default::default()
            };
            paths.extend(state.store.edit_tags(&ids, &change).await?);
        }
        Some(Some(new)) => {
            // Songs whose credits end up the same are corrected together.
            let mut groups: BTreeMap<(Option<String>, Option<String>), Vec<i64>> = BTreeMap::new();
            for song in &songs {
                let artist = song.artist.as_deref().and_then(|a| renamed(a, &old, new));
                let album_artist = song
                    .album_artist
                    .as_deref()
                    .and_then(|a| renamed(a, &old, new));
                if artist.is_some() || album_artist.is_some() {
                    groups
                        .entry((artist, album_artist))
                        .or_default()
                        .push(song.id);
                }
            }
            for ((artist, album_artist), ids) in groups {
                let change = TagChange {
                    artist: artist.map(Some),
                    album_artist: album_artist.map(Some),
                    ..Default::default()
                };
                paths.extend(state.store.edit_tags(&ids, &change).await?);
            }
            state.store.rename_artist(id, new).await?;
        }
    }
    if let Some(genre) = genre {
        let led: Vec<i64> = songs.iter().filter(|s| s.leads).map(|s| s.id).collect();
        let change = TagChange {
            genre: Some(genre),
            ..Default::default()
        };
        paths.extend(state.store.edit_tags(&led, &change).await?);
    }
    paths.sort();
    paths.dedup();
    if !paths.is_empty() {
        reread(&state, &paths).await?;
    }
    let now = match name {
        Some(Some(new)) => state.store.artist_named(&new).await?,
        // Whoever the files name on a song they led.
        Some(None) => match songs.iter().find(|s| s.leads).or(songs.first()) {
            Some(song) => state.store.track(song.id).await?.and_then(|t| t.artist_id),
            None => None,
        },
        None => None,
    };
    Ok(Json(EditedArtist {
        id: now.unwrap_or(id),
    }))
}

/// `credit` with the name `old` replaced by `new` wherever it stands as a
/// whole name ("A feat. Old" becomes "A feat. New"); `None` if it isn't in
/// there.
fn renamed(credit: &str, old: &str, new: &str) -> Option<String> {
    let is_name = |c: char| c.is_alphanumeric();
    let mut out = String::with_capacity(credit.len());
    let mut rest = credit;
    let mut found = false;
    while !rest.is_empty() {
        let starts_word = !out.chars().next_back().is_some_and(is_name);
        let at = rest
            .get(..old.len())
            .filter(|s| s.eq_ignore_ascii_case(old));
        let ends_word = !rest[at.map_or(0, str::len)..].starts_with(is_name);
        if let (Some(at), true, true) = (at, starts_word, ends_word) {
            out.push_str(new);
            rest = &rest[at.len()..];
            found = true;
        } else if let Some(c) = rest.chars().next() {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    found.then_some(out)
}

/// Reads files again after something changed how they read (a correction,
/// an identification), and tells everyone.
pub async fn reread(state: &AppState, paths: &[String]) -> ApiResult<()> {
    let roots = crate::folders::library_roots(state).await?;
    djinn_library::reread(&state.store, &roots, &state.art_dir, paths)
        .await
        .map_err(anyhow::Error::from)?;
    let _ = state.events.send(ServerEvent::LibraryChanged);
    crate::smart::refresh(state).await;
    // A new name makes a new artist or album to look up.
    if settings::flag(&state.store, settings::ONLINE_INFO, false)
        .await
        .unwrap_or(false)
    {
        start_artist_lookup(state, false);
    }
    Ok(())
}

async fn album(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<AlbumDetail>> {
    state
        .store
        .album(id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn artist(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ArtistDetail>> {
    let mut artist = state
        .store
        .artist(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if artist.genres.as_deref().is_none_or(|g| g.trim().is_empty()) {
        artist.genres = crate::smart::song_genres(&state, id);
    }
    Ok(Json(artist))
}

#[derive(Deserialize)]
pub struct ArtQuery {
    /// Thumbnail size in pixels (128 or 512); omit for the original.
    size: Option<u32>,
}

/// Cover art by content hash, optionally as a thumbnail.
async fn cover_art(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
    Query(query): Query<ArtQuery>,
) -> ApiResult<impl IntoResponse> {
    serve_art(&state, hash, query).await
}

pub async fn serve_art(
    state: &AppState,
    hash: String,
    query: ArtQuery,
) -> ApiResult<impl IntoResponse + use<>> {
    let art_dir = state.art_dir.as_ref().clone();
    let file = tokio::task::spawn_blocking(move || match query.size {
        Some(size) => art::thumbnail(&art_dir, &hash, size),
        None => Ok(art::path(&art_dir, &hash)),
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(anyhow::Error::from)?
    .ok_or_else(ApiError::not_found)?;
    let bytes = tokio::fs::read(file).await?;
    Ok((
        [
            (header::CONTENT_TYPE, art::mime_type(&bytes)),
            // Content-addressed: a hash never changes meaning.
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    ))
}

#[derive(Serialize)]
struct LibraryStatus {
    scanning: bool,
    last_scan: Option<ScanProgress>,
    tracks: i64,
    /// Where cover art and artist photos are cached.
    art_dir: String,
}

async fn status(_: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<LibraryStatus>> {
    Ok(Json(LibraryStatus {
        scanning: state.scan.running.load(Ordering::Relaxed),
        last_scan: state.scan.last.lock().ok().and_then(|l| l.clone()),
        tracks: state.store.track_count().await?,
        art_dir: state.art_dir.display().to_string(),
    }))
}

/// Opens the image cache in the file manager of the computer DJINN runs
/// on (admin).
async fn open_art_folder(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    std::fs::create_dir_all(state.art_dir.as_ref()).map_err(anyhow::Error::from)?;
    open::that_detached(state.art_dir.as_ref()).map_err(|e| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("couldn't open the folder on this computer: {e}"),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a request to start background work did.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum StartOutcome {
    /// It started now.
    Started,
    /// One is already running; nothing new was started.
    Running,
    /// One is running; this one runs right after it.
    Queued,
}

#[derive(Serialize)]
struct StartResponse {
    status: StartOutcome,
}

fn started(status: StartOutcome) -> (StatusCode, Json<StartResponse>) {
    let code = match status {
        StartOutcome::Started | StartOutcome::Queued => StatusCode::ACCEPTED,
        StartOutcome::Running => StatusCode::OK,
    };
    (code, Json(StartResponse { status }))
}

async fn rescan(_: AdminUser, State(state): State<AppState>) -> (StatusCode, Json<StartResponse>) {
    started(if start_scan(&state) {
        StartOutcome::Started
    } else {
        StartOutcome::Running
    })
}

#[derive(Deserialize, Default)]
struct LookupRequest {
    /// Look up every artist again, not just new or stale ones.
    #[serde(default)]
    force: bool,
}

/// Looks up artist photos and bios online now (admin). If a lookup is
/// already running, a "recheck all" is queued to run right after it.
async fn artist_info(
    _: AdminUser,
    State(state): State<AppState>,
    body: Option<Json<LookupRequest>>,
) -> (StatusCode, Json<StartResponse>) {
    let force = body.map(|Json(b)| b.force).unwrap_or_default();
    if start_artist_lookup(&state, force) {
        return started(StartOutcome::Started);
    }
    if force {
        state.scan.lookup_force_next.store(true, Ordering::Release);
        return started(StartOutcome::Queued);
    }
    started(StartOutcome::Running)
}

/// Current lookup state, for clients and the SSE snapshot.
pub fn lookup_status(state: &AppState) -> LookupStatus {
    let last = state.scan.lookup_last.lock().ok().and_then(|l| l.clone());
    LookupStatus {
        running: state.scan.lookup_running.load(Ordering::Acquire),
        finished_at: last.as_ref().map(|(_, at)| *at),
        last: last.map(|(summary, _)| summary),
    }
}

/// Starts an online artist lookup in the background. Returns false if one
/// is already running.
/// At startup: finishes lookups that failed last time (a busy service, no
/// network), without rechecking anything that already worked.
pub async fn retry_unfinished_lookups(state: &AppState) {
    let on = settings::flag(&state.store, settings::ONLINE_INFO, false)
        .await
        .unwrap_or(false);
    if !on {
        return;
    }
    // Nothing is ever older than the oldest possible time, so these are
    // only the ones never finished.
    let artists = state.store.artists_for_info_lookup(i64::MIN).await;
    let albums = state.store.albums_for_info_lookup(i64::MIN).await;
    let due = artists.is_ok_and(|a| !a.is_empty()) || albums.is_ok_and(|a| !a.is_empty());
    if due {
        tracing::info!("finishing artist lookups that didn't complete last time");
        start_artist_lookup(state, false);
    }
}

pub fn start_artist_lookup(state: &AppState, force: bool) -> bool {
    if state.scan.lookup_running.swap(true, Ordering::AcqRel) {
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let mut force = force;
        loop {
            let _ = state
                .events
                .send(ServerEvent::ArtistLookup(lookup_status(&state)));
            tracing::info!(force, "looking up artist photos and bios online");
            let contact = state
                .store
                .get_setting(settings::MUSICBRAINZ_CONTACT)
                .await
                .ok()
                .flatten()
                .filter(|c| !c.trim().is_empty());
            let options = djinn_library::online::LookupOptions {
                force,
                musicbrainz_contact: contact,
            };
            let result =
                djinn_library::online::lookup_artists(&state.store, &state.art_dir, &options).await;
            match result {
                Ok(summary) => {
                    tracing::info!(
                        checked = summary.checked,
                        photos = summary.photos,
                        bios = summary.bios,
                        covers = summary.covers,
                        genres = summary.genres,
                        "artist lookup finished"
                    );
                    // A run with nothing due keeps the last useful summary.
                    let did_work = summary != Default::default();
                    if let Ok(mut last) = state.scan.lookup_last.lock() {
                        if did_work || last.is_none() {
                            *last = Some((summary, djinn_store::now_ms()));
                        }
                    }
                    let _ = state.events.send(ServerEvent::LibraryChanged);
                    // New genres change search by description and similar
                    // artists.
                    if did_work {
                        crate::smart::refresh(&state).await;
                    }
                }
                Err(e) => tracing::error!(error = %e, "artist lookup failed"),
            }
            // A "recheck all" asked for while this ran goes next.
            if !state.scan.lookup_force_next.swap(false, Ordering::AcqRel) {
                break;
            }
            force = true;
        }
        state.scan.lookup_running.store(false, Ordering::Release);
        let _ = state
            .events
            .send(ServerEvent::ArtistLookup(lookup_status(&state)));
    });
    true
}

/// Starts a library scan in the background. Returns false if one is
/// already running.
pub fn start_scan(state: &AppState) -> bool {
    if state.scan.running.swap(true, Ordering::AcqRel) {
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        tracing::info!("checking the library for changes");
        let started = std::time::Instant::now();
        let progress_state = state.clone();
        let roots = match crate::folders::library_roots(&state).await {
            Ok(roots) => roots,
            Err(e) => {
                tracing::error!(error = %format!("{e:#}"), "reading library folders failed");
                state.scan.running.store(false, Ordering::Release);
                return;
            }
        };
        let result = djinn_library::scan(&state.store, &roots, &state.art_dir, move |progress| {
            if let Ok(mut last) = progress_state.scan.last.lock() {
                *last = Some(progress.clone());
            }
            let _ = progress_state
                .events
                .send(ServerEvent::ScanProgress(progress.clone()));
        })
        .await;
        state.scan.running.store(false, Ordering::Release);
        match result {
            Ok(summary) => {
                tracing::info!(
                    found = summary.found,
                    read = summary.read,
                    removed = summary.removed,
                    secs = started.elapsed().as_secs_f32(),
                    "library check finished"
                );
                // Nothing new or gone: no artists to look up, no songs to
                // measure, and the playlist is the same.
                if summary.read == 0 && summary.removed == 0 {
                    return;
                }
                let _ = state.events.send(ServerEvent::LibraryChanged);
                if settings::flag(&state.store, settings::ONLINE_INFO, false)
                    .await
                    .unwrap_or(false)
                {
                    start_artist_lookup(&state, false);
                }
                crate::audio_api::start_analysis(&state).await;
                crate::ai::start(&state);
                // New tracks may have joined albums and artists in the
                // active playlist.
                if let Err(e) = state.jukebox.lock().await.auto_mix_changed().await {
                    tracing::error!(error = %format!("{e:#}"), "refreshing the queue failed");
                }
            }
            Err(e) => tracing::error!(error = %e, "library scan failed"),
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn starting_work_twice_is_not_an_error() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        // Pretend a lookup is already running.
        state.scan.lookup_running.store(true, Ordering::Release);

        let lookup = "/api/library/artist-info";
        let (status, _) = call(
            &state,
            Method::POST,
            lookup,
            Some(r#"{"force":false}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "already running");
        assert!(!state.scan.lookup_force_next.load(Ordering::Acquire));

        let (status, _) = call(
            &state,
            Method::POST,
            lookup,
            Some(r#"{"force":true}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "recheck all is queued");
        assert!(state.scan.lookup_force_next.load(Ordering::Acquire));

        state.scan.running.store(true, Ordering::Release);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/rescan",
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "already checking for changes");
    }

    #[test]
    fn renaming_finds_whole_names_only() {
        assert_eq!(renamed("Kream", "kream", "KREAM").as_deref(), Some("KREAM"));
        assert_eq!(
            renamed("Zara Larsson feat. Kream", "Kream", "KREAM").as_deref(),
            Some("Zara Larsson feat. KREAM")
        );
        assert_eq!(renamed("Kreamy", "Kream", "KREAM"), None);
        assert_eq!(renamed("Björk", "Björk", "Bjork").as_deref(), Some("Bjork"));
        assert_eq!(renamed("Someone", "Kream", "KREAM"), None);
    }

    #[tokio::test]
    async fn renaming_an_artist_corrects_their_songs() {
        use djinn_store::ScannedTrack;
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        state
            .store
            .save_scanned(&[
                ScannedTrack {
                    path: "/gone/a.flac".into(),
                    title: "Own".into(),
                    artist: Some("Kream".into()),
                    album: Some("Ours".into()),
                    credits: vec![("Kream".into(), false)],
                    ..Default::default()
                },
                ScannedTrack {
                    path: "/gone/b.flac".into(),
                    title: "Guest spot".into(),
                    artist: Some("Zara feat. Kream".into()),
                    credits: vec![("Zara".into(), false), ("Kream".into(), true)],
                    ..Default::default()
                },
            ])
            .await
            .unwrap();
        let kream = state.store.artist_named("Kream").await.unwrap().unwrap();
        let ids = state.store.library_track_ids().await.unwrap();
        let body = r#"{"name":"KREAM Two","genre":"House"}"#;
        let (status, _) = call(
            &state,
            Method::PUT,
            &format!("/api/artists/{kream}/edit"),
            Some(body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let mut edits = Vec::new();
        for id in ids {
            edits.push(state.store.tag_edit(id).await.unwrap());
        }
        edits.sort_by(|a, b| a.artist.cmp(&b.artist));
        assert_eq!(edits[0].artist.as_deref(), Some("KREAM Two"));
        assert_eq!(edits[0].genre.as_deref(), Some("House"), "songs they lead");
        assert_eq!(edits[1].artist.as_deref(), Some("Zara feat. KREAM Two"));
        assert_eq!(edits[1].genre, None, "only featured");
        assert_eq!(
            state.store.artist_named("KREAM Two").await.unwrap(),
            Some(kream),
            "the same artist, keeping their photo and bio"
        );
    }

    #[tokio::test]
    async fn admins_correct_tags() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let user = session(&state, "sam", Role::User).await;
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: "/gone/a.flac".into(),
                title: "Untitled".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.library_track_ids().await.unwrap()[0];
        let put = |body: String, who: &String| {
            let (state, who) = (state.clone(), who.clone());
            async move {
                call(
                    &state,
                    Method::PUT,
                    "/api/tracks/edit",
                    Some(&body),
                    Some(&who),
                )
                .await
                .0
            }
        };

        let body = format!(r#"{{"track_ids":[{id}],"title":" Lush Life ","year":2015}}"#);
        assert_eq!(put(body.clone(), &user).await, StatusCode::FORBIDDEN);
        assert_eq!(put(body, &admin).await, StatusCode::NO_CONTENT);
        // Left out stays, null goes back to the file's value.
        let body = format!(r#"{{"track_ids":[{id}],"year":null,"genre":"Pop"}}"#);
        assert_eq!(put(body, &admin).await, StatusCode::NO_CONTENT);
        let edit = state.store.tag_edit(id).await.unwrap();
        assert_eq!(edit.title.as_deref(), Some("Lush Life"));
        assert_eq!(edit.year, None);
        assert_eq!(edit.genre.as_deref(), Some("Pop"));

        let body = format!(r#"{{"track_ids":[{id}],"year":20150}}"#);
        assert_eq!(put(body, &admin).await, StatusCode::BAD_REQUEST);
        let body = r#"{"track_ids":[]}"#.to_owned();
        assert_eq!(put(body, &admin).await, StatusCode::BAD_REQUEST);
    }
}
