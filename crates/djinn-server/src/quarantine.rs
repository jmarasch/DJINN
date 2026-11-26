//! Setting problem files aside (Library problems' Quarantine): every song of
//! one problem kind is moved or copied into another library folder, keeping
//! its place under its own library folder ("Artist/Album/song.mp3" stays
//! "Artist/Album/song.mp3"). Moving needs the song's own library folder to
//! be changeable; copying only reads it. Both are the editor's own move and
//! copy, so they're recorded for undo in the editor's history, need the
//! protected area unlocked and write permission where they land, and are
//! audited.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use axum::extract::{Path as RoutePath, State};
use axum::routing::post;
use axum::{Json, Router};
use djinn_store::ProblemKind;
use serde::{Deserialize, Serialize};

use crate::auth::Device;
use crate::editor_api::{relocate, AfterMove, Renamed};
use crate::editor_files::copy_files;
use crate::error::{ApiError, ApiResult};
use crate::protected::{audit, Elevated};
use crate::state::AppState;

/// The most songs one quarantine takes.
const MOST_SONGS: usize = 20_000;
/// Problem items read at a time while gathering songs.
const ITEMS_AT_ONCE: i64 = 1_000;

pub fn router() -> Router<AppState> {
    Router::new().route("/library/problems/{kind}/quarantine", post(quarantine))
}

/// Kinds whose items aren't songs in the library (folders, files DJINN
/// couldn't add, songs already gone, artist names), so there's nothing to
/// set aside.
pub fn has_songs(kind: ProblemKind) -> bool {
    use ProblemKind as K;
    !matches!(
        kind,
        K::MissingFolder
            | K::Unreadable
            | K::BadName
            | K::Unsupported
            | K::Vanished
            | K::ArtistVariants
    )
}

#[derive(Deserialize)]
struct Quarantine {
    /// The library folder they go to.
    to: String,
    /// Copy them (else move them).
    #[serde(default)]
    copy: bool,
    #[serde(default)]
    preview: bool,
}

#[derive(Serialize)]
struct Answer {
    files: Vec<Renamed>,
    /// The size of the files that would go (or went).
    bytes: u64,
    /// Space free where they go, when the system says.
    free: Option<u64>,
    batch: Option<i64>,
}

async fn quarantine(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    RoutePath(kind): RoutePath<String>,
    Json(body): Json<Quarantine>,
) -> ApiResult<Json<Answer>> {
    let kind = ProblemKind::parse(&kind).ok_or_else(ApiError::not_found)?;
    if !has_songs(kind) {
        return Err(ApiError::bad_request("these problems aren't songs"));
    }
    let roots = crate::folders::library_roots(&state).await?;
    let to_root = PathBuf::from(body.to.trim());
    if !roots.contains(&to_root) {
        return Err(ApiError::bad_request("choose one of the library folders"));
    }
    let ids = songs_of(&state, kind).await?;
    if ids.len() > MOST_SONGS {
        return Err(ApiError::bad_request(format!(
            "that's over {MOST_SONGS} songs; fix some another way first"
        )));
    }
    let songs = state.store.editor_songs_by_id(&ids).await?;
    let editable = crate::editable::editable_roots(&state).await?;
    let mut taken = HashSet::new();
    let mut files: Vec<Renamed> = songs
        .iter()
        .filter_map(|song| {
            let from = PathBuf::from(&song.path);
            let (root, to) = place(&roots, &to_root, &from)?;
            let to_text = to.to_string_lossy().into_owned();
            let problem = if root == to_root {
                Some("it's in that library folder already".to_owned())
            } else if !body.copy && !editable.contains(&root) {
                Some("its library folder is read only, so it can only be copied".to_owned())
            } else if !taken.insert(to_text.to_lowercase()) {
                Some("another song would get the same place".to_owned())
            } else if to.exists() {
                Some("a file with that name is there already".to_owned())
            } else {
                None
            };
            Some(Renamed {
                id: song.id,
                from: song.path.clone(),
                to: to_text,
                problem,
            })
        })
        .collect();
    let bytes = sizes(&files).await;
    let free = djinn_platform::system::disk(&to_root).map(|d| d.available);
    if body.preview {
        return Ok(Json(Answer {
            files,
            bytes,
            free,
            batch: None,
        }));
    }
    if body.copy && free.is_some_and(|free| bytes > free) {
        return Err(ApiError::bad_request(
            "there isn't room for them in that library folder",
        ));
    }
    let (batch, done) = if body.copy {
        copy_files(&state, &user, &mut files).await?
    } else {
        // The folders they leave empty go to the trash too.
        let after = AfterMove {
            remove_empty: true,
            folder_art: false,
        };
        relocate(&state, &user, &mut files, "rename", after).await?
    };
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        if body.copy {
            "quarantine_copy"
        } else {
            "quarantine_move"
        },
        &format!(
            "{done} files ({}) to {}, approval {batch}",
            kind.as_str(),
            to_root.display()
        ),
    )
    .await;
    Ok(Json(Answer {
        files,
        bytes,
        free,
        batch: (done > 0).then_some(batch),
    }))
}

/// Every song a problem kind lists: its songs, and every song of the albums
/// it lists.
async fn songs_of(state: &AppState, kind: ProblemKind) -> ApiResult<Vec<i64>> {
    let mut songs = Vec::new();
    let mut albums = Vec::new();
    let mut offset = 0;
    loop {
        let items = state.store.problems(kind, offset, ITEMS_AT_ONCE).await?;
        for item in &items {
            match (item.track_id, item.album_id) {
                (Some(song), _) => songs.push(song),
                (None, Some(album)) => albums.push(album),
                (None, None) => {}
            }
        }
        if (items.len() as i64) < ITEMS_AT_ONCE || songs.len() > MOST_SONGS {
            break;
        }
        offset += ITEMS_AT_ONCE;
    }
    if !albums.is_empty() {
        songs.extend(state.store.album_song_ids(&albums).await?);
    }
    songs.sort_unstable();
    songs.dedup();
    Ok(songs)
}

/// The library folder a file is in (the deepest, when one is inside
/// another) and where it goes under `to_root`, keeping its place.
fn place(roots: &[PathBuf], to_root: &Path, file: &Path) -> Option<(PathBuf, PathBuf)> {
    let root = roots
        .iter()
        .filter(|r| file.starts_with(r))
        .max_by_key(|r| r.components().count())?;
    let rel = file.strip_prefix(root).ok()?;
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some((root.clone(), to_root.join(rel)))
}

/// The size of the files that can go.
async fn sizes(files: &[Renamed]) -> u64 {
    let paths: Vec<String> = files
        .iter()
        .filter(|f| f.problem.is_none())
        .map(|f| f.from.clone())
        .collect();
    tokio::task::spawn_blocking(move || {
        paths
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    })
    .await
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_keeps_its_place_under_its_library_folder() {
        let roots = [
            PathBuf::from("/music/main"),
            PathBuf::from("/music/ready"),
            PathBuf::from("/music/main/inner"),
        ];
        let (root, to) = place(
            &roots,
            Path::new("/music/ready"),
            Path::new("/music/main/Band/Album/01 Song.mp3"),
        )
        .unwrap();
        assert_eq!(root, PathBuf::from("/music/main"));
        assert_eq!(to, PathBuf::from("/music/ready/Band/Album/01 Song.mp3"));
        // A library folder inside another counts as its own.
        let (root, to) = place(
            &roots,
            Path::new("/music/ready"),
            Path::new("/music/main/inner/Band/Song.mp3"),
        )
        .unwrap();
        assert_eq!(root, PathBuf::from("/music/main/inner"));
        assert_eq!(to, PathBuf::from("/music/ready/Band/Song.mp3"));
        // Nothing outside the library, and no way out of it.
        assert!(place(
            &roots,
            Path::new("/music/ready"),
            Path::new("/elsewhere/a.mp3")
        )
        .is_none());
        assert!(place(
            &roots,
            Path::new("/music/ready"),
            Path::new("/music/main/../../etc/passwd")
        )
        .is_none());
    }

    async fn send(
        state: &AppState,
        path: &str,
        body: &str,
        cookie: &str,
    ) -> (axum::http::StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_owned()))
            .unwrap();
        let res = crate::app(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn problem_songs_are_copied_or_moved_keeping_their_folders() {
        let state = AppState::for_tests().await;
        let admin = crate::test_util::session(&state, "admin", djinn_store::Role::Admin).await;
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let base = std::env::temp_dir().join(format!("djinn-quarantine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let [main, ready, other] = ["main", "ready", "other"].map(|n| base.join(n));
        for root in [&main, &ready, &other] {
            std::fs::create_dir_all(root).unwrap();
            crate::folders::add_root_for_tests(&state, root).await;
        }
        // Someone else looks after the main folder: only the others may change.
        let allow = |roots: &[&PathBuf]| {
            roots
                .iter()
                .map(|r| r.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n")
        };
        state
            .store
            .set_setting(crate::editable::KEY, &allow(&[&ready, &other]))
            .await
            .unwrap();
        let song = main.join("Band").join("Record").join("01 Song.flac");
        std::fs::create_dir_all(song.parent().unwrap()).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-engine/tests/fixtures/gapless_a.flac"),
            &song,
        )
        .unwrap();
        // No album tag: a "no album" problem.
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: song.to_string_lossy().into_owned(),
                title: "Song".into(),
                artist: Some("Band".into()),
                ..Default::default()
            }])
            .await
            .unwrap();
        let url = "/api/library/problems/no_album/quarantine";
        let ask = |to: &Path, copy: bool, preview: bool| {
            format!(
                r#"{{"to":{},"copy":{copy},"preview":{preview}}}"#,
                serde_json::Value::from(to.to_string_lossy().into_owned())
            )
        };

        // A read only folder can't be moved out of.
        let (status, plan) = send(&state, url, &ask(&ready, false, true), &admin).await;
        assert_eq!(status, 200, "{plan}");
        assert!(plan["files"][0]["problem"]
            .as_str()
            .unwrap()
            .contains("read only"));

        // Copied, keeping Band/Record, with its size and the room there.
        let (status, plan) = send(&state, url, &ask(&ready, true, true), &admin).await;
        assert_eq!(status, 200, "{plan}");
        assert_eq!(plan["bytes"], std::fs::metadata(&song).unwrap().len());
        assert!(plan["files"][0]["problem"].is_null());
        let (status, done) = send(&state, url, &ask(&ready, true, false), &admin).await;
        assert_eq!(status, 200, "{done}");
        assert!(ready.join("Band/Record/01 Song.flac").is_file());
        assert!(song.is_file());

        // Once main may change, the songs move: the original, and the copy
        // (now a song of its own with the same problem) from the ready folder.
        state
            .store
            .set_setting(crate::editable::KEY, &allow(&[&main, &ready, &other]))
            .await
            .unwrap();
        let (status, done) = send(&state, url, &ask(&other, false, false), &admin).await;
        assert_eq!(status, 200, "{done}");
        assert!(other.join("Band/Record/01 Song.flac").is_file());
        assert!(!song.exists());
        // Both wanted the same place: one went, the other stayed.
        let stuck: Vec<&str> = done["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f["problem"].as_str())
            .collect();
        assert_eq!(stuck, ["another song would get the same place"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_kinds_with_songs_can_be_set_aside() {
        assert!(has_songs(ProblemKind::NoLengthHeader));
        assert!(has_songs(ProblemKind::AlbumNoCover));
        assert!(!has_songs(ProblemKind::Vanished));
        assert!(!has_songs(ProblemKind::MissingFolder));
    }
}
