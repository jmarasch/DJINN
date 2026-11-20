//! Which library folders may be changed. Each music folder DJINN reads is
//! view only until an admin allows changes in it, so one folder (say, where
//! new music lands to be tidied) can be edited while another (one that
//! someone else looks after) can only be watched. Every route that writes
//! to the music folders must call [`require`] on the file it touches, after
//! taking `Elevated`. Choosing is a protected action and is audited.

use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, Device};
use crate::error::{ApiError, ApiResult};
use crate::protected::{audit, Elevated};
use crate::state::AppState;

/// Setting key: the library folders that may be changed, one path per line.
pub const KEY: &str = "editable_folders";

pub fn router() -> Router<AppState> {
    Router::new().route("/library/editable", get(list).put(set))
}

async fn chosen(state: &AppState) -> anyhow::Result<Vec<PathBuf>> {
    Ok(state
        .store
        .get_setting(KEY)
        .await?
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// The library folders that may be changed now: those chosen that are still
/// library folders.
pub async fn editable_roots(state: &AppState) -> anyhow::Result<Vec<PathBuf>> {
    let roots = crate::folders::library_roots(state).await?;
    Ok(chosen(state)
        .await?
        .into_iter()
        .filter(|c| roots.contains(c))
        .collect())
}

/// Whether a file may be changed: it sits under an editable library folder.
/// A path with `..` in it never does, since "/music/../elsewhere" starts with
/// "/music" piece by piece but isn't inside it.
pub async fn is_editable(state: &AppState, file: &Path) -> anyhow::Result<bool> {
    if file
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Ok(false);
    }
    Ok(editable_roots(state)
        .await?
        .iter()
        .any(|root| file.starts_with(root)))
}

/// Refuses a change to a file in a view-only folder.
pub async fn require(state: &AppState, file: &Path) -> ApiResult<()> {
    if is_editable(state, file).await? {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "that folder is view only; allow changes in it under Library editor first",
        ))
    }
}

#[derive(Serialize)]
struct Folder {
    path: String,
    name: String,
    editable: bool,
}

async fn list(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Folder>>> {
    let editable = editable_roots(&state).await?;
    let roots = crate::folders::library_roots(&state).await?;
    Ok(Json(
        roots
            .into_iter()
            .map(|root| Folder {
                name: root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.to_string_lossy().into_owned()),
                editable: editable.contains(&root),
                path: root.to_string_lossy().into_owned(),
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct Choice {
    path: String,
    editable: bool,
}

/// Allows or stops changes in one library folder. Allowing checks that DJINN
/// can write there at all (a read-only mount, a share without rights) and
/// says why not.
async fn set(
    Elevated(admin): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Choice>,
) -> ApiResult<StatusCode> {
    let root = PathBuf::from(&body.path);
    let roots = crate::folders::library_roots(&state).await?;
    if !roots.contains(&root) {
        return Err(ApiError::bad_request(
            "that isn't one of your library folders",
        ));
    }
    let mut now = chosen(&state).await?;
    now.retain(|p| p != &root);
    if body.editable {
        check_writable(&root).await?;
        now.push(root.clone());
    }
    let text: Vec<String> = now
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    state.store.set_setting(KEY, &text.join("\n")).await?;
    audit(
        &state,
        &admin,
        device.ip.as_deref(),
        if body.editable {
            "folder_editable"
        } else {
            "folder_view_only"
        },
        &body.path,
    )
    .await;
    tracing::warn!(folder = %body.path, editable = body.editable, by = %admin.username, "library folder changes setting");
    Ok(StatusCode::NO_CONTENT)
}

/// Makes and removes a tiny file in the folder, so a read-only mount or a
/// share without rights is said so now rather than when a change fails.
async fn check_writable(root: &Path) -> ApiResult<()> {
    let probe = root.join(format!(".djinn-write-check-{}", std::process::id()));
    let made = tokio::fs::write(&probe, b"").await;
    let _ = tokio::fs::remove_file(&probe).await;
    made.map_err(|e| {
        ApiError::bad_request(format!(
            "DJINN can't write in that folder ({e}). Check the folder's rights, or that it isn't mounted read-only"
        ))
    })
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use djinn_store::Role;

    use super::*;
    use crate::test_util::{call, get_body, session};

    #[tokio::test]
    async fn folders_are_view_only_until_allowed_and_the_choice_is_locked() {
        let state = AppState::for_tests().await;
        let pool = std::env::temp_dir().join(format!("djinn-editable-pool-{}", std::process::id()));
        let quar = std::env::temp_dir().join(format!("djinn-editable-quar-{}", std::process::id()));
        for dir in [&pool, &quar] {
            std::fs::create_dir_all(dir).unwrap();
            crate::folders::add_root_for_tests(&state, dir).await;
        }
        let admin = session(&state, "admin", Role::Admin).await;
        // Nothing is editable at first.
        let (status, body) = get_body(&state, "/api/library/editable", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("\"editable\":false") && !body.contains("\"editable\":true"),
            "{body}"
        );
        let song = quar.join("a.mp3");
        assert!(require(&state, &song).await.is_err());
        // The choice itself is a protected action.
        let put = |body: String| {
            let state = state.clone();
            let who = admin.clone();
            async move {
                call(
                    &state,
                    Method::PUT,
                    "/api/library/editable",
                    Some(&body),
                    Some(&who),
                )
                .await
                .0
            }
        };
        let quar_json = format!(r#"{{"path":"{}","editable":true}}"#, quar.to_string_lossy());
        assert_eq!(put(quar_json.clone()).await, StatusCode::LOCKED);
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        assert_eq!(put(quar_json).await, StatusCode::NO_CONTENT);
        // The quarantine folder may be changed; the pool may not.
        assert!(require(&state, &song).await.is_ok());
        assert!(require(&state, &pool.join("b.mp3")).await.is_err());
        // Climbing out of the allowed folder doesn't count as inside it.
        let parent = song.parent().unwrap();
        let outside = parent.join("..").join("elsewhere.mp3");
        assert!(require(&state, &outside).await.is_err());
        // Not a library folder: refused.
        assert_eq!(
            put(r#"{"path":"/nowhere","editable":true}"#.into()).await,
            StatusCode::BAD_REQUEST
        );
        // Taking it back.
        let back = format!(
            r#"{{"path":"{}","editable":false}}"#,
            quar.to_string_lossy()
        );
        assert_eq!(put(back).await, StatusCode::NO_CONTENT);
        assert!(require(&state, &song).await.is_err());
        for dir in [pool, quar] {
            std::fs::remove_dir_all(dir).ok();
        }
    }
}
