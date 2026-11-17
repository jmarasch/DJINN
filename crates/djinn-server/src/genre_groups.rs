//! Genre containers (Admin > Library > Genres): similar genres grouped under
//! one name, so "black metal", "death metal" and "speed metal" show as
//! "Metal". Off by default (`genre_containers`). While on, genre lists, the
//! genre cloud and genre pages show containers in place of their genres, and
//! a container's page holds every song of its genres; artist and album pages
//! keep the real genres with the container shown first. Nothing in the
//! library changes, and turning it off puts everything back.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use djinn_library::genre_groups as clusters;
use djinn_store::{GenreGroup, GenreMember};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::settings;
use crate::state::AppState;

/// On/off: fold genres into their containers.
pub const GENRE_CONTAINERS: &str = "genre_containers";
/// Longest container name.
const NAME_MOST: usize = 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/genre-groups",
            get(overview).post(create).delete(clear_all),
        )
        .route("/genre-groups/fold", get(fold_for_pages))
        .route("/genre-groups/enabled", put(set_enabled))
        .route("/genre-groups/choices", get(choices))
        .route("/genre-groups/auto", post(auto))
        .route("/genre-groups/place", put(place))
        .route("/genre-groups/{id}", put(rename).delete(remove))
        .route("/genre-groups/free/{genre}", delete(free))
}

/// Reads the containers into memory (empty while they're off), and drops
/// genre pages worked out with the old ones.
pub async fn reload(state: &AppState) {
    let on = settings::flag(&state.store, GENRE_CONTAINERS, false)
        .await
        .unwrap_or(false);
    let fold: HashMap<String, String> = if on {
        match state.store.genre_fold().await {
            Ok(rows) => rows.into_iter().collect(),
            Err(e) => {
                tracing::warn!(error = %e, "couldn't read the genre containers");
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };
    if let Ok(mut kept) = state.genre_fold.write() {
        *kept = fold;
    }
    if let Ok(mut pulls) = state.genre_pulls.lock() {
        pulls.clear();
    }
}

/// Every genre in the library and how many songs have it.
fn genre_counts(state: &AppState) -> Vec<(String, usize)> {
    let Ok(catalog) = state.catalog.read() else {
        return Vec::new();
    };
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for names in catalog.song_tags().values() {
        for name in names {
            *counts.entry(name.as_str()).or_default() += 1;
        }
    }
    let mut out: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(name, n)| (name.to_owned(), n))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[derive(Serialize)]
struct GenreCount {
    genre: String,
    songs: usize,
    /// Placed by hand (Auto-group leaves it where it is).
    pinned: bool,
}

#[derive(Serialize)]
struct Container {
    #[serde(flatten)]
    group: GenreGroup,
    genres: Vec<GenreCount>,
}

#[derive(Serialize)]
struct Overview {
    enabled: bool,
    containers: Vec<Container>,
    /// Genres in no container.
    loose: Vec<GenreCount>,
}

async fn overview(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Overview>> {
    let enabled = settings::flag(&state.store, GENRE_CONTAINERS, false).await?;
    let (groups, members) = state.store.genre_groups().await?;
    let placed: HashMap<&str, &GenreMember> =
        members.iter().map(|m| (m.genre.as_str(), m)).collect();
    let mut containers: Vec<Container> = groups
        .into_iter()
        .map(|group| Container {
            group,
            genres: Vec::new(),
        })
        .collect();
    let at: HashMap<i64, usize> = containers
        .iter()
        .enumerate()
        .map(|(i, c)| (c.group.id, i))
        .collect();
    let mut loose = Vec::new();
    for (genre, songs) in genre_counts(&state) {
        let member = placed.get(genre.as_str());
        let row = GenreCount {
            pinned: member.is_some_and(|m| m.pinned),
            genre,
            songs,
        };
        match member.and_then(|m| m.group_id).and_then(|id| at.get(&id)) {
            Some(&i) => containers[i].genres.push(row),
            None => loose.push(row),
        }
    }
    for c in &mut containers {
        c.genres
            .sort_by(|a, b| b.songs.cmp(&a.songs).then_with(|| a.genre.cmp(&b.genre)));
    }
    loose.sort_by(|a, b| b.songs.cmp(&a.songs).then_with(|| a.genre.cmp(&b.genre)));
    Ok(Json(Overview {
        enabled,
        containers,
        loose,
    }))
}

/// Genre to container, for chips on artist, album and song pages (empty
/// while containers are off).
async fn fold_for_pages(
    _: CurrentUser,
    State(state): State<AppState>,
) -> Json<HashMap<String, String>> {
    Json(
        state
            .genre_fold
            .read()
            .map(|f| f.clone())
            .unwrap_or_default(),
    )
}

#[derive(Deserialize)]
struct Enabled {
    on: bool,
}

async fn set_enabled(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Enabled>,
) -> ApiResult<StatusCode> {
    settings::set_flag(&state.store, GENRE_CONTAINERS, body.on).await?;
    reload(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The genres Auto-group sorts: all but the ones placed by hand or in a
/// container someone made or changed.
async fn clustered(state: &AppState) -> anyhow::Result<clusters::Tree> {
    let (groups, members) = state.store.genre_groups().await?;
    let kept: HashSet<i64> = groups.iter().filter(|g| !g.auto).map(|g| g.id).collect();
    let skip: HashSet<String> = members
        .into_iter()
        .filter(|m| m.pinned || m.group_id.is_some_and(|id| kept.contains(&id)))
        .map(|m| m.genre)
        .collect();
    let (Ok(catalog), Ok(index)) = (state.catalog.read(), state.sound.read()) else {
        return Ok(clusters::Tree::default());
    };
    let map = clusters::full_map(catalog.song_tags(), &index);
    Ok(clusters::tree(&map, &skip))
}

#[derive(Serialize)]
struct Choices {
    /// Fewest containers first; each is a whole number of containers.
    choices: Vec<clusters::Choice>,
}

/// The ways Auto-group can sort the genres, for the slider: worked out once
/// when the page opens.
async fn choices(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Choices>> {
    let tree = clustered(&state).await?;
    Ok(Json(Choices {
        choices: tree.choices(),
    }))
}

#[derive(Deserialize)]
struct Auto {
    /// About how many containers.
    containers: usize,
}

#[derive(Serialize)]
struct Made {
    containers: usize,
    grouped: usize,
}

/// Groups the genres into about the number of containers asked for,
/// replacing the containers Auto-group made before.
async fn auto(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Auto>,
) -> ApiResult<Json<Made>> {
    let tree = clustered(&state).await?;
    let Some(choice) = tree.nearest(body.containers) else {
        return Err(ApiError::bad_request(
            "There aren't enough related genres to group yet",
        ));
    };
    let groups = tree.named(choice.step);
    state.store.replace_auto_genre_groups(&groups).await?;
    reload(&state).await;
    Ok(Json(Made {
        containers: groups.len(),
        grouped: groups.iter().map(|(_, members)| members.len()).sum(),
    }))
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

fn clean_name(name: &str) -> ApiResult<String> {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(ApiError::bad_request("Give the container a name"));
    }
    Ok(name.chars().take(NAME_MOST).collect())
}

#[derive(Serialize)]
struct Created {
    id: i64,
}

async fn create(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Name>,
) -> ApiResult<Json<Created>> {
    let name = clean_name(&body.name)?;
    let id = state
        .store
        .create_genre_group(&name)
        .await?
        .ok_or_else(|| ApiError::conflict("There's a container with that name already"))?;
    reload(&state).await;
    Ok(Json(Created { id }))
}

async fn rename(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Name>,
) -> ApiResult<StatusCode> {
    let name = clean_name(&body.name)?;
    if !state.store.rename_genre_group(id, &name).await? {
        return Err(ApiError::conflict(
            "That container is gone, or another has the name",
        ));
    }
    reload(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_genre_group(id).await? {
        return Err(ApiError::not_found());
    }
    reload(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Place {
    genre: String,
    /// The container, or none to take it out of every container.
    group: Option<i64>,
}

async fn place(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Place>,
) -> ApiResult<StatusCode> {
    let genre = body.genre.trim();
    if genre.is_empty() {
        return Err(ApiError::bad_request("No genre given"));
    }
    if !state.store.place_genre(genre, body.group).await? {
        return Err(ApiError::not_found());
    }
    reload(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Lets a genre placed by hand go back to Auto-group.
async fn free(
    _: AdminUser,
    State(state): State<AppState>,
    Path(genre): Path<String>,
) -> ApiResult<StatusCode> {
    state.store.free_genre(&genre).await?;
    reload(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Takes every container away.
async fn clear_all(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.clear_genre_groups().await?;
    reload(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The container a name is (case aside), when containers are on.
pub fn container_named(state: &AppState, name: &str) -> Option<String> {
    let fold = state.genre_fold.read().ok()?;
    let lower = name.to_lowercase();
    fold.values().find(|c| c.to_lowercase() == lower).cloned()
}

/// The genre map's song genres as shown: containers folded in while on.
pub fn shown_tags(
    state: &AppState,
    tags: &HashMap<i64, Vec<String>>,
) -> Option<HashMap<i64, Vec<String>>> {
    let fold = state.genre_fold.read().ok()?;
    if fold.is_empty() {
        return None;
    }
    Some(clusters::fold_tags(tags, &fold))
}

/// Genre names as shown: each in a container replaced by it, once.
pub fn fold_names(state: &AppState, names: Vec<String>) -> Vec<String> {
    let Ok(fold) = state.genre_fold.read() else {
        return names;
    };
    if fold.is_empty() {
        return names;
    }
    let mut out: Vec<String> = Vec::with_capacity(names.len());
    for name in names {
        let shown = fold.get(&name).cloned().unwrap_or(name);
        if !out.contains(&shown) {
            out.push(shown);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::{Role, ScannedTrack};

    fn names(body: &str) -> Vec<String> {
        let map: serde_json::Value = serde_json::from_str(body).unwrap();
        map["genres"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn containers_fold_genres_only_while_on() {
        let state = AppState::for_tests().await;
        let song = |n: &str, genre: &str| ScannedTrack {
            path: format!("/m/{n}.flac"),
            title: n.into(),
            artist: Some("Band".into()),
            genre: Some(genre.into()),
            duration_ms: Some(200_000),
            ..Default::default()
        };
        state
            .store
            .save_scanned(&[
                song("a", "Black Metal"),
                song("b", "Death Metal"),
                song("c", "House"),
            ])
            .await
            .unwrap();
        crate::smart::refresh(&state).await;
        let admin = session(&state, "ana", Role::Admin).await;
        let listener = session(&state, "lu", Role::User).await;

        let (status, _) = call(
            &state,
            Method::POST,
            "/api/genre-groups",
            Some(r#"{"name":"Metal"}"#),
            Some(&listener),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/genre-groups",
            Some(r#"{"name":"  Metal "}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = get_body(&state, "/api/genre-groups", &admin).await;
        let overview: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            overview["containers"][0]["name"], "Metal",
            "the name is tidied"
        );
        let id = overview["containers"][0]["id"].as_i64().unwrap();
        let (_, body) = get_body(&state, "/api/genres", &admin).await;
        let genres = names(&body);
        for genre in ["black metal", "death metal"] {
            assert!(genres.contains(&genre.to_owned()), "{genres:?}");
            let place = format!(r#"{{"genre":"{genre}","group":{id}}}"#);
            let (status, _) = call(
                &state,
                Method::PUT,
                "/api/genre-groups/place",
                Some(&place),
                Some(&admin),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        }
        // Still off: nothing folds.
        let (_, body) = get_body(&state, "/api/genres", &listener).await;
        assert!(names(&body).contains(&"black metal".to_owned()));
        let (_, body) = get_body(&state, "/api/genre-groups/fold", &listener).await;
        assert_eq!(body, "{}");

        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/genre-groups/enabled",
            Some(r#"{"on":true}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = get_body(&state, "/api/genres", &listener).await;
        let genres = names(&body);
        assert!(genres.contains(&"Metal".to_owned()), "{genres:?}");
        assert!(!genres.contains(&"black metal".to_owned()), "{genres:?}");
        let (_, body) = get_body(&state, "/api/genre-groups/fold", &listener).await;
        let fold: HashMap<String, String> = serde_json::from_str(&body).unwrap();
        assert_eq!(fold.len(), 2);
        // The container's page holds both songs, whatever the case asked.
        let (status, body) = get_body(&state, "/api/genres/metal", &listener).await;
        assert_eq!(status, StatusCode::OK);
        let page: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(page["total"], 2, "{body}");
        // The admin page lists the container with its genres and the loose one.
        let (_, body) = get_body(&state, "/api/genre-groups", &admin).await;
        let overview: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            overview["containers"][0]["genres"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(overview["loose"][0]["genre"], "house");
    }
}
