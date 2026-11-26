//! DJINN GO: your own library. Likes, history, where you left off, your
//! own playlists, personal radio, and sending a song to the party. Only on
//! the GO port; the guard there has already checked the role.

use std::collections::HashSet;

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_store::{
    GoPlaylist, GoPlaylistDetail, GoPrefs, LikeKind, Liked, LikedIds, Played, RequestOrigin,
    Resume, Track, User,
};
use serde::{Deserialize, Serialize};

use crate::auth::{CurrentSession, Device};
use crate::error::{ApiError, ApiResult};
use crate::go::GoRequest;
use crate::state::AppState;

const MAX_PLAYLIST_NAME: usize = 100;
const MAX_PLAYLIST_SONGS: usize = 20_000;
/// Songs saved of a queue to carry on from.
const MAX_RESUME_QUEUE: usize = 5000;
const RADIO_SONGS: usize = 30;
/// How many songs smart cache gets to choose from.
const OFFLINE_SUGGESTIONS: i64 = 1000;

/// Personal radio avoids songs heard this recently.
const RADIO_FRESH_MS: i64 = 3 * 60 * 60 * 1000;
/// ...and leans away from songs skipped this recently.
const RADIO_SKIPS_MS: i64 = 30 * 24 * 60 * 60 * 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/go/home", get(home))
        .route("/go/likes", get(liked))
        .route("/go/likes/ids", get(liked_ids))
        .route("/go/likes/{kind}/{id}", put(like).delete(unlike))
        .route("/go/plays", post(played))
        .route("/go/offline/versions", post(offline_versions))
        .route("/go/offline-suggested", get(offline_suggested))
        .route("/go/history", get(history))
        .route("/go/year", get(crate::go_year::year))
        .route("/go/resume", get(resume).put(save_resume))
        .route("/go/prefs", get(prefs).put(set_prefs))
        .route("/go/playlists", get(playlists).post(create_playlist))
        .route(
            "/go/playlists/{id}",
            get(playlist).patch(update_playlist).delete(delete_playlist),
        )
        .route("/go/playlists/{id}/songs", post(add_songs).put(set_songs))
        .route("/go/radio/{id}", get(radio))
        .route("/go/wild", get(wild))
        .route("/go/party/{id}", post(send_to_party))
}

/// A signed-in user, on the GO port only (404 elsewhere).
pub struct GoUser {
    pub user: User,
}

impl FromRequestParts<AppState> for GoUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        if parts.extensions.get::<GoRequest>().is_none() {
            return Err(ApiError::not_found());
        }
        let session = CurrentSession::from_request_parts(parts, state).await?;
        Ok(Self { user: session.user })
    }
}

#[derive(Serialize)]
struct Home {
    resume: Option<ResumeWithSongs>,
    recent: Vec<Track>,
    playlists: Vec<GoPlaylist>,
    newest: Vec<Track>,
}

#[derive(Serialize)]
struct ResumeWithSongs {
    #[serde(flatten)]
    resume: Resume,
    songs: Vec<Track>,
}

async fn resume_with_songs(state: &AppState, user_id: i64) -> ApiResult<Option<ResumeWithSongs>> {
    let Some(resume) = state.store.resume(user_id).await? else {
        return Ok(None);
    };
    let songs = state.store.tracks_by_ids(&resume.queue).await?;
    if songs.is_empty() {
        return Ok(None);
    }
    Ok(Some(ResumeWithSongs { resume, songs }))
}

/// The start page: where you left off, lately played, your playlists, and
/// what's new in the library.
async fn home(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Home>> {
    let newest = state
        .store
        .tracks_page(djinn_store::TrackSort::Added, 0, 20)
        .await?
        .items;
    Ok(Json(Home {
        resume: resume_with_songs(&state, me.user.id).await?,
        recent: state.store.recently_played(me.user.id, 20).await?,
        playlists: state.store.go_playlists(me.user.id).await?,
        newest,
    }))
}

async fn liked(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Liked>> {
    Ok(Json(state.store.liked(me.user.id).await?))
}

async fn liked_ids(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<LikedIds>> {
    Ok(Json(state.store.liked_ids(me.user.id).await?))
}

async fn like(
    me: GoUser,
    State(state): State<AppState>,
    Path((kind, id)): Path<(LikeKind, i64)>,
) -> ApiResult<StatusCode> {
    state.store.set_like(me.user.id, kind, id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlike(
    me: GoUser,
    State(state): State<AppState>,
    Path((kind, id)): Path<(LikeKind, i64)>,
) -> ApiResult<StatusCode> {
    state.store.set_like(me.user.id, kind, id, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PlayReport {
    track_id: i64,
    /// How much was heard; under 30 seconds counts as a skip.
    ms_played: i64,
    /// When, for a play reported later (it happened offline). Now without.
    at: Option<i64>,
    /// Skipped with "play less": counts against the song in radio.
    #[serde(default)]
    play_less: bool,
}

#[derive(Deserialize)]
struct SongIds {
    ids: Vec<i64>,
}

/// The current version of each song still in the library, so an app can
/// replace copies of changed files and notice removed ones.
async fn offline_versions(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<SongIds>,
) -> ApiResult<Json<Vec<djinn_store::TrackVersion>>> {
    downloads_allowed(&state, &me).await?;
    Ok(Json(state.store.track_versions(&body.ids).await?))
}

/// What's worth keeping on a device: most played first, then liked.
async fn offline_suggested(
    me: GoUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<Track>>> {
    downloads_allowed(&state, &me).await?;
    Ok(Json(
        state
            .store
            .worth_keeping(me.user.id, OFFLINE_SUGGESTIONS)
            .await?,
    ))
}

async fn downloads_allowed(state: &AppState, me: &GoUser) -> ApiResult<()> {
    if state.store.permissions(&me.user).await?.downloads {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

/// The player reports each song when it ends or is left.
async fn played(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<PlayReport>,
) -> ApiResult<StatusCode> {
    state
        .store
        .record_go_play_at(
            me.user.id,
            body.track_id,
            body.ms_played,
            body.at,
            body.play_less,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PageQuery {
    #[serde(default)]
    offset: i64,
}

async fn history(
    me: GoUser,
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Vec<Played>>> {
    Ok(Json(
        state
            .store
            .go_history(me.user.id, query.offset.max(0), 100)
            .await?,
    ))
}

async fn resume(
    me: GoUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Option<ResumeWithSongs>>> {
    Ok(Json(resume_with_songs(&state, me.user.id).await?))
}

/// The player saves where it is every so often, and on pause.
async fn save_resume(
    me: GoUser,
    State(state): State<AppState>,
    Json(mut body): Json<Resume>,
) -> ApiResult<StatusCode> {
    body.queue.truncate(MAX_RESUME_QUEUE);
    state.store.save_resume(me.user.id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn prefs(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<GoPrefs>> {
    Ok(Json(state.store.go_prefs(me.user.id).await?))
}

async fn set_prefs(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<GoPrefs>,
) -> ApiResult<StatusCode> {
    state.store.set_go_prefs(me.user.id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn playlists(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Vec<GoPlaylist>>> {
    Ok(Json(state.store.go_playlists(me.user.id).await?))
}

#[derive(Deserialize)]
struct NewPlaylist {
    name: String,
    #[serde(default)]
    songs: Vec<i64>,
}

fn playlist_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_PLAYLIST_NAME {
        return Err(ApiError::bad_request(format!(
            "pick a name of 1 to {MAX_PLAYLIST_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}

async fn create_playlist(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<NewPlaylist>,
) -> ApiResult<(StatusCode, Json<GoPlaylistDetail>)> {
    let name = playlist_name(&body.name)?;
    let id = state.store.create_go_playlist(me.user.id, &name).await?;
    if !body.songs.is_empty() {
        let songs: Vec<i64> = body.songs.into_iter().take(MAX_PLAYLIST_SONGS).collect();
        state
            .store
            .set_go_playlist_tracks(me.user.id, id, &songs)
            .await?;
    }
    let detail = state
        .store
        .go_playlist(me.user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(detail)))
}

async fn playlist(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<GoPlaylistDetail>> {
    let detail = state
        .store
        .go_playlist(me.user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct PlaylistChange {
    name: Option<String>,
    shared: Option<bool>,
}

async fn update_playlist(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PlaylistChange>,
) -> ApiResult<StatusCode> {
    let name = body.name.as_deref().map(playlist_name).transpose()?;
    if body.shared == Some(true) && !state.store.permissions(&me.user).await?.share_playlists {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "your account can't share playlists",
        ));
    }
    if !state
        .store
        .update_go_playlist(me.user.id, id, name.as_deref(), body.shared)
        .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_playlist(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_go_playlist(me.user.id, id).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Songs {
    songs: Vec<i64>,
}

async fn add_songs(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Songs>,
) -> ApiResult<StatusCode> {
    if body.songs.len() > MAX_PLAYLIST_SONGS
        || !state
            .store
            .add_to_go_playlist(me.user.id, id, &body.songs)
            .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces a playlist's songs: for removing and reordering.
async fn set_songs(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Songs>,
) -> ApiResult<StatusCode> {
    if body.songs.len() > MAX_PLAYLIST_SONGS
        || !state
            .store
            .set_go_playlist_tracks(me.user.id, id, &body.songs)
            .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RadioQuery {
    /// Songs already lined up, comma separated, when asking for more: the draw
    /// favors the same few songs, so without this it soon offers nothing new.
    #[serde(default)]
    exclude: String,
}

/// Personal radio from a song: songs that sound like it, more often by its
/// artist, skipping ones you heard in the last few hours and putting ones you
/// skipped lately last. Without sound analysis, the same artist and genre.
async fn radio(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<RadioQuery>,
) -> ApiResult<Json<Vec<Track>>> {
    let seed = state
        .store
        .track(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let now = djinn_store::now_ms();
    let (fresh_played, fresh_skipped) = state
        .store
        .go_listening_since(me.user.id, now - RADIO_FRESH_MS)
        .await?;
    // Only "play less" keeps songs (and ones like them) away; a plain skip
    // just means not now.
    let skipped_list = state
        .store
        .go_play_less_since(me.user.id, now - RADIO_SKIPS_MS)
        .await?;
    let heard: HashSet<i64> = fresh_played
        .iter()
        .copied()
        .chain(fresh_skipped)
        .chain([id])
        .chain(query.exclude.split(',').filter_map(|t| t.parse().ok()))
        .collect();
    let skipped: HashSet<i64> = skipped_list.iter().copied().collect();
    // Weighted like the party's radio: by sound, leaning on the seed's artist.
    let mut ids = crate::smart::personal_radio(&state, id, &fresh_played, &skipped_list);
    if ids.is_empty() {
        ids = fallback_radio(&state, &seed).await?;
    } else {
        // Skipped songs are out of the draw; better last than a radio that stops.
        ids.extend(skipped_list.iter().copied().filter(|t| !heard.contains(t)));
    }
    ids.retain(|t| !heard.contains(t));
    // Stable: keeps the drawn order within "not skipped" and "skipped".
    ids.sort_by_key(|t| skipped.contains(t));
    ids.truncate(RADIO_SONGS);
    let mut songs = vec![seed];
    songs.extend(state.store.tracks_by_ids(&ids).await?);
    Ok(Json(songs))
}

/// Songs wild play hands out at a time.
const WILD_SONGS: usize = 30;

/// Wild play: songs from the whole library, drawn at random, more often the
/// ones you like or play through and less often ones you said "play less"
/// to, leaving out what you heard in the last few hours and what's already
/// lined up (`exclude`).
async fn wild(
    me: GoUser,
    State(state): State<AppState>,
    Query(query): Query<RadioQuery>,
) -> ApiResult<Json<Vec<Track>>> {
    let now = djinn_store::now_ms();
    let (fresh_played, fresh_skipped) = state
        .store
        .go_listening_since(me.user.id, now - RADIO_FRESH_MS)
        .await?;
    let lined: Vec<i64> = query
        .exclude
        .split(',')
        .filter_map(|t| t.parse().ok())
        .collect();
    let heard: HashSet<i64> = fresh_played.into_iter().chain(fresh_skipped).collect();
    let library = state.store.library_track_ids().await?;
    let library = wild_pool(&library, &heard, &lined);
    let history = state.store.go_song_history(me.user.id).await?;
    let liked: HashSet<i64> = state
        .store
        .liked_ids(me.user.id)
        .await?
        .tracks
        .into_iter()
        .collect();
    let ids = wild_draw(&library, &history, &liked, WILD_SONGS, rand::random::<f32>);
    Ok(Json(state.store.tracks_by_ids(&ids).await?))
}

/// What wild play may draw from: the library less what you heard lately and
/// what's lined up (newest last). A small library that has all played starts
/// over, leaving out only the newer half of what's lined up, so it never stops.
fn wild_pool(library: &[i64], heard: &HashSet<i64>, lined: &[i64]) -> Vec<i64> {
    let lined_set: HashSet<i64> = lined.iter().copied().collect();
    let fresh: Vec<i64> = library
        .iter()
        .copied()
        .filter(|id| !heard.contains(id) && !lined_set.contains(id))
        .collect();
    if !fresh.is_empty() {
        return fresh;
    }
    let recent: HashSet<i64> = lined[lined.len() / 2..].iter().copied().collect();
    library
        .iter()
        .copied()
        .filter(|id| !recent.contains(id))
        .collect()
}

/// How much a song weighs in your wild play: a liked song twice as much,
/// played through more (up to 4x after ten), marked "play less" less (down
/// to 0.1x). Songs you never played weigh 1.
fn wild_weight(finished: i64, play_less: i64, liked: bool) -> f32 {
    let played = 1.0 + 0.3 * finished.clamp(0, 10) as f32;
    let kept = (finished.max(0) as f32 + 1.0)
        / (finished.max(0) as f32 + 1.0 + 3.0 * play_less.max(0) as f32);
    let liked = if liked { 2.0 } else { 1.0 };
    (played * kept * liked).clamp(0.1, 8.0)
}

/// `count` of `library` in a weighted random order.
fn wild_draw(
    library: &[i64],
    history: &[(i64, i64, i64)],
    liked: &HashSet<i64>,
    count: usize,
    mut random: impl FnMut() -> f32,
) -> Vec<i64> {
    let heard: std::collections::HashMap<i64, (i64, i64)> =
        history.iter().map(|&(id, f, l)| (id, (f, l))).collect();
    let mut keyed: Vec<(f32, i64)> = library
        .iter()
        .map(|&id| {
            let (finished, less) = heard.get(&id).copied().unwrap_or((0, 0));
            let weight = wild_weight(finished, less, liked.contains(&id));
            let roll = random().clamp(1e-6, 1.0);
            (-roll.ln() / weight, id)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    keyed.into_iter().take(count).map(|(_, id)| id).collect()
}

/// Same artist, then same genre, shuffled: radio without sound analysis.
async fn fallback_radio(state: &AppState, seed: &Track) -> ApiResult<Vec<i64>> {
    let mut ids = Vec::new();
    if let Some(artist) = seed.artist_id {
        ids.extend(
            state
                .store
                .tracks_for(djinn_store::EntryKind::Artist, artist)
                .await?,
        );
    }
    if let Some(genre) = seed.genre.as_deref() {
        let first = genre.split([',', ';', '/']).next().unwrap_or("").trim();
        if !first.is_empty() {
            let found = state.store.search(first, 100).await?;
            ids.extend(found.tracks.iter().map(|t| t.id));
        }
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    shuffle(&mut ids);
    Ok(ids)
}

fn shuffle(ids: &mut [i64]) {
    use rand::seq::SliceRandom;
    ids.shuffle(&mut rand::rng());
}

#[derive(Serialize)]
struct SentToParty {
    /// "added", or "backed" when someone had already asked for it.
    outcome: djinn_store::RequestOutcome,
}

/// Asks the party for a song, like requesting it from the party app. Only
/// from home: the party plays there.
async fn send_to_party(
    me: GoUser,
    State(state): State<AppState>,
    device: Device,
    Path(id): Path<i64>,
) -> ApiResult<Json<SentToParty>> {
    if !device.home {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "you can send songs to the party from home",
        ));
    }
    let origin = RequestOrigin {
        name: me.user.name().to_owned(),
        mac: me.user.mac.clone(),
        ip: device.ip.clone(),
    };
    // Into the room this person is in at the party.
    let (_, outcome) = state
        .jukebox_of(me.user.id)
        .await
        .lock()
        .await
        .add_request(&me.user, id, &origin)
        .await
        .map_err(crate::player_api::queue_error)?;
    Ok(Json(SentToParty { outcome }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wild_weights_follow_what_you_did() {
        assert_eq!(wild_weight(0, 0, false), 1.0);
        assert!(wild_weight(0, 0, true) > wild_weight(0, 0, false));
        assert!(wild_weight(5, 0, false) > wild_weight(1, 0, false));
        assert!(wild_weight(0, 1, false) < 0.5);
        assert!(wild_weight(0, 3, true) >= 0.1);
        assert!(wild_weight(50, 0, true) <= 8.0);
    }

    #[test]
    fn wild_draw_takes_each_song_once_and_favors_liked_ones() {
        let library: Vec<i64> = (1..=100).collect();
        let liked: HashSet<i64> = [1].into();
        let history = [(2, 0, 5)];
        let mut seed = 7_u32;
        let mut random = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        let (mut liked_first, mut less_first) = (0, 0);
        for _ in 0..400 {
            let drawn = wild_draw(&library, &history, &liked, 30, &mut random);
            assert_eq!(drawn.len(), 30);
            let unique: HashSet<i64> = drawn.iter().copied().collect();
            assert_eq!(unique.len(), 30);
            liked_first += usize::from(drawn.contains(&1));
            less_first += usize::from(drawn.contains(&2));
        }
        // Out of 100 even songs each lands in 30 about 30% of the time.
        assert!(liked_first > 160, "liked {liked_first}");
        assert!(less_first < 60, "play less {less_first}");
    }

    #[test]
    fn wild_play_starts_over_when_everything_played() {
        let library = [1, 2, 3, 4];
        let heard: HashSet<i64> = [1].into();
        assert_eq!(wild_pool(&library, &heard, &[2]), vec![3, 4]);
        // All lined up or heard: the older half of the queue comes back.
        assert_eq!(wild_pool(&library, &heard, &[2, 3, 4, 1]), vec![2, 3]);
        assert_eq!(wild_pool(&library, &HashSet::new(), &[]), vec![1, 2, 3, 4]);
    }

    #[test]
    fn wild_draw_with_a_small_library_gives_what_there_is() {
        let drawn = wild_draw(&[4, 5], &[], &HashSet::new(), 30, || 0.5);
        assert_eq!(drawn.len(), 2);
    }
}
