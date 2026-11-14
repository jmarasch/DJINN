//! The library editor's compare screen: reads every song's tags from its
//! file and sets them next to what DJINN's database holds, field by field,
//! with the reason they differ (a correction made in DJINN, a gap an
//! identification filled, or something else). It only reads: nothing in the
//! music folders or the database changes. It lives behind the protected
//! areas' lock, since writing to files will happen from the same screen.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::compare::{self, FieldDiff, Why};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::protected::{audit, Elevated};
use crate::state::AppState;

/// Files read between checks on whether heavy work may run.
const CHUNK: usize = 400;
/// Threads reading tags at once (mostly helps on network shares).
const READERS: usize = 4;
/// Most differing songs kept, so a hopeless library can't fill the memory.
const KEEP: usize = 100_000;
const PAGE: usize = 50;

pub fn router() -> Router<AppState> {
    Router::new().route("/library/compare", get(show).post(start))
}

#[derive(Clone, Debug, Serialize)]
struct SongDiff {
    track_id: i64,
    path: String,
    title: String,
    artist: Option<String>,
    diffs: Vec<FieldDiff>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Run {
    started_at: i64,
    finished_at: Option<i64>,
    total: usize,
    done: usize,
    /// Songs with at least one difference.
    differing: usize,
    /// Files that couldn't be read (gone, or tags unreadable).
    unreadable: usize,
    /// How many differences there are per field and reason.
    counts: Vec<Count>,
    #[serde(skip)]
    songs: Vec<SongDiff>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Count {
    field: &'static str,
    why: Why,
    songs: usize,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<Run>> = Mutex::new(None);

fn with(f: impl FnOnce(&mut Run)) {
    if let Ok(mut guard) = LAST.lock() {
        if let Some(run) = guard.as_mut() {
            f(run);
        }
    }
}

/// Starts a comparison of the whole library in the background.
async fn start(
    Elevated(admin): Elevated,
    device: crate::auth::Device,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err(ApiError::conflict("a comparison is already running"));
    }
    if let Ok(mut guard) = LAST.lock() {
        *guard = Some(Run {
            started_at: djinn_store::now_ms(),
            ..Run::default()
        });
    }
    audit(&state, &admin, device.ip.as_deref(), "compare_run", "").await;
    tokio::spawn(async move {
        if let Err(e) = run(&state).await {
            tracing::warn!(error = %format!("{e:#}"), "library comparison stopped");
            with(|r| r.error = Some(e.to_string()));
        }
        with(|r| r.finished_at = Some(djinn_store::now_ms()));
        RUNNING.store(false, Ordering::Release);
    });
    Ok(StatusCode::ACCEPTED)
}

async fn run(state: &AppState) -> anyhow::Result<()> {
    let rows = state.store.compare_rows().await?;
    let edits = state.store.tag_edits_by_path().await?;
    let identified = state.store.identified_paths().await?;
    let original_years = djinn_library::original_years(&state.store).await?;
    with(|r| r.total = rows.len());
    let mut counts: HashMap<(&'static str, Why), usize> = HashMap::new();
    let mut done = 0;
    for chunk in rows.chunks(CHUNK) {
        let go_on = crate::background::wait_turn(state, || async { true }, || async {}).await;
        if !go_on || *state.shutdown.borrow() {
            anyhow::bail!("DJINN was stopping");
        }
        let rows_owned = chunk.to_vec();
        let per = rows_owned.len().div_ceil(READERS).max(1);
        let read: Vec<Result<compare::FileTags, String>> = tokio::task::spawn_blocking(move || {
            std::thread::scope(|s| {
                let handles: Vec<_> = rows_owned
                    .chunks(per)
                    .map(|part| {
                        s.spawn(move || {
                            part.iter()
                                .map(|r| compare::read(&PathBuf::from(&r.path), original_years))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            })
        })
        .await?;
        let mut unreadable = 0;
        let mut found: Vec<SongDiff> = Vec::new();
        for (row, file) in chunk.iter().zip(read) {
            let Ok(file) = file else {
                unreadable += 1;
                continue;
            };
            let diffs = compare::differences(
                row,
                &file,
                edits.get(&row.path),
                identified.contains(&row.path),
            );
            if diffs.is_empty() {
                continue;
            }
            for d in &diffs {
                *counts.entry((d.field, d.why)).or_default() += 1;
            }
            found.push(SongDiff {
                track_id: row.id,
                path: row.path.clone(),
                title: row.title.clone(),
                artist: row.artist.clone(),
                diffs,
            });
        }
        done += chunk.len();
        with(|r| {
            r.done = done;
            r.unreadable += unreadable;
            r.differing += found.len();
            let room = KEEP.saturating_sub(r.songs.len());
            r.songs.extend(found.into_iter().take(room));
            let mut list: Vec<Count> = counts
                .iter()
                .map(|(&(field, why), &songs)| Count { field, why, songs })
                .collect();
            list.sort_by_key(|c| (c.why, c.field));
            r.counts = list;
        });
    }
    Ok(())
}

#[derive(Deserialize)]
struct Filter {
    #[serde(default)]
    offset: usize,
    field: Option<String>,
    why: Option<Why>,
}

#[derive(Serialize)]
struct Shown {
    running: bool,
    run: Option<Run>,
    /// Songs matching the filter, and one page of them.
    matching: usize,
    songs: Vec<SongDiff>,
}

async fn show(_: Elevated, Query(filter): Query<Filter>) -> Json<Shown> {
    let run = LAST.lock().ok().and_then(|g| g.clone());
    let matches = |s: &SongDiff| {
        s.diffs.iter().any(|d| {
            filter.field.as_deref().is_none_or(|f| d.field == f)
                && filter.why.is_none_or(|w| d.why == w)
        })
    };
    let (matching, songs) = match &run {
        Some(r) => {
            let all: Vec<&SongDiff> = r.songs.iter().filter(|s| matches(s)).collect();
            let page = all
                .iter()
                .skip(filter.offset)
                .take(PAGE)
                .map(|s| (*s).clone())
                .collect();
            (all.len(), page)
        }
        None => (0, Vec::new()),
    };
    Json(Shown {
        running: RUNNING.load(Ordering::Acquire),
        run,
        matching,
        songs,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use djinn_store::Role;

    use super::*;
    use crate::test_util::{call, get_body, session};

    #[tokio::test]
    async fn the_comparison_is_locked_and_reads_only() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        // Locked at the default level.
        let (status, _) = get_body(&state, "/api/library/compare", &admin).await;
        assert_eq!(status, StatusCode::LOCKED);
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let (status, body) = get_body(&state, "/api/library/compare", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"matching\":0"), "{body}");
        let user = session(&state, "sam", Role::User).await;
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/compare",
            Some("{}"),
            Some(&user),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
