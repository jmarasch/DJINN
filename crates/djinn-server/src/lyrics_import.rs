//! Importing lyrics from a zip (Admin > Library > Lyrics). Each `.lrc` or
//! `.txt` file is matched to a song, first by the artist and title in its
//! header lines, then by its place in the zip (the library folder's name and
//! the song's path under it), and saved for songs that have no lyrics. Lyrics
//! from song files, tags and LRCLIB always win; the zip only fills gaps.
//!
//! The text of a file is only ever stored as text. Files that look like code
//! or markup are thrown out with a warning, and program or data files in the
//! zip are never read.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::lyrics_import::{self as lines, Content};
use djinn_library::playlist_match::SongIndex;
use djinn_store::{NewLyrics, SongFacts, WantedSong};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::auth::AdminUser;
use crate::backup::data_dir;
use crate::error::{ApiError, ApiResult};
use crate::lyrics::LYRICS_LOOKUP;
use crate::state::AppState;
use crate::{folders, settings};

/// Largest zip taken, in bytes.
pub const MAX_ZIP_BYTES: u64 = 250 * 1024 * 1024;
/// Most files one zip may hold.
const MAX_ENTRIES: usize = 150_000;
/// Most text one zip may hold once unpacked, which stops zip bombs.
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
/// Entries read between saves.
const CHUNK: usize = 400;
/// Names kept in the report's lists; the counts are always complete.
const LIST_CAP: usize = 2000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/lyrics/import",
            get(status).post(upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/lyrics/import/report", get(report_file))
        .route("/lyrics/import/dismiss", post(dismiss))
}

// --- The report ---

/// A file thrown out, and why (never its content).
#[derive(Clone, Debug, Serialize)]
pub struct Warning {
    pub entry: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub file: String,
    pub replace: bool,
    pub even_if_unasked: bool,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// Files in the zip, and how many are done.
    pub total: u64,
    pub done: u64,
    pub saved_timed: u64,
    pub saved_plain: u64,
    /// The song has lyrics from its file, its tags or LRCLIB.
    pub had_lyrics: u64,
    /// The song has lyrics from an earlier import (kept unless replacing).
    pub kept_import: u64,
    /// Waiting for LRCLIB to be asked about the song first.
    pub waiting_lrclib: u64,
    pub instrumental: u64,
    pub empty: u64,
    pub too_big: u64,
    /// Files that aren't lyrics (folders, covers, system files).
    pub ignored: u64,
    pub not_matched: u64,
    pub ambiguous: u64,
    /// The header named one song and the path another; the header won.
    pub mismatched: u64,
    pub not_matched_names: Vec<String>,
    pub ambiguous_names: Vec<String>,
    pub warning_count: u64,
    pub warnings: Vec<Warning>,
    pub error: Option<String>,
}

impl Report {
    fn warn(&mut self, entry: &str, reason: &str) {
        self.warning_count += 1;
        if self.warnings.len() < LIST_CAP {
            self.warnings.push(Warning {
                entry: entry.to_owned(),
                reason: reason.to_owned(),
            });
        }
    }

    fn not_matched(&mut self, entry: &str) {
        self.not_matched += 1;
        if self.not_matched_names.len() < LIST_CAP {
            self.not_matched_names.push(entry.to_owned());
        }
    }

    fn ambiguous(&mut self, entry: &str) {
        self.ambiguous += 1;
        if self.ambiguous_names.len() < LIST_CAP {
            self.ambiguous_names.push(entry.to_owned());
        }
    }

    /// The report as a text file for download.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut line = |s: String| {
            out.push_str(&s);
            out.push('\n');
        };
        line(format!("Lyrics import: {}", self.file));
        if let Some(e) = &self.error {
            line(format!("Stopped: {e}"));
        }
        line(format!("Files in the zip: {}", self.total));
        line(format!("Saved, timed: {}", self.saved_timed));
        line(format!("Saved, plain: {}", self.saved_plain));
        line(format!("Song already had lyrics: {}", self.had_lyrics));
        line(format!(
            "Song kept lyrics from an earlier import: {}",
            self.kept_import
        ));
        line(format!(
            "Waiting for LRCLIB to be asked first: {}",
            self.waiting_lrclib
        ));
        line(format!("Instrumental marks ignored: {}", self.instrumental));
        line(format!("Empty: {}", self.empty));
        line(format!("Too big: {}", self.too_big));
        line(format!("Not lyrics, ignored: {}", self.ignored));
        line(format!("No matching song: {}", self.not_matched));
        line(format!("More than one matching song: {}", self.ambiguous));
        line(format!(
            "Header and path named different songs (header used): {}",
            self.mismatched
        ));
        line(format!(
            "Thrown out for suspicious content: {}",
            self.warning_count
        ));
        for w in &self.warnings {
            line(format!("  WARNING {}: {}", w.entry, w.reason));
        }
        for name in &self.ambiguous_names {
            line(format!("  AMBIGUOUS {name}"));
        }
        for name in &self.not_matched_names {
            line(format!("  NO SONG {name}"));
        }
        out
    }
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static REPORT: Mutex<Option<Report>> = Mutex::new(None);

fn update(f: impl FnOnce(&mut Report)) {
    if let Ok(mut guard) = REPORT.lock() {
        if let Some(report) = guard.as_mut() {
            f(report);
        }
    }
}

fn snapshot() -> Option<Report> {
    REPORT.lock().ok().and_then(|g| g.clone())
}

// --- Routes ---

#[derive(Serialize)]
struct Status {
    running: bool,
    report: Option<Report>,
}

async fn status(_: AdminUser) -> Json<Status> {
    Json(Status {
        running: RUNNING.load(Ordering::Acquire),
        report: snapshot(),
    })
}

async fn report_file(_: AdminUser) -> ApiResult<Response> {
    let report = snapshot().ok_or_else(ApiError::not_found)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"lyrics-import-report.txt\"",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        report.text(),
    )
        .into_response())
}

/// Clears the last report (and Admin Home's notice about it).
async fn dismiss(_: AdminUser) -> ApiResult<StatusCode> {
    if RUNNING.load(Ordering::Acquire) {
        return Err(ApiError::conflict("an import is running"));
    }
    if let Ok(mut guard) = REPORT.lock() {
        *guard = None;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct UploadQuery {
    #[serde(default)]
    name: String,
    /// Replace lyrics from an earlier import.
    #[serde(default)]
    replace: bool,
    /// Import even for songs LRCLIB hasn't been asked about yet.
    #[serde(default)]
    unasked: bool,
}

/// Takes a zip as the request body and imports it in the background.
async fn upload(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> ApiResult<StatusCode> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err(ApiError::conflict(
            "an import is running; wait for it to finish",
        ));
    }
    let started = receive(&state, &query, body).await;
    match started {
        Ok(path) => {
            let state = state.clone();
            let options = Options {
                replace: query.replace,
                unasked: query.unasked,
            };
            let name: String = query
                .name
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect();
            tokio::spawn(async move {
                if let Ok(mut guard) = REPORT.lock() {
                    *guard = Some(Report {
                        file: name,
                        replace: options.replace,
                        even_if_unasked: options.unasked,
                        started_at: djinn_store::now_ms(),
                        ..Report::default()
                    });
                }
                let result = run(&state, &path, options).await;
                if let Err(e) = &result {
                    tracing::warn!(error = %format!("{e:#}"), "lyrics import stopped");
                    update(|r| r.error = Some(format!("{e}")));
                }
                update(|r| r.finished_at = Some(djinn_store::now_ms()));
                let _ = tokio::fs::remove_file(&path).await;
                if let Some(r) = snapshot() {
                    if r.warning_count > 0 {
                        tracing::warn!(
                            files = r.warning_count,
                            "lyrics import threw out files that looked like code or markup"
                        );
                    }
                    tracing::info!(
                        timed = r.saved_timed,
                        plain = r.saved_plain,
                        no_song = r.not_matched,
                        "lyrics import done"
                    );
                }
                RUNNING.store(false, Ordering::Release);
                // Imported songs are read once for lyrics beside the song,
                // which win, then LRCLIB carries on with the rest.
                crate::lyrics::start(&state);
            });
            Ok(StatusCode::ACCEPTED)
        }
        Err(e) => {
            RUNNING.store(false, Ordering::Release);
            Err(e)
        }
    }
}

/// Saves the upload, refusing what isn't a zip or is too big.
async fn receive(state: &AppState, _query: &UploadQuery, body: Body) -> ApiResult<PathBuf> {
    let dir = data_dir(state).join("uploads");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(anyhow::Error::from)?;
    let path = dir.join(format!("lyrics-{}.zip", crate::auth::new_token()?));
    let saved = save_zip(body, &path).await;
    if let Err(e) = saved {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(e);
    }
    Ok(path)
}

async fn save_zip(body: Body, path: &Path) -> ApiResult<()> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(anyhow::Error::from)?;
    let mut size = 0u64;
    let mut first = true;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| ApiError::bad_request(format!("the upload broke off: {e}")))?;
        if first {
            first = false;
            if !chunk.starts_with(b"PK\x03\x04") {
                return Err(ApiError::bad_request("that isn't a zip file"));
            }
        }
        size += chunk.len() as u64;
        if size > MAX_ZIP_BYTES {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "that zip is over {} MB; send several smaller ones",
                    MAX_ZIP_BYTES / 1024 / 1024
                ),
            ));
        }
        file.write_all(&chunk).await.map_err(anyhow::Error::from)?;
    }
    file.flush().await.map_err(anyhow::Error::from)?;
    if size == 0 {
        return Err(ApiError::bad_request("that file is empty"));
    }
    Ok(())
}

// --- The library, ready to match against ---

struct Library {
    songs: SongIndex,
    /// Song paths without their extension, as `path_key`s.
    by_stem: HashMap<String, Vec<i64>>,
    /// The last two parts of those, for when no folder name fits.
    by_tail: HashMap<String, Vec<i64>>,
    /// Each library folder: its name and its path, as keys.
    roots: Vec<(String, String)>,
}

fn stem_of(path: &str) -> &str {
    match path.rfind('.') {
        Some(dot) if !path[dot..].contains(['/', '\\']) => &path[..dot],
        _ => path,
    }
}

fn tail_of(key: &str) -> Option<String> {
    let mut parts = key.rsplit('/').filter(|p| !p.is_empty());
    let file = parts.next()?;
    let folder = parts.next()?;
    Some(format!("{folder}/{file}"))
}

impl Library {
    fn new(facts: Vec<SongFacts>, roots: &[PathBuf]) -> Self {
        let mut by_stem: HashMap<String, Vec<i64>> = HashMap::new();
        let mut by_tail: HashMap<String, Vec<i64>> = HashMap::new();
        for fact in &facts {
            let key = lines::path_key(stem_of(&fact.path));
            if let Some(tail) = tail_of(&key) {
                by_tail.entry(tail).or_default().push(fact.id);
            }
            by_stem.entry(key).or_default().push(fact.id);
        }
        let roots = roots
            .iter()
            .filter_map(|r| {
                let name = r.file_name()?.to_string_lossy().into_owned();
                Some((
                    lines::path_key(&name),
                    lines::path_key(&r.to_string_lossy()),
                ))
            })
            .collect();
        Self {
            songs: SongIndex::new(facts),
            by_stem,
            by_tail,
            roots,
        }
    }

    /// The song a zip entry's place names: its library folder's name, then
    /// the path under it. `Err` when several songs fit.
    fn by_path(&self, entry: &str) -> Result<Option<i64>, ()> {
        let key = lines::path_key(stem_of(entry));
        let first = key.split('/').find(|p| !p.is_empty()).unwrap_or("");
        let mut found: Vec<i64> = Vec::new();
        for (name, root) in &self.roots {
            if name != first {
                continue;
            }
            let under = key
                .trim_start_matches('/')
                .strip_prefix(first)
                .unwrap_or("");
            if let Some(ids) = self.by_stem.get(&format!("{root}{under}")) {
                found.extend(ids);
            }
        }
        if found.is_empty() {
            // The music may sit under another folder name here.
            if let Some(ids) = tail_of(&key).and_then(|t| self.by_tail.get(&t)) {
                found.extend(ids);
            }
        }
        found.sort_unstable();
        found.dedup();
        match found.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(*one)),
            _ => Err(()),
        }
    }

    /// The song a header names, when artist and title are both there.
    fn by_tags(&self, header: &lines::Header) -> Option<i64> {
        let (Some(title), Some(artist)) = (&header.title, &header.artist) else {
            return None;
        };
        self.songs.find(&WantedSong {
            title: title.clone(),
            artist: Some(artist.clone()),
            album: header.album.clone(),
            duration_ms: header.length_ms,
            path: None,
            either_way: false,
        })
    }
}

// --- The job ---

#[derive(Clone, Copy)]
struct Options {
    replace: bool,
    unasked: bool,
}

/// One zip entry read and checked, ready to match.
enum Item {
    Ignored,
    TooBig,
    Warning(String, &'static str),
    Lyrics {
        name: String,
        header: lines::Header,
        content: Content,
    },
}

fn read_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    index: usize,
    total: &mut u64,
) -> Result<Item, String> {
    let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
    let name = String::from_utf8_lossy(file.name_raw()).into_owned();
    if file.is_dir() {
        return Ok(Item::Ignored);
    }
    let escapes = name.starts_with(['/', '\\'])
        || name
            .split(['/', '\\'])
            .any(|p| p == ".." || p.ends_with(':'));
    if escapes || file.enclosed_name().is_none() {
        return Ok(Item::Warning(name, "unsafe path"));
    }
    let lower = name.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if lower.split('/').any(|p| p == "__macosx") || base.starts_with('.') || base == "thumbs.db" {
        return Ok(Item::Ignored);
    }
    if lines::dangerous_extension(&lower) {
        return Ok(Item::Warning(name, "program or data file, not read"));
    }
    if !(lower.ends_with(".lrc") || lower.ends_with(".txt")) {
        return Ok(Item::Ignored);
    }
    if let Some(why) = lines::suspicious(&name) {
        return Ok(Item::Warning(name, why));
    }
    *total += file.size();
    if *total > MAX_TOTAL_BYTES {
        return Err("the zip holds more text than is allowed once unpacked".into());
    }
    if file.size() > lines::MAX_FILE_BYTES {
        return Ok(Item::TooBig);
    }
    let mut bytes = Vec::new();
    // The declared size can lie; never read past the limit.
    (&mut file)
        .take(lines::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > lines::MAX_FILE_BYTES {
        return Ok(Item::TooBig);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Item::Warning(name, "not valid text"));
    };
    if let Some(why) = lines::suspicious(&text) {
        return Ok(Item::Warning(name, why));
    }
    Ok(Item::Lyrics {
        header: lines::header(&text),
        content: lines::content(&text),
        name,
    })
}

async fn run(state: &AppState, zip_path: &Path, options: Options) -> anyhow::Result<()> {
    let facts = state.store.song_facts().await?;
    let roots = folders::library_roots(state).await?;
    let library = tokio::task::spawn_blocking(move || Library::new(facts, &roots)).await?;
    let lookups = settings::flag(&state.store, LYRICS_LOOKUP, true).await?;

    let file = std::fs::File::open(zip_path)?;
    let archive = zip::ZipArchive::new(file)
        .map_err(|e| anyhow::anyhow!("that isn't a zip DJINN can read: {e}"))?;
    let count = archive.len();
    if count > MAX_ENTRIES {
        anyhow::bail!("that zip holds {count} files; the most is {MAX_ENTRIES}");
    }
    update(|r| r.total = count as u64);
    let archive = Arc::new(Mutex::new(archive));
    let mut unpacked = 0u64;

    let mut next = 0usize;
    while next < count {
        if *state.shutdown.borrow() {
            anyhow::bail!("DJINN was stopping");
        }
        let end = (next + CHUNK).min(count);
        let shared = archive.clone();
        let (items, total_after) = tokio::task::spawn_blocking(move || {
            let mut archive = shared
                .lock()
                .map_err(|_| "the zip reader failed".to_owned())?;
            let mut total = unpacked;
            let mut items = Vec::with_capacity(end - next);
            for i in next..end {
                items.push(read_entry(&mut archive, i, &mut total)?);
            }
            Ok::<_, String>((items, total))
        })
        .await?
        .map_err(|e| anyhow::anyhow!(e))?;
        unpacked = total_after;
        save_chunk(state, &library, items, options, lookups).await?;
        update(|r| r.done = end as u64);
        next = end;
        // Heavy work waits its turn like the other lyrics jobs.
        let _ = crate::background::wait_turn(state, || async { true }, || async {}).await;
    }
    Ok(())
}

async fn save_chunk(
    state: &AppState,
    library: &Library,
    items: Vec<Item>,
    options: Options,
    lookups: bool,
) -> anyhow::Result<()> {
    // Match first, so the songs' lyrics can be looked at in one query.
    struct Found {
        id: i64,
        found: djinn_library::lyrics::Found,
    }
    let mut matched: Vec<Found> = Vec::new();
    for item in items {
        match item {
            Item::Ignored => update(|r| r.ignored += 1),
            Item::TooBig => update(|r| r.too_big += 1),
            Item::Warning(name, why) => update(|r| r.warn(&name, why)),
            Item::Lyrics {
                name,
                header,
                content,
            } => {
                let found = match content {
                    Content::Instrumental => {
                        update(|r| r.instrumental += 1);
                        continue;
                    }
                    Content::Empty => {
                        update(|r| r.empty += 1);
                        continue;
                    }
                    Content::Lyrics(found) => found,
                };
                let by_path = library.by_path(&name);
                let id = match (library.by_tags(&header), by_path) {
                    (Some(id), Ok(Some(other))) if id != other => {
                        update(|r| r.mismatched += 1);
                        id
                    }
                    (Some(id), _) => id,
                    (None, Ok(Some(id))) => id,
                    (None, Ok(None)) => {
                        update(|r| r.not_matched(&name));
                        continue;
                    }
                    (None, Err(())) => {
                        update(|r| r.ambiguous(&name));
                        continue;
                    }
                };
                matched.push(Found { id, found });
            }
        }
    }
    let ids: Vec<i64> = matched.iter().map(|m| m.id).collect();
    let states: HashMap<i64, _> = state
        .store
        .lyrics_states(&ids)
        .await?
        .into_iter()
        .map(|s| (s.track_id, s))
        .collect();
    let mut saved: HashSet<i64> = HashSet::new();
    for m in matched {
        let current = states.get(&m.id);
        // LRCLIB gets the first go at songs it hasn't been asked about.
        let unasked = match current {
            None => true,
            Some(s) => s.source == "none" && s.online_checked_at.is_none(),
        };
        if lookups && unasked && !options.unasked {
            update(|r| r.waiting_lrclib += 1);
            continue;
        }
        if saved.contains(&m.id) {
            update(|r| r.kept_import += 1);
            continue;
        }
        let new = NewLyrics {
            synced: m.found.synced.as_deref(),
            plain: m.found.plain.as_deref(),
            instrumental: false,
            source: "bulk",
        };
        if state
            .store
            .save_bulk_lyrics(m.id, &new, options.replace)
            .await?
        {
            saved.insert(m.id);
            let timed = m.found.synced.is_some();
            update(|r| {
                if timed {
                    r.saved_timed += 1;
                } else {
                    r.saved_plain += 1;
                }
            });
        } else if current.is_some_and(|s| s.source == "bulk") {
            update(|r| r.kept_import += 1);
        } else {
            update(|r| r.had_lyrics += 1);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// The report is shared by the whole program, so these tests take turns.
    static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn state_with_songs(name: &str) -> (AppState, std::path::PathBuf) {
        let state = AppState::for_tests().await;
        let dir =
            std::env::temp_dir().join(format!("djinn-lyrics-import-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let songs: Vec<djinn_store::ScannedTrack> = [
            ("Queen", "Bohemian Rhapsody", "Opera"),
            ("Queen", "Radio Ga Ga", "Works"),
            ("Abba", "Waterloo", "Waterloo"),
            ("Abba", "Dancing Queen", "Arrival"),
            ("Abba", "SOS", "Abba"),
        ]
        .iter()
        .enumerate()
        .map(|(i, (artist, title, album))| djinn_store::ScannedTrack {
            path: format!("/srv/music/{artist}/{album}/0{} {title}.flac", i + 1),
            size: 1,
            mtime: 5,
            title: (*title).to_owned(),
            artist: Some((*artist).to_owned()),
            album: Some((*album).to_owned()),
            ..Default::default()
        })
        .collect();
        state.store.save_scanned(&songs).await.unwrap();
        (state, dir)
    }

    async fn import(
        state: &AppState,
        dir: &Path,
        files: &[(&str, &[u8])],
        options: Options,
    ) -> Report {
        let path = dir.join("test.zip");
        std::fs::write(&path, zip_of(files)).unwrap();
        *REPORT.lock().unwrap() = Some(Report::default());
        run(state, &path, options).await.unwrap();
        snapshot().unwrap()
    }

    #[tokio::test]
    async fn lyrics_are_matched_filled_and_checked() {
        let _turn = TURN.lock().await;
        let (state, dir) = state_with_songs("filled").await;
        // Lyrics lookups off: nothing to wait for.
        settings::set_flag(&state.store, LYRICS_LOOKUP, false)
            .await
            .unwrap();
        let ids: Vec<i64> = state
            .store
            .song_facts()
            .await
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        // Waterloo already has lyrics from its tags.
        let own = NewLyrics {
            synced: None,
            plain: Some("Own words"),
            instrumental: false,
            source: "file",
        };
        let waterloo = state
            .store
            .song_facts()
            .await
            .unwrap()
            .iter()
            .find(|f| f.title == "Waterloo")
            .unwrap()
            .id;
        state
            .store
            .save_read_lyrics(waterloo, 5, &own)
            .await
            .unwrap();
        assert_eq!(ids.len(), 5);

        let files: &[(&str, &[u8])] = &[
            // By tags, even though the path is wrong.
            (
                "Music/Elsewhere/x.lrc",
                b"[ar:Queen]\n[ti:Bohemian Rhapsody]\n[00:01.00]Is this the real life?",
            ),
            // By path: no header, under the library folder's name "music".
            (
                "music/Queen/Works/02 Radio Ga Ga.txt",
                b"All we hear is\nRadio ga ga",
            ),
            (
                "music/Abba/Waterloo/03 Waterloo.lrc",
                b"[ar:Abba]\n[ti:Waterloo]\nMy my",
            ),
            ("music/Nobody/Nothing.lrc", b"[00:01.00]Lost"),
            ("music/Abba/Arrival/04 Dancing Queen.lrc", b"[Instrumental]"),
            (
                "music/Abba/Abba/05 SOS.lrc",
                b"[ti:'); DROP TABLE tracks;--]\n[00:01.00]x",
            ),
            ("music/run.sh", b"rm -rf /"),
            ("music/../escape.lrc", b"[00:01.00]x"),
            ("music/cover.jpg", b"jpg"),
        ];
        let report = import(
            &state,
            &dir,
            files,
            Options {
                replace: false,
                unasked: false,
            },
        )
        .await;
        assert_eq!(report.saved_timed, 1, "{report:?}");
        assert_eq!(report.saved_plain, 1, "{report:?}");
        assert_eq!(report.had_lyrics, 1, "{report:?}");
        assert_eq!(report.not_matched, 1, "{report:?}");
        assert_eq!(report.instrumental, 1, "{report:?}");
        assert_eq!(report.warning_count, 3, "{report:?}");
        assert_eq!(report.ignored, 1, "{report:?}");
        let words = state.store.lyrics(ids[1]).await.unwrap().unwrap();
        assert_eq!(words.source, "bulk");
        assert_eq!(words.plain.as_deref(), Some("All we hear is\nRadio ga ga"));
        assert_eq!(
            state
                .store
                .lyrics(waterloo)
                .await
                .unwrap()
                .unwrap()
                .plain
                .as_deref(),
            Some("Own words")
        );
        // The songs table is untouched by the SQL in a header.
        assert_eq!(state.store.song_facts().await.unwrap().len(), 5);

        // Again: imported lyrics stay unless replacing.
        let again = import(
            &state,
            &dir,
            &[("music/Queen/Works/02 Radio Ga Ga.txt", b"Different words")],
            Options {
                replace: false,
                unasked: false,
            },
        )
        .await;
        assert_eq!((again.kept_import, again.saved_plain), (1, 0));
        let replaced = import(
            &state,
            &dir,
            &[("music/Queen/Works/02 Radio Ga Ga.txt", b"Different words")],
            Options {
                replace: true,
                unasked: false,
            },
        )
        .await;
        assert_eq!(replaced.saved_plain, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn lrclib_goes_first_unless_told_not_to() {
        let _turn = TURN.lock().await;
        let (state, dir) = state_with_songs("first").await;
        settings::set_flag(&state.store, LYRICS_LOOKUP, true)
            .await
            .unwrap();
        let files: &[(&str, &[u8])] = &[("music/Queen/Works/02 Radio Ga Ga.lrc", b"[00:01.00]Hi")];
        let waiting = import(
            &state,
            &dir,
            files,
            Options {
                replace: false,
                unasked: false,
            },
        )
        .await;
        assert_eq!((waiting.waiting_lrclib, waiting.saved_timed), (1, 0));
        let forced = import(
            &state,
            &dir,
            files,
            Options {
                replace: false,
                unasked: true,
            },
        )
        .await;
        assert_eq!((forced.waiting_lrclib, forced.saved_timed), (0, 1));
        std::fs::remove_dir_all(&dir).ok();
    }
}
