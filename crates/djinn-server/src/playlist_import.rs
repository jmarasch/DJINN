//! Importing playlists from other services, and keeping every playlist
//! honest about songs that can't play. An import brings names, never music:
//! what the library has joins the list, the rest stays flagged in its place
//! until a library check finds it. Files that won't play are noticed when
//! they fail, by a check an admin starts, and are tried again after each
//! library check.

use std::collections::HashSet;
use std::time::UNIX_EPOCH;

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::playlist_file::{self, ImportedList, ParsedImport};
use djinn_library::playlist_match::SongIndex;
use djinn_store::{ImportRow, SongFile, Store, WantedSong};
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::playlist_api::{validate_name, Member, MAX_NAME};
use crate::state::AppState;
use crate::youtube;

/// Big enough for an Apple Music library export.
const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_SONGS: usize = 20_000;
/// Songs tested at once by a playlist check.
const CHECKS: usize = 2;
const NAME_FALLBACK: &str = "Imported playlist";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/playlists/import",
            post(import).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
        .route(
            "/playlists/youtube-key",
            get(youtube_key_status).put(set_youtube_key),
        )
        .route("/playlists/check", get(check_status))
        .route("/playlists/{id}/check", post(start_check))
}

#[derive(Deserialize)]
struct ImportBody {
    /// What to call it, when the import is one playlist.
    name: Option<String>,
    /// The file it came from: its extension tells some formats apart, and
    /// its name is the fallback name.
    filename: Option<String>,
    text: String,
}

#[derive(Serialize)]
struct ImportedPlaylist {
    id: i64,
    name: String,
    found: usize,
    missing: usize,
}

#[derive(Serialize)]
struct ImportResult {
    playlists: Vec<ImportedPlaylist>,
    /// Lines that didn't name a song.
    skipped: usize,
}

async fn import(
    Member(user): Member,
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> ApiResult<(StatusCode, Json<ImportResult>)> {
    let parsed = match youtube::playlist_id(&body.text) {
        Some(list) => from_youtube(&state, &list).await?,
        None => playlist_file::parse(&body.text, body.filename.as_deref())
            .map_err(|e| ApiError::bad_request(e.to_string()))?,
    };
    if parsed.lists.iter().map(|l| l.songs.len()).sum::<usize>() > MAX_SONGS {
        return Err(ApiError::bad_request(format!(
            "that's more than {MAX_SONGS} songs"
        )));
    }
    // A bare video id names no song, and finding out which would mean telling
    // YouTube about each one, which DJINN doesn't do.
    let mut skipped = parsed.skipped;
    let mut lists = Vec::with_capacity(parsed.lists.len());
    for list in parsed.lists {
        let total = list.songs.len();
        let songs: Vec<WantedSong> = list
            .songs
            .iter()
            .filter(|s| s.video_id.is_none())
            .map(|s| s.song.clone())
            .collect();
        skipped += total - songs.len();
        if !songs.is_empty() {
            lists.push((list, songs));
        }
    }
    if lists.is_empty() {
        return Err(ApiError::bad_request(
            "that file lists only video ids, not song names. Paste the playlist's YouTube link \
             instead, or export it to a file with song names",
        ));
    }
    let count = lists.len();
    let (names, wanted): (Vec<String>, Vec<Vec<WantedSong>>) = lists
        .into_iter()
        .map(|(list, songs)| (list_name(&list, count, &body), songs))
        .unzip();
    let index = library_index(&state.store).await?;

    let mut playlists = Vec::with_capacity(wanted.len());
    for (songs, name) in wanted.into_iter().zip(names) {
        let rows = match_rows(&index, songs);
        let found: HashSet<i64> = rows
            .iter()
            .filter_map(|row| match row {
                ImportRow::Track(id) => Some(*id),
                ImportRow::Wanted(_) => None,
            })
            .collect();
        let found = found.len();
        let missing = rows
            .iter()
            .filter(|row| matches!(row, ImportRow::Wanted(_)))
            .count();
        let playlist = state.store.import_playlist(user.id, &name, &rows).await?;
        playlists.push(ImportedPlaylist {
            id: playlist.id,
            name: playlist.name,
            found,
            missing,
        });
    }
    Ok((
        StatusCode::CREATED,
        Json(ImportResult { playlists, skipped }),
    ))
}

fn list_name(list: &ImportedList, lists: usize, body: &ImportBody) -> String {
    let typed = body.name.as_deref().filter(|_| lists == 1);
    let from_file = body
        .filename
        .as_deref()
        .map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f))
        .map(|f| f.rsplit_once('.').map_or(f, |(stem, _)| stem))
        .filter(|_| lists == 1);
    let candidates = [typed, list.name.as_deref(), from_file];
    candidates
        .into_iter()
        .flatten()
        .find_map(|name| {
            let name: String = name
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_NAME)
                .collect();
            validate_name(&name).ok()
        })
        .unwrap_or_else(|| NAME_FALLBACK.to_owned())
}

/// A song listed twice holds its first place, found or not.
fn match_rows(index: &SongIndex, songs: Vec<WantedSong>) -> Vec<ImportRow> {
    let mut seen = HashSet::new();
    songs
        .into_iter()
        .filter_map(|song| match index.find(&song) {
            Some(id) => Some(ImportRow::Track(id)),
            None => {
                let name = (
                    song.title.to_lowercase(),
                    song.artist.as_deref().map(str::to_lowercase),
                );
                seen.insert(name).then_some(ImportRow::Wanted(song))
            }
        })
        .collect()
}

async fn library_index(store: &Store) -> anyhow::Result<SongIndex> {
    let facts = store.song_facts().await?;
    Ok(tokio::task::spawn_blocking(move || SongIndex::new(facts)).await?)
}

/// Reads a YouTube playlist, if an admin has added a key. Without one nothing
/// is sent anywhere.
async fn from_youtube(state: &AppState, list: &str) -> ApiResult<ParsedImport> {
    let key = state.store.get_setting(youtube::API_KEY).await?;
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        return Err(ApiError::bad_request(
            "reading YouTube links needs an API key, which an admin can add under Admin > \
             Library. Or export the playlist to a file",
        ));
    };
    youtube::read_playlist(youtube::API, &key, list, MAX_SONGS)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

#[derive(Serialize)]
struct KeyStatus {
    /// The key itself never leaves the server.
    has_key: bool,
}

/// Whether links work, for anyone who can import, on the jukebox page and GO.
async fn youtube_key_status(
    _: Member,
    State(state): State<AppState>,
) -> ApiResult<Json<KeyStatus>> {
    let key = state.store.get_setting(youtube::API_KEY).await?;
    Ok(Json(KeyStatus {
        has_key: key.is_some_and(|k| !k.is_empty()),
    }))
}

#[derive(Deserialize)]
struct NewKey {
    key: String,
}

/// Saves the key, or takes it away when empty.
async fn set_youtube_key(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NewKey>,
) -> ApiResult<StatusCode> {
    let key = body.key.trim();
    let fits = key.len() <= 200 && key.chars().all(|c| c.is_ascii_graphic());
    if !fits {
        return Err(ApiError::bad_request("that doesn't look like an API key"));
    }
    state.store.set_setting(youtube::API_KEY, key).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What to do about a song that failed. A file that's there gets marked,
/// with its size and date so a replacement is noticed. One that's gone from
/// a folder that's still there is removed, as a scan would. If its folder is
/// gone too it could just be an unplugged drive, so that's for a library
/// check to decide, since it knows which folders are available.
pub async fn note_failure(store: &Store, file: &SongFile, error: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(&file.path);
    match tokio::fs::metadata(path).await {
        Ok(meta) => {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok());
            let size = i64::try_from(meta.len()).ok();
            store
                .mark_track_problem(file.id, error, size, mtime)
                .await?;
            tracing::warn!(track = file.id, path = %file.path, %error, "marked as not playing");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let folder_there = match path.parent() {
                Some(parent) => tokio::fs::metadata(parent).await.is_ok(),
                None => false,
            };
            if folder_there {
                store.mark_removed(&[file.id]).await?;
                tracing::warn!(track = file.id, path = %file.path, "file is gone");
            }
        }
        Err(e) => tracing::warn!(track = file.id, error = %e, "can't reach a file that failed"),
    }
    Ok(())
}

/// Plays a file through without making a sound.
async fn play_through(file: &SongFile) -> Result<(), String> {
    let path = std::path::PathBuf::from(&file.path);
    let expected = file.duration_ms.and_then(|ms| u64::try_from(ms).ok());
    tokio::task::spawn_blocking(move || djinn_engine::verify(&path, expected))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// After a library check: tries marked files again (a share that was down
/// may be back), then fills gaps from whatever the library now has. A check
/// that found nothing new can still have marks to clear.
pub async fn after_library_check(state: &AppState, changed: bool) -> anyhow::Result<()> {
    let mut cleared = 0;
    for file in state.store.problem_files().await? {
        if play_through(&file).await.is_ok() && state.store.clear_track_problem(file.id).await? {
            cleared += 1;
        }
    }
    let filled = if changed || cleared > 0 {
        fill_gaps(&state.store).await?
    } else {
        0
    };
    if cleared > 0 || filled > 0 {
        tracing::info!(cleared, filled, "playlists have more songs that can play");
        for jukebox in state.all_jukeboxes() {
            jukebox.lock().await.auto_mix_changed().await?;
        }
    }
    Ok(())
}

/// Gives flagged playlist entries a song, wherever the library now has one.
pub async fn fill_gaps(store: &Store) -> anyhow::Result<usize> {
    let unfilled = store.unfilled_entries().await?;
    if unfilled.is_empty() {
        return Ok(0);
    }
    let index = library_index(store).await?;
    let mut filled = 0;
    for entry in unfilled {
        if let Some(track) = index.find(&entry.song) {
            store.fill_playlist_entry(entry.entry_id, track).await?;
            filled += 1;
        }
    }
    Ok(filled)
}

/// How a playlist check is going. It stays here when done, for the page to
/// read.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PlaylistCheck {
    pub playlist_id: i64,
    pub running: bool,
    pub done: usize,
    pub total: usize,
    /// Songs that didn't play through.
    pub problems: usize,
}

async fn check_status(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Option<PlaylistCheck>>> {
    let status = state
        .scan
        .playlist_check
        .lock()
        .map_or(None, |check| check.clone());
    Ok(Json(status))
}

fn update_check(state: &AppState, change: impl FnOnce(&mut PlaylistCheck)) {
    if let Ok(mut check) = state.scan.playlist_check.lock() {
        if let Some(check) = check.as_mut() {
            change(check);
        }
    }
}

/// Starts playing every song in a playlist through, in the background.
async fn start_check(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if state.store.playlist_for(admin.id, id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    let files = state.store.playlist_files(id).await?;
    {
        let mut check = state
            .scan
            .playlist_check
            .lock()
            .map_err(|_| anyhow::anyhow!("playlist check status is unavailable"))?;
        if check.as_ref().is_some_and(|c| c.running) {
            return Err(ApiError::conflict("a playlist check is already running"));
        }
        *check = Some(PlaylistCheck {
            playlist_id: id,
            running: true,
            done: 0,
            total: files.len(),
            problems: 0,
        });
    }
    tokio::spawn(async move {
        if let Err(e) = run_check(&state, id, files).await {
            tracing::error!(error = %format!("{e:#}"), "playlist check failed");
        }
        update_check(&state, |check| check.running = false);
    });
    Ok(StatusCode::ACCEPTED)
}

async fn run_check(state: &AppState, id: i64, files: Vec<SongFile>) -> anyhow::Result<()> {
    let mut results = stream::iter(files)
        .map(|file| async move {
            let result = play_through(&file).await;
            (file, result)
        })
        .buffer_unordered(CHECKS);
    while let Some((file, result)) = results.next().await {
        let failed = result.is_err();
        match result {
            Ok(()) => {
                state.store.clear_track_problem(file.id).await?;
            }
            Err(error) => note_failure(&state.store, &file, &error).await?,
        }
        update_check(state, |check| {
            check.done += 1;
            check.problems += usize::from(failed);
        });
    }
    for jukebox in state.all_jukeboxes() {
        let mut jukebox = jukebox.lock().await;
        if jukebox.active_playlist() == Some(id) {
            jukebox.auto_mix_changed().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::{Role, ScannedTrack};

    fn song(title: &str, artist: &str) -> ScannedTrack {
        ScannedTrack {
            path: format!("/m/{title}.flac"),
            title: title.into(),
            artist: Some(artist.into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_missing_song_listed_twice_holds_one_place() {
        let index = SongIndex::new(Vec::new());
        let song = |title: &str| WantedSong {
            title: title.into(),
            artist: Some("Someone".into()),
            ..Default::default()
        };
        let rows = match_rows(&index, vec![song("Heaven"), song("Other"), song("HEAVEN")]);
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn video_ids_are_never_sent_to_youtube() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let import = |text: &str| serde_json::json!({ "text": text }).to_string();
        let ids = import("Video ID,Playlist Video Creation Timestamp\ndQw4w9WgXcQ,2024\n");
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists/import",
            Some(&ids),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(state.store.playlists().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_youtube_link_without_a_key_sends_nothing() {
        let state = AppState::for_tests().await;
        let sam = session(&state, "sam", Role::User).await;
        let link = serde_json::json!({
            "text": "https://music.youtube.com/playlist?list=PL123&si=abc",
        })
        .to_string();
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists/import",
            Some(&link),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn only_admins_set_the_key_and_nobody_can_read_it_back() {
        let state = AppState::for_tests().await;
        let sam = session(&state, "sam", Role::User).await;
        let admin = session(&state, "admin", Role::Admin).await;
        let path = "/api/playlists/youtube-key";
        let body = Some(r#"{"key":"SECRET-KEY"}"#);
        let (status, _) = call(&state, Method::PUT, path, body, Some(&sam)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, seen) = get_body(&state, path, &sam).await;
        assert_eq!(seen, r#"{"has_key":false}"#);

        let (status, _) = call(&state, Method::PUT, path, body, Some(&admin)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        for who in [&sam, &admin] {
            let (_, seen) = get_body(&state, path, who).await;
            assert_eq!(seen, r#"{"has_key":true}"#);
        }

        let (status, _) = call(
            &state,
            Method::PUT,
            path,
            Some(r#"{"key":""}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, seen) = get_body(&state, path, &admin).await;
        assert_eq!(seen, r#"{"has_key":false}"#);
    }

    #[tokio::test]
    async fn import_flags_what_the_library_lacks_then_finds_it_later() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        state
            .store
            .save_scanned(&[song("S&M", "Rihanna"), song("I Cry", "Flo Rida")])
            .await
            .unwrap();
        let body = serde_json::json!({
            "filename": "party.csv",
            "text": "Track name,Artist name,Album\nS&M,Rihanna,Loud\nNumb,\"Marshmello, Khalid\",\nI Cry,Flo Rida,\n",
        })
        .to_string();
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists/import",
            Some(&body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (_, list) = get_body(&state, "/api/playlists", &admin).await;
        let list: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(list[0]["name"], "party");
        assert_eq!(list[0]["problem_songs"], 1);
        let id = list[0]["id"].as_i64().unwrap();

        let ids = state.store.playlist_track_ids(id).await.unwrap();
        assert_eq!(ids.len(), 2, "only what the library has can play");
        let detail = state.store.playlist(id).await.unwrap().unwrap();
        assert_eq!(detail.entries[1].name.as_deref(), Some("Numb"));
        assert_eq!(detail.entries[1].problem_songs, 1);

        // The song turns up; its place in the list is waiting for it.
        state
            .store
            .save_scanned(&[song("Numb", "Marshmello")])
            .await
            .unwrap();
        after_library_check(&state, true).await.unwrap();
        let detail = state.store.playlist(id).await.unwrap().unwrap();
        let titles: Vec<_> = detail.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["S&M", "Numb", "I Cry"]);
        assert_eq!(
            detail.entries.iter().map(|e| e.problem_songs).sum::<i64>(),
            0
        );
    }

    #[tokio::test]
    async fn anyone_signed_in_imports_their_own_and_links_are_explained() {
        let state = AppState::for_tests().await;
        let sam = session(&state, "sam", Role::User).await;
        let jo = session(&state, "jo", Role::User).await;
        let body = serde_json::json!({ "text": "Rihanna - S&M", "name": "Sam's" }).to_string();
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists/import",
            Some(&body),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (_, mine) = get_body(&state, "/api/playlists", &sam).await;
        let mine: serde_json::Value = serde_json::from_str(&mine).unwrap();
        assert_eq!(mine[0]["name"], "Sam's");
        let (_, others) = get_body(&state, "/api/playlists", &jo).await;
        assert_eq!(others, "[]", "private to whoever imported it");

        let link =
            serde_json::json!({ "text": "https://open.spotify.com/playlist/abc" }).to_string();
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists/import",
            Some(&link),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_file_that_comes_back_is_marked_clear_after_a_check() {
        let state = AppState::for_tests().await;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures/gapless_a.flac");
        state
            .store
            .save_scanned(&[ScannedTrack {
                path: fixture.to_string_lossy().into_owned(),
                title: "Good".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.library_track_ids().await.unwrap()[0];
        state
            .store
            .mark_track_problem(id, "share was down", None, None)
            .await
            .unwrap();
        assert!(state.store.library_track_ids().await.unwrap().is_empty());
        after_library_check(&state, false).await.unwrap();
        assert_eq!(state.store.library_track_ids().await.unwrap(), [id]);
    }

    #[tokio::test]
    async fn failures_mark_broken_files_and_leave_unreachable_folders_alone() {
        let state = AppState::for_tests().await;
        let dir = std::env::temp_dir().join(format!("djinn-note-failure-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("broken.flac");
        std::fs::write(&broken, b"not audio").unwrap();
        let gone = dir.join("gone.flac");
        let offline = dir.join("unplugged/drive.flac");
        let paths = [&broken, &gone, &offline].map(|p| p.to_string_lossy().into_owned());
        let tracks: Vec<ScannedTrack> = paths
            .iter()
            .map(|path| ScannedTrack {
                path: path.clone(),
                title: path.clone(),
                ..Default::default()
            })
            .collect();
        state.store.save_scanned(&tracks).await.unwrap();
        let known = state.store.known_files().await.unwrap();
        for k in &known {
            let file = state.store.song_file(k.id).await.unwrap().unwrap();
            note_failure(&state.store, &file, "failed").await.unwrap();
        }
        let of = |path: &str| known.iter().find(|k| k.path == path).unwrap().id;
        let playable = state.store.library_track_ids().await.unwrap();
        assert_eq!(playable, [of(&paths[2])], "could be an unplugged drive");
        let problems = state.store.problem_files().await.unwrap();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].id, of(&paths[0]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
