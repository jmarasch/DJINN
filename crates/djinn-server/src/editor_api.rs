//! The library editor's folder tree and grid. Everything here is read only
//! and behind the protected areas' lock; it reads every library folder,
//! view only ones included. Changes will go through routes that also check
//! `editable::require`.

use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::compare::{self, FileTags};
use djinn_store::{EditorSong, FolderChild};
use serde::{Deserialize, Serialize};

use crate::editable::editable_roots;
use crate::error::{ApiError, ApiResult};
use crate::protected::Elevated;
use crate::state::AppState;

const PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/editor/tree", get(tree))
        .route("/editor/songs", get(songs))
}

#[derive(Deserialize)]
struct TreeQuery {
    /// A folder; empty for the library folders themselves.
    #[serde(default)]
    path: String,
}

#[derive(Serialize)]
struct Node {
    name: String,
    path: String,
    songs: i64,
    /// Library folders only: whether changes are allowed in it.
    editable: Option<bool>,
}

#[derive(Serialize)]
struct Tree {
    folders: Vec<Node>,
    songs_here: i64,
}

fn join(parent: &str, name: &str) -> String {
    let sep = if parent.contains('\\') && !parent.contains('/') {
        '\\'
    } else {
        '/'
    };
    format!("{}{sep}{name}", parent.trim_end_matches(['/', '\\']))
}

/// The folder must be one of the library folders or inside one.
async fn inside_library(state: &AppState, folder: &str) -> ApiResult<Vec<PathBuf>> {
    let roots = crate::folders::library_roots(state).await?;
    if !roots.iter().any(|r| Path::new(folder).starts_with(r)) {
        return Err(ApiError::bad_request("that isn't inside a library folder"));
    }
    Ok(roots)
}

async fn tree(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<TreeQuery>,
) -> ApiResult<Json<Tree>> {
    if query.path.is_empty() {
        let editable = editable_roots(&state).await?;
        let mut folders = Vec::new();
        for root in crate::folders::library_roots(&state).await? {
            let path = root.to_string_lossy().into_owned();
            let listing = state.store.folder_listing(&path).await?;
            folders.push(Node {
                name: root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                songs: listing.folders.iter().map(|f| f.songs).sum::<i64>() + listing.songs_here,
                editable: Some(editable.contains(&root)),
                path,
            });
        }
        return Ok(Json(Tree {
            folders,
            songs_here: 0,
        }));
    }
    inside_library(&state, &query.path).await?;
    let listing = state.store.folder_listing(&query.path).await?;
    Ok(Json(Tree {
        folders: listing
            .folders
            .into_iter()
            .map(|FolderChild { name, songs }| Node {
                path: join(&query.path, &name),
                name,
                songs,
                editable: None,
            })
            .collect(),
        songs_here: listing.songs_here,
    }))
}

#[derive(Deserialize)]
struct SongsQuery {
    folder: String,
    #[serde(default)]
    offset: i64,
    /// Also read each file's own tags (editing the library, not the database).
    #[serde(default)]
    files: bool,
}

#[derive(Serialize)]
struct Row {
    #[serde(flatten)]
    song: EditorSong,
    /// What the file says, when asked for; None if it couldn't be read.
    file: Option<FileTags>,
}

#[derive(Serialize)]
struct Songs {
    total: i64,
    /// Changes can be made here: the folder is allowed.
    editable: bool,
    songs: Vec<Row>,
}

async fn songs(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<SongsQuery>,
) -> ApiResult<Json<Songs>> {
    inside_library(&state, &query.folder).await?;
    let editable = editable_roots(&state)
        .await?
        .iter()
        .any(|root| Path::new(&query.folder).starts_with(root));
    let (found, total) = state
        .store
        .editor_songs(&query.folder, query.offset, PAGE)
        .await?;
    let files: Vec<Option<FileTags>> = if query.files {
        let original_years = djinn_library::original_years(&state.store).await?;
        let paths: Vec<String> = found.iter().map(|s| s.path.clone()).collect();
        tokio::task::spawn_blocking(move || {
            paths
                .iter()
                .map(|p| compare::read(Path::new(p), original_years).ok())
                .collect()
        })
        .await
        .map_err(anyhow::Error::from)?
    } else {
        vec![None; found.len()]
    };
    Ok(Json(Songs {
        total,
        editable,
        songs: found
            .into_iter()
            .zip(files)
            .map(|(song, file)| Row { song, file })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use djinn_store::Role;

    use crate::state::AppState;
    use crate::test_util::{get_body, session};

    #[tokio::test]
    async fn the_tree_and_grid_are_locked_and_stay_inside_the_library() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let (status, _) = get_body(&state, "/api/editor/tree", &admin).await;
        assert_eq!(status, StatusCode::LOCKED);
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let (status, body) = get_body(&state, "/api/editor/tree", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"folders\""), "{body}");
        // A folder outside every library folder is refused.
        let (status, _) = get_body(&state, "/api/editor/songs?folder=/etc", &admin).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
