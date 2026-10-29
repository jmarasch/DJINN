//! Smart play (admin): party styles, the switches for learning from the
//! party, and what each style has learned.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use djinn_store::{ArtistSummary, PartyStyle, PartyStyleError, SongHistory, Track};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::jukebox::{Learned, SMART_PLAY, SMART_PLAYLISTS, SMART_STRENGTH};
use crate::settings;
use crate::state::AppState;

const MAX_NAME: usize = 40;
/// Favored and avoided entries shown for each of songs, artists, genres.
const SHOWN: usize = 10;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/party", get(overview))
        .route("/party/settings", put(set_settings))
        .route("/party/styles", post(create))
        .route("/party/styles/{id}", patch(rename).delete(remove))
        .route("/party/styles/{id}/use", post(use_style))
        .route("/party/styles/{id}/clear", post(clear))
        .route("/party/styles/{id}/learned", get(learned))
}

#[derive(Serialize, Deserialize)]
struct Settings {
    /// Weigh wild play and radio by what parties did.
    smart_play: bool,
    /// Also order shuffled playlists by it.
    smart_playlists: bool,
    /// How much it counts, 0 to 100.
    strength: i64,
}

#[derive(Serialize)]
struct Overview {
    #[serde(flatten)]
    settings: Settings,
    /// The style in use.
    active: Option<i64>,
    styles: Vec<PartyStyle>,
}

async fn overview(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Overview>> {
    let store = &state.store;
    Ok(Json(Overview {
        settings: Settings {
            smart_play: settings::flag(store, SMART_PLAY, true).await?,
            smart_playlists: settings::flag(store, SMART_PLAYLISTS, false).await?,
            strength: settings::number(store, SMART_STRENGTH, 100).await?,
        },
        active: store.active_party_style().await?,
        styles: store.party_styles().await?,
    }))
}

async fn set_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Settings>,
) -> ApiResult<StatusCode> {
    if !(0..=100).contains(&body.strength) {
        return Err(ApiError::bad_request("strength is 0 to 100"));
    }
    let store = &state.store;
    settings::set_flag(store, SMART_PLAY, body.smart_play).await?;
    settings::set_flag(store, SMART_PLAYLISTS, body.smart_playlists).await?;
    store
        .set_setting(SMART_STRENGTH, &body.strength.to_string())
        .await?;
    state.jukebox.lock().await.party_changed();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

async fn create(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Name>,
) -> ApiResult<(StatusCode, Json<PartyStyle>)> {
    let name = validate_name(&body.name)?;
    let style = state
        .store
        .create_party_style(&name)
        .await
        .map_err(style_error)?;
    Ok((StatusCode::CREATED, Json(style)))
}

async fn rename(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Name>,
) -> ApiResult<StatusCode> {
    let name = validate_name(&body.name)?;
    if !state
        .store
        .rename_party_style(id, &name)
        .await
        .map_err(style_error)?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state
        .store
        .delete_party_style(id)
        .await
        .map_err(style_error)?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn use_style(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.use_party_style(id).await? {
        return Err(ApiError::not_found());
    }
    state.jukebox.lock().await.party_changed();
    Ok(StatusCode::NO_CONTENT)
}

async fn clear(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.clear_party_style(id).await? {
        return Err(ApiError::not_found());
    }
    state.jukebox.lock().await.party_changed();
    Ok(StatusCode::NO_CONTENT)
}

/// The most favored and most avoided of something, strongest first.
#[derive(Serialize)]
struct Extremes<T> {
    favored: Vec<T>,
    avoided: Vec<T>,
}

#[derive(Serialize)]
struct LearnedSong {
    track: Track,
    weight: f32,
    requests: i64,
    finished: i64,
    skipped: i64,
}

#[derive(Serialize)]
struct LearnedArtist {
    artist: ArtistSummary,
    weight: f32,
}

#[derive(Serialize)]
struct LearnedGenre {
    name: String,
    weight: f32,
}

#[derive(Serialize)]
struct LearnedView {
    songs: Extremes<LearnedSong>,
    artists: Extremes<LearnedArtist>,
    genres: Extremes<LearnedGenre>,
}

async fn learned(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<LearnedView>> {
    if state.store.party_style(id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    let history = state.store.party_history(id).await?;
    let links = state.jukebox.lock().await.links();
    let learned = Learned::new(&history, &links);

    let songs = extremes(&learned.songs);
    let ids: Vec<i64> = songs
        .favored
        .iter()
        .chain(&songs.avoided)
        .copied()
        .collect();
    let tracks = state.store.tracks_by_ids(&ids).await?;
    let song = |id: &i64| -> Option<LearnedSong> {
        let track = tracks.iter().find(|t| t.id == *id)?.clone();
        let h = history
            .iter()
            .find(|h| h.track_id == *id)
            .copied()
            .unwrap_or(SongHistory::default());
        Some(LearnedSong {
            weight: learned.songs.get(id).copied().unwrap_or(1.0),
            track,
            requests: h.requests,
            finished: h.finished,
            skipped: h.skipped,
        })
    };

    let artists = extremes(&learned.artists);
    let ids: Vec<i64> = artists
        .favored
        .iter()
        .chain(&artists.avoided)
        .copied()
        .collect();
    let found = state.store.artists_by_ids(&ids).await?;
    let artist = |id: &i64| -> Option<LearnedArtist> {
        Some(LearnedArtist {
            artist: found.iter().find(|a| a.id == *id)?.clone(),
            weight: learned.artists.get(id).copied().unwrap_or(1.0),
        })
    };

    let genres = extremes(&learned.genres);
    let genre = |name: &String| -> Option<LearnedGenre> {
        Some(LearnedGenre {
            weight: learned.genres.get(name).copied()?,
            name: name.clone(),
        })
    };

    Ok(Json(LearnedView {
        songs: Extremes {
            favored: songs.favored.iter().filter_map(song).collect(),
            avoided: songs.avoided.iter().filter_map(song).collect(),
        },
        artists: Extremes {
            favored: artists.favored.iter().filter_map(artist).collect(),
            avoided: artists.avoided.iter().filter_map(artist).collect(),
        },
        genres: Extremes {
            favored: genres.favored.iter().filter_map(genre).collect(),
            avoided: genres.avoided.iter().filter_map(genre).collect(),
        },
    }))
}

/// The keys of the [`SHOWN`] highest weights above 1 and lowest below 1.
fn extremes<K: Clone>(weights: &std::collections::HashMap<K, f32>) -> Extremes<K> {
    let mut sorted: Vec<(&K, f32)> = weights.iter().map(|(k, w)| (k, *w)).collect();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
    let favored = sorted
        .iter()
        .filter(|(_, w)| *w > 1.0)
        .take(SHOWN)
        .map(|(k, _)| (*k).clone())
        .collect();
    let avoided = sorted
        .iter()
        .rev()
        .filter(|(_, w)| *w < 1.0)
        .take(SHOWN)
        .map(|(k, _)| (*k).clone())
        .collect();
    Extremes { favored, avoided }
}

fn validate_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "style names are 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}

fn style_error(e: PartyStyleError) -> ApiError {
    match e {
        PartyStyleError::NameTaken | PartyStyleError::InUse => ApiError::conflict(e.to_string()),
        PartyStyleError::Store(e) => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn admins_manage_styles() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        let user = session(&state, "sam", Role::User).await;
        let send = |method, path: &'static str, body: Option<&'static str>, who: String| {
            let state = state.clone();
            async move { call(&state, method, path, body, Some(&who)).await.0 }
        };

        assert_eq!(
            send(Method::GET, "/api/party", None, user.clone()).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(
                Method::POST,
                "/api/party/styles",
                Some(r#"{"name":"Halloween"}"#),
                admin.clone()
            )
            .await,
            StatusCode::CREATED
        );
        assert_eq!(
            send(
                Method::POST,
                "/api/party/styles",
                Some(r#"{"name":"halloween"}"#),
                admin.clone()
            )
            .await,
            StatusCode::CONFLICT
        );
        assert_eq!(
            send(
                Method::POST,
                "/api/party/styles",
                Some(r#"{"name":"  "}"#),
                admin.clone()
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        let halloween = state.store.party_styles().await.unwrap()[1].id;
        assert_eq!(halloween, 2);
        assert_eq!(
            send(Method::POST, "/api/party/styles/2/use", None, admin.clone()).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(state.store.active_party_style().await.unwrap(), Some(2));
        assert_eq!(
            send(Method::DELETE, "/api/party/styles/2", None, admin.clone()).await,
            StatusCode::CONFLICT,
            "the style in use stays"
        );
        assert_eq!(
            send(
                Method::GET,
                "/api/party/styles/2/learned",
                None,
                admin.clone()
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            send(
                Method::GET,
                "/api/party/styles/9/learned",
                None,
                admin.clone()
            )
            .await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(
                Method::PUT,
                "/api/party/settings",
                Some(r#"{"smart_play":true,"smart_playlists":true,"strength":150}"#),
                admin.clone()
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(
                Method::PUT,
                "/api/party/settings",
                Some(r#"{"smart_play":false,"smart_playlists":true,"strength":40}"#),
                admin.clone()
            )
            .await,
            StatusCode::NO_CONTENT
        );
        assert!(!settings::flag(&state.store, SMART_PLAY, true)
            .await
            .unwrap());
        assert_eq!(
            settings::number(&state.store, SMART_STRENGTH, 100)
                .await
                .unwrap(),
            40
        );
    }
}
