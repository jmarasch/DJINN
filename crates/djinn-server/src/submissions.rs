//! Submitting music from DJINN GO.
//!
//! An upload is saved to `<db dir>/uploads/`, checked (it must be an audio
//! file DJINN can decode, under the size limit), and its tags read. It's
//! flagged as a duplicate when the same file was sent before or the library
//! has a song with the same title, artist and length. Then it waits for an
//! admin, or goes straight in when the uploader's role allows and it isn't
//! a duplicate. Approved files move into the uploads library folder (as
//! Artist/Album/file) and the library is checked; turned-down ones are
//! deleted. Other music folders are never written to.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use djinn_store::{NewSubmission, Submission, SubmissionStatus, User};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::AdminUser;
use crate::backup::data_dir;
use crate::error::{ApiError, ApiResult};
use crate::go_library::GoUser;
use crate::state::AppState;
use crate::{folders, library_api, settings};

/// Where approved uploads go; empty for `<db dir>/Uploaded music`.
pub const UPLOAD_FOLDER: &str = "upload_folder";
/// Largest upload, in megabytes.
pub const UPLOAD_MAX_MB: &str = "upload_max_mb";
/// Room for a long song as a high-quality MP3 (about 12 MB for five minutes
/// at 320 kbps); admins raise it for lossless files.
const DEFAULT_MAX_MB: i64 = 15;
/// Uploads one person can have waiting at once.
const MAX_WAITING: i64 = 200;
const HOLDING_DIR: &str = "uploads";
/// What DJINN can decode (djinn-engine's symphonia features).
const EXTENSIONS: &[&str] = &["mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "wav"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/go/submissions",
            get(mine)
                .post(upload)
                // Songs can be big; the size setting limits them instead.
                .layer(DefaultBodyLimit::disable()),
        )
        .route("/go/submissions/{id}", delete(withdraw))
        .route("/submissions", get(waiting))
        .route("/submissions/settings", get(get_settings).put(put_settings))
        .route("/submissions/{id}/file", get(listen))
        .route("/submissions/{id}/approve", post(approve_one))
        .route("/submissions/{id}/decline", post(decline_one))
}

fn holding_dir(state: &AppState) -> PathBuf {
    data_dir(state).join(HOLDING_DIR)
}

/// The library folder approved uploads go to.
pub async fn upload_folder(state: &AppState) -> anyhow::Result<PathBuf> {
    Ok(match state.store.get_setting(UPLOAD_FOLDER).await? {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path.trim()),
        _ => data_dir(state).join("Uploaded music"),
    })
}

async fn max_bytes(state: &AppState) -> anyhow::Result<u64> {
    let mb = settings::number(&state.store, UPLOAD_MAX_MB, DEFAULT_MAX_MB).await?;
    Ok(u64::try_from(mb.clamp(1, 4096)).unwrap_or(DEFAULT_MAX_MB as u64) * 1024 * 1024)
}

#[derive(Deserialize)]
struct UploadQuery {
    /// The file's name, for its extension and the name it keeps.
    name: String,
}

/// Receives one song file as the request body.
async fn upload(
    me: GoUser,
    State(state): State<AppState>,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> ApiResult<(StatusCode, Json<Submission>)> {
    let permissions = state.store.permissions(&me.user).await?;
    if !permissions.submit_music {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "your account can't submit music",
        ));
    }
    if state.store.waiting_count(me.user.id).await? >= MAX_WAITING {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "you have a lot waiting for review already; try again once some are through",
        ));
    }
    // Only the name, not any folders a browser sent with it.
    let base = query.name.rsplit(['/', '\\']).next().unwrap_or(&query.name);
    let file_name = clean_name(base, "song");
    let ext = Path::new(&file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(ApiError::bad_request(
            "send MP3, FLAC, M4A (AAC), Ogg Vorbis or WAV files",
        ));
    }
    let dir = holding_dir(&state);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(anyhow::Error::from)?;
    let stored = format!("{}.{ext}", crate::auth::new_token()?);
    let path = dir.join(&stored);
    let limit = max_bytes(&state).await?;
    let (size, sha) = match save_limited(body, &path, limit).await {
        Ok(saved) => saved,
        Err(e) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(e);
        }
    };

    // Decode it (fingerprinting reads the audio) and read its tags.
    let checked = tokio::task::spawn_blocking({
        let path = path.clone();
        move || {
            let decodes = djinn_engine::fingerprint(&path).is_ok();
            (decodes, djinn_library::basics(&path))
        }
    })
    .await
    .map_err(anyhow::Error::from)?;
    let basics = match checked {
        (true, Some(basics)) => basics,
        _ => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(ApiError::bad_request(
                "that file isn't audio DJINN can play",
            ));
        }
    };
    if let Some(earlier) = state.store.submission_with_hash(&sha).await? {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(ApiError::conflict(format!(
            "that exact file was sent already (upload {earlier})"
        )));
    }
    let duplicate_of = match (&basics.title, &basics.artist) {
        (Some(title), Some(artist)) => {
            state
                .store
                .same_song(title, artist, basics.duration_ms)
                .await?
        }
        _ => None,
    };
    let id = state
        .store
        .add_submission(&NewSubmission {
            user_id: me.user.id,
            file_name: &file_name,
            stored_name: &stored,
            size: i64::try_from(size).unwrap_or(i64::MAX),
            sha256: &sha,
            title: basics.title.as_deref(),
            artist: basics.artist.as_deref(),
            album: basics.album.as_deref(),
            duration_ms: basics.duration_ms,
            duplicate_of,
        })
        .await?;
    tracing::info!(user = %me.user.username, file = %file_name, "music submitted");
    if permissions.submit_without_review && duplicate_of.is_none() {
        approve(&state, id, None).await?;
    }
    let submission = state
        .store
        .submission(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(submission)))
}

/// Writes the body to `path`, stopping at `limit` bytes. Returns the size
/// and SHA-256.
async fn save_limited(body: Body, path: &Path, limit: u64) -> ApiResult<(u64, Vec<u8>)> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(anyhow::Error::from)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| ApiError::bad_request(format!("the upload broke off: {e}")))?;
        size += chunk.len() as u64;
        if size > limit {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("that file is over {} MB", limit / 1024 / 1024),
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(anyhow::Error::from)?;
    }
    file.flush().await.map_err(anyhow::Error::from)?;
    if size == 0 {
        return Err(ApiError::bad_request("that file is empty"));
    }
    Ok((size, hasher.finalize().to_vec()))
}

/// A file or folder name safe on every system: no separators or reserved
/// characters, not just dots, not too long.
fn clean_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let short: String = trimmed.chars().take(120).collect();
    let reserved = matches!(
        short
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
            .as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "LPT1" | "LPT2" | "LPT3"
    );
    if short.is_empty() || reserved {
        fallback.to_owned()
    } else {
        short
    }
}

/// Moves an upload into the library and checks the library. `admin` is
/// who approved it; None when the role let it straight in.
async fn approve(state: &AppState, id: i64, admin: Option<&User>) -> ApiResult<PathBuf> {
    let submission = state
        .store
        .submission(id)
        .await?
        .filter(|s| s.status == SubmissionStatus::Waiting)
        .ok_or_else(ApiError::not_found)?;
    let folder = upload_folder(state).await?;
    let artist = clean_name(
        submission.artist.as_deref().unwrap_or("Unknown artist"),
        "Unknown artist",
    );
    let album = clean_name(submission.album.as_deref().unwrap_or("Singles"), "Singles");
    let dir = folder.join(artist).join(album);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| anyhow::anyhow!("couldn't make {}: {e}", dir.display()))?;
    let target = free_name(&dir, &submission.file_name);
    let from = holding_dir(state).join(&submission.stored_name);
    if tokio::fs::rename(&from, &target).await.is_err() {
        // Another drive: copy, then remove.
        tokio::fs::copy(&from, &target)
            .await
            .map_err(|e| anyhow::anyhow!("couldn't move the upload: {e}"))?;
        let _ = tokio::fs::remove_file(&from).await;
    }
    let path = target.to_string_lossy().into_owned();
    state
        .store
        .decide_submission(
            id,
            SubmissionStatus::Approved,
            admin.map(|a| a.id),
            None,
            Some(&path),
        )
        .await?;
    folders::ensure_root(state, &folder).await?;
    library_api::start_scan(state);
    tracing::info!(file = %path, by = ?admin.map(|a| &a.username), "upload added to the library");
    Ok(target)
}

/// `name` in `dir`, or "name (2)" and so on if it's taken.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "song".to_owned());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

async fn mine(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Submission>>> {
    Ok(Json(state.store.user_submissions(me.user.id).await?))
}

/// Takes back an upload still waiting.
async fn withdraw(
    me: GoUser,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
) -> ApiResult<StatusCode> {
    let stored = state
        .store
        .withdraw_submission(me.user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let _ = tokio::fs::remove_file(holding_dir(&state).join(stored)).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn waiting(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Submission>>> {
    Ok(Json(state.store.waiting_submissions().await?))
}

/// Plays a waiting upload for the admin reviewing it.
async fn listen(
    _: AdminUser,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    request: Request,
) -> ApiResult<Response> {
    let submission = state
        .store
        .submission(id)
        .await?
        .filter(|s| s.status == SubmissionStatus::Waiting)
        .ok_or_else(ApiError::not_found)?;
    let path = holding_dir(&state).join(submission.stored_name);
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => Ok(response.map(Body::new).into_response()),
        Err(e) => match e {},
    }
}

async fn approve_one(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
) -> ApiResult<StatusCode> {
    approve(&state, id, Some(&admin)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
struct Decline {
    /// Why, for the uploader.
    #[serde(default)]
    note: String,
}

async fn decline_one(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    Json(body): Json<Decline>,
) -> ApiResult<StatusCode> {
    let submission = state
        .store
        .submission(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let note: String = body.note.trim().chars().take(300).collect();
    if !state
        .store
        .decide_submission(
            id,
            SubmissionStatus::Declined,
            Some(admin.id),
            Some(&note).filter(|n| !n.is_empty()).map(String::as_str),
            None,
        )
        .await?
    {
        return Err(ApiError::not_found());
    }
    let _ = tokio::fs::remove_file(holding_dir(&state).join(submission.stored_name)).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
struct UploadSettings {
    /// Where approved uploads go.
    folder: String,
    max_mb: i64,
}

async fn settings_view(state: &AppState) -> anyhow::Result<UploadSettings> {
    Ok(UploadSettings {
        folder: upload_folder(state).await?.to_string_lossy().into_owned(),
        max_mb: settings::number(&state.store, UPLOAD_MAX_MB, DEFAULT_MAX_MB).await?,
    })
}

async fn get_settings(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<UploadSettings>> {
    Ok(Json(settings_view(&state).await?))
}

async fn put_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UploadSettings>,
) -> ApiResult<Json<UploadSettings>> {
    if !(1..=4096).contains(&body.max_mb) {
        return Err(ApiError::bad_request("the size limit must be 1 to 4096 MB"));
    }
    let folder = body.folder.trim();
    if !folder.is_empty() && !Path::new(folder).is_absolute() {
        return Err(ApiError::bad_request(
            "give the full path of a folder, like D:\\Music\\Uploads",
        ));
    }
    state.store.set_setting(UPLOAD_FOLDER, folder).await?;
    state
        .store
        .set_setting(UPLOAD_MAX_MB, &body.max_mb.to_string())
        .await?;
    Ok(Json(settings_view(&state).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(clean_name("..\\evil.mp3", "x"), "_evil.mp3");
        assert_eq!(clean_name("b:c?.flac", "x"), "b_c_.flac");
        assert_eq!(clean_name("  ...  ", "Singles"), "Singles");
        assert_eq!(clean_name("CON.mp3", "song"), "song");
        assert_eq!(clean_name("AC/DC", "Unknown"), "AC_DC");
        assert_eq!(clean_name("Sigur Rós", "x"), "Sigur Rós");
    }

    /// Sends `body` to the GO port as the session in `cookie`.
    async fn go_send(
        state: &AppState,
        method: axum::http::Method,
        path: &str,
        body: Vec<u8>,
        cookie: &str,
    ) -> (StatusCode, String) {
        use axum::http::header;
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = crate::go::router(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn uploads_are_checked_then_reviewed_or_let_in() {
        use axum::http::Method;
        use djinn_store::{Permissions, Role};

        let dir = std::env::temp_dir().join(format!("djinn-uploads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("art")).unwrap();
        let state = AppState::for_tests_in(&dir).await;
        state.go.enable_for_tests();
        let song = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-library/tests/fixtures/tagged.flac"),
        )
        .unwrap();

        // A role that can upload straight into the library (and use GO away
        // from home, which tests count as).
        let trusted = state
            .store
            .create_role(
                "Trusted",
                &Permissions {
                    go: true,
                    go_remote: true,
                    submit_music: true,
                    submit_without_review: true,
                    ..Permissions::default()
                },
            )
            .await
            .unwrap();
        let sam = crate::test_util::session(&state, "sam", Role::User).await;
        let sam_id = state
            .store
            .user_credentials("sam")
            .await
            .unwrap()
            .unwrap()
            .0
            .id;
        state
            .store
            .set_account_role(sam_id, trusted.id)
            .await
            .unwrap();

        let (status, text) = go_send(
            &state,
            Method::POST,
            "/api/go/submissions?name=..%2Fmy%20song.flac",
            song.clone(),
            &sam,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
        assert!(text.contains("\"status\":\"approved\""), "{text}");
        let folder = upload_folder(&state).await.unwrap();
        let placed: Vec<_> = walk(&folder);
        assert_eq!(placed.len(), 1, "{placed:?}");
        assert!(placed[0].ends_with("my song.flac"), "{placed:?}");
        assert!(folders::library_roots(&state)
            .await
            .unwrap()
            .iter()
            .any(|r| folder.starts_with(r) || r.ends_with("Uploaded music")));

        // The same file again is refused; so is something that isn't audio.
        let (status, _) = go_send(
            &state,
            Method::POST,
            "/api/go/submissions?name=again.flac",
            song.clone(),
            &sam,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = go_send(
            &state,
            Method::POST,
            "/api/go/submissions?name=fake.mp3",
            b"not music at all".to_vec(),
            &sam,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = go_send(
            &state,
            Method::POST,
            "/api/go/submissions?name=notes.txt",
            b"hello".to_vec(),
            &sam,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // With review: it waits, then the admin turns it down and it's gone.
        let reviewed = Permissions {
            submit_without_review: false,
            ..trusted.permissions.clone()
        };
        state
            .store
            .update_role(trusted.id, "Trusted", &reviewed)
            .await
            .unwrap();
        let mut changed = song.clone();
        changed.extend_from_slice(b"padding so it hashes differently");
        let (status, text) = go_send(
            &state,
            Method::POST,
            "/api/go/submissions?name=second.flac",
            changed,
            &sam,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
        assert!(text.contains("\"status\":\"waiting\""), "{text}");
        let waiting = state.store.waiting_submissions().await.unwrap();
        assert_eq!(waiting.len(), 1);
        let admin = crate::test_util::session(&state, "ana", Role::Admin).await;
        let (status, _) = crate::test_util::call(
            &state,
            Method::POST,
            &format!("/api/submissions/{}/decline", waiting[0].id),
            Some(r#"{"note":"have it already"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!holding_dir(&state).join(&waiting[0].stored_name).exists());
        let (_, mine) = go_send(&state, Method::GET, "/api/go/submissions", Vec::new(), &sam).await;
        assert!(mine.contains("have it already"), "{mine}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    found.extend(walk(&path));
                } else {
                    found.push(path);
                }
            }
        }
        found
    }
}
