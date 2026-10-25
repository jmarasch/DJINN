//! Library folders (admin): which folders hold music, and a browser for
//! picking new ones from the web page.
//!
//! Folders listed in config.toml always count and can only be changed
//! there. Folders added in the app are kept in the database.

use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use djinn_store::Store;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::library_api::start_scan;
use crate::state::AppState;

const APP_ROOTS: &str = "library_roots";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/roots", get(list).post(add).delete(remove))
        .route("/fs/dirs", get(browse))
}

/// Every library folder: config.toml's first, then the app's, each once.
pub async fn library_roots(state: &AppState) -> anyhow::Result<Vec<PathBuf>> {
    let mut roots: Vec<PathBuf> = state.library_roots.as_ref().clone();
    for root in app_roots(&state.store).await? {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    Ok(roots)
}

async fn app_roots(store: &Store) -> anyhow::Result<Vec<PathBuf>> {
    Ok(match store.get_setting(APP_ROOTS).await? {
        Some(json) => serde_json::from_str(&json)?,
        None => Vec::new(),
    })
}

async fn save_app_roots(store: &Store, roots: &[PathBuf]) -> anyhow::Result<()> {
    store
        .set_setting(APP_ROOTS, &serde_json::to_string(roots)?)
        .await?;
    Ok(())
}

#[derive(Serialize)]
struct Root {
    path: PathBuf,
    /// Set in config.toml, so it can't be removed here.
    from_config: bool,
    /// The folder can be read right now.
    available: bool,
}

async fn list(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Root>>> {
    Ok(Json(roots_view(&state).await?))
}

async fn roots_view(state: &AppState) -> anyhow::Result<Vec<Root>> {
    Ok(library_roots(state)
        .await?
        .into_iter()
        .map(|path| Root {
            from_config: state.library_roots.contains(&path),
            available: path.is_dir(),
            path,
        })
        .collect())
}

#[derive(Deserialize)]
struct RootPath {
    path: PathBuf,
}

/// Adds a folder and scans it.
async fn add(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<RootPath>,
) -> ApiResult<(StatusCode, Json<Vec<Root>>)> {
    let path = body
        .path
        .canonicalize()
        .ok()
        .filter(|p| p.is_dir())
        .ok_or_else(|| ApiError::bad_request("that folder doesn't exist or can't be read"))?;
    let mut roots = app_roots(&state.store).await?;
    if library_roots(&state).await?.contains(&path) {
        return Err(ApiError::conflict("that folder is already in the library"));
    }
    roots.push(path.clone());
    save_app_roots(&state.store, &roots).await?;
    tracing::info!(path = %path.display(), "library folder added");
    start_scan(&state);
    Ok((StatusCode::CREATED, Json(roots_view(&state).await?)))
}

/// Removes a folder added in the app; its songs leave the library.
async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<RootPath>,
) -> ApiResult<Json<Vec<Root>>> {
    if state.library_roots.contains(&body.path) {
        return Err(ApiError::bad_request(
            "this folder is set in config.toml; remove it there",
        ));
    }
    let mut roots = app_roots(&state.store).await?;
    let before = roots.len();
    roots.retain(|r| r != &body.path);
    if roots.len() == before {
        return Err(ApiError::not_found());
    }
    save_app_roots(&state.store, &roots).await?;
    tracing::info!(path = %body.path.display(), "library folder removed");
    start_scan(&state);
    Ok(Json(roots_view(&state).await?))
}

#[derive(Deserialize)]
struct BrowseQuery {
    path: Option<PathBuf>,
}

#[derive(Serialize)]
struct Folder {
    name: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct Listing {
    /// `None` at the top level (the list of drives on Windows).
    path: Option<PathBuf>,
    parent: Option<PathBuf>,
    dirs: Vec<Folder>,
    /// Handy starting points: home, Music, mounted drives.
    places: Vec<Folder>,
}

/// Lists the subfolders of `path` (default: the home folder), so the admin
/// can pick a library folder from the browser.
async fn browse(_: AdminUser, Query(query): Query<BrowseQuery>) -> ApiResult<Json<Listing>> {
    let listing = tokio::task::spawn_blocking(move || list_dirs(query.path))
        .await
        .map_err(anyhow::Error::from)??;
    Ok(Json(listing))
}

fn list_dirs(path: Option<PathBuf>) -> ApiResult<Listing> {
    let path = match path.or_else(dirs::home_dir) {
        Some(path) => path,
        None => return Ok(top_level()),
    };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| ApiError::bad_request(format!("can't open {}: {e}", path.display())))?;
    let mut dirs: Vec<Folder> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            (!name.starts_with('.')).then(|| Folder {
                name,
                path: e.path(),
            })
        })
        .collect();
    dirs.sort_by_key(|d| d.name.to_lowercase());
    Ok(Listing {
        parent: path.parent().map(Path::to_path_buf).or_else(|| {
            // On Windows, going up from a drive root shows the drives.
            cfg!(windows).then(PathBuf::new)
        }),
        path: Some(path),
        dirs,
        places: places(),
    })
}

/// Windows drive letters that exist.
fn top_level() -> Listing {
    let dirs = if cfg!(windows) {
        ('A'..='Z')
            .map(|d| PathBuf::from(format!("{d}:\\")))
            .filter(|p| p.is_dir())
            .map(|path| Folder {
                name: path.display().to_string(),
                path,
            })
            .collect()
    } else {
        vec![Folder {
            name: "/".to_owned(),
            path: PathBuf::from("/"),
        }]
    };
    Listing {
        path: None,
        parent: None,
        dirs,
        places: places(),
    }
}

fn places() -> Vec<Folder> {
    let mut places = Vec::new();
    let mut push = |name: &str, path: Option<PathBuf>| {
        if let Some(path) = path.filter(|p| p.is_dir()) {
            places.push(Folder {
                name: name.to_owned(),
                path,
            });
        }
    };
    push("Home", dirs::home_dir());
    push("Music", dirs::audio_dir());
    if cfg!(target_os = "linux") {
        // Where desktops mount USB drives and network shares.
        let user = std::env::var("USER").unwrap_or_default();
        push("Drives", Some(PathBuf::from(format!("/run/media/{user}"))));
        push("Media", Some(PathBuf::from("/media")));
        push("Mounts", Some(PathBuf::from("/mnt")));
    }
    places
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn add_and_remove_library_folders() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let dir = std::env::temp_dir().join(format!("djinn-roots-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Music")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        let canonical = dir.join("Music").canonicalize().unwrap();
        let body = serde_json::json!({ "path": dir.join("Music") }).to_string();

        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/roots",
            Some(&body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(library_roots(&state).await.unwrap(), std::slice::from_ref(&canonical));
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/roots",
            Some(&body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let missing = serde_json::json!({ "path": dir.join("nope") }).to_string();
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/roots",
            Some(&missing),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let listing = list_dirs(Some(dir.clone())).unwrap();
        let names: Vec<_> = listing.dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Music"], "hidden folders are left out");

        let remove = serde_json::json!({ "path": canonical }).to_string();
        let (status, _) = call(
            &state,
            Method::DELETE,
            "/api/library/roots",
            Some(&remove),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(library_roots(&state).await.unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
