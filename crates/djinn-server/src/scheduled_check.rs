//! The optional nightly library check (Admin > Library): at a set hour,
//! check the folders for changes, then keep a short summary of what changed
//! since the last one (songs added and gone, problems, copies, reports) for
//! the admin to read in the morning.

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Timelike;
use djinn_store::{now_ms, ProblemKind};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::settings;
use crate::state::AppState;

/// Setting keys: whether it runs, at which hour (this computer's time),
/// the day it last ran ("2026-11-06"), and its last summary (JSON).
pub const ON: &str = "scheduled_check";
pub const HOUR: &str = "scheduled_check_hour";
const DAY: &str = "scheduled_check_day";
const LAST: &str = "scheduled_check_last";
const DEFAULT_HOUR: i64 = 4;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Longest a check is waited for before the summary is written anyway.
const MAX_WAIT: Duration = Duration::from_secs(6 * 60 * 60);

/// What changed since the summary before.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// When the check finished (Unix ms).
    pub at: i64,
    pub added: i64,
    pub removed: i64,
    /// Files not added or broken, not counting hidden kinds.
    pub trouble: i64,
    pub trouble_change: i64,
    /// Songs with more than one copy.
    pub duplicates: i64,
    pub duplicates_change: i64,
    /// Songs listeners reported.
    pub reports: i64,
    pub reports_change: i64,
}

/// Runs the check each day at its hour, while it's on.
pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            match due(&state).await {
                Ok(Some(today)) => {
                    if let Err(e) = state.store.set_setting(DAY, &today).await {
                        tracing::warn!(error = %e, "saving the nightly check day failed");
                        continue;
                    }
                    if let Err(e) = run(&state).await {
                        tracing::warn!(error = %format!("{e:#}"), "nightly library check failed");
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %format!("{e:#}"), "nightly check settings"),
            }
        }
    });
}

/// Today's date if the check should run now.
async fn due(state: &AppState) -> anyhow::Result<Option<String>> {
    if !settings::flag(&state.store, ON, false).await? {
        return Ok(None);
    }
    let hour = settings::number(&state.store, HOUR, DEFAULT_HOUR).await?;
    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let last = state.store.get_setting(DAY).await?;
    Ok(is_due(i64::from(now.hour()), hour, last.as_deref(), &today).then_some(today))
}

/// Due at `hour` (or later the same day, if DJINN was off at the hour)
/// once a day.
fn is_due(now_hour: i64, hour: i64, last_day: Option<&str>, today: &str) -> bool {
    now_hour >= hour && last_day != Some(today)
}

/// Checks the library, waits for it, and writes the summary.
async fn run(state: &AppState) -> anyhow::Result<Summary> {
    tracing::info!("nightly library check starting");
    let before = last(state).await?;
    crate::library_api::start_scan(state);
    let started = std::time::Instant::now();
    while state.scan.running.load(Ordering::Acquire) && started.elapsed() < MAX_WAIT {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // The first time, the day before.
    let since = before.as_ref().map_or(now_ms() - DAY_MS, |b| b.at);
    let (added, removed) = state.store.library_changes_since(since).await?;
    let hidden = crate::problems_api::hidden(&state.store).await?;
    let mut trouble = 0;
    let mut reports = 0;
    for (kind, count) in state.store.problem_counts().await? {
        if kind == ProblemKind::Reported {
            reports = count;
        }
        if kind.is_trouble() && !hidden.contains(&kind) {
            trouble += count;
        }
    }
    let duplicates = i64::try_from(crate::duplicates::current(state).await?.groups.len())?;
    let summary = summarize(
        before.as_ref(),
        now_ms(),
        (added, removed),
        (trouble, duplicates, reports),
    );
    state
        .store
        .set_setting(LAST, &serde_json::to_string(&summary)?)
        .await?;
    tracing::info!(
        added,
        removed,
        trouble,
        duplicates,
        reports,
        "nightly library check finished"
    );
    Ok(summary)
}

fn summarize(
    before: Option<&Summary>,
    at: i64,
    (added, removed): (i64, i64),
    (trouble, duplicates, reports): (i64, i64, i64),
) -> Summary {
    let change = |now: i64, then: fn(&Summary) -> i64| before.map_or(0, |b| now - then(b));
    Summary {
        at,
        added,
        removed,
        trouble,
        trouble_change: change(trouble, |b| b.trouble),
        duplicates,
        duplicates_change: change(duplicates, |b| b.duplicates),
        reports,
        reports_change: change(reports, |b| b.reports),
    }
}

async fn last(state: &AppState) -> anyhow::Result<Option<Summary>> {
    Ok(state
        .store
        .get_setting(LAST)
        .await?
        .and_then(|text| serde_json::from_str(&text).ok()))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/library/scheduled", get(status).put(save))
}

#[derive(Serialize)]
struct Status {
    on: bool,
    hour: i64,
    last: Option<Summary>,
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    Ok(Json(Status {
        on: settings::flag(&state.store, ON, false).await?,
        hour: settings::number(&state.store, HOUR, DEFAULT_HOUR).await?,
        last: last(&state).await?,
    }))
}

#[derive(Deserialize)]
struct Save {
    on: bool,
    hour: i64,
}

async fn save(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Save>,
) -> ApiResult<Json<Status>> {
    if !(0..=23).contains(&body.hour) {
        return Err(ApiError::bad_request("hour must be 0 to 23"));
    }
    settings::set_flag(&state.store, ON, body.on).await?;
    state
        .store
        .set_setting(HOUR, &body.hour.to_string())
        .await?;
    status(admin, State(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    #[test]
    fn runs_once_a_day_from_its_hour() {
        assert!(!is_due(3, 4, None, "2026-11-06"));
        assert!(is_due(4, 4, None, "2026-11-06"));
        // Off at 4: it runs when DJINN starts later that day.
        assert!(is_due(9, 4, Some("2026-11-05"), "2026-11-06"));
        assert!(!is_due(9, 4, Some("2026-11-06"), "2026-11-06"));
    }

    #[test]
    fn summaries_say_what_changed() {
        let first = summarize(None, 1, (100, 0), (3, 2, 0));
        assert_eq!(first.trouble_change, 0, "nothing to compare with");
        let second = summarize(Some(&first), 2, (5, 1), (4, 1, 2));
        assert_eq!(
            (
                second.trouble_change,
                second.duplicates_change,
                second.reports_change
            ),
            (1, -1, 2)
        );
    }

    #[tokio::test]
    async fn admins_turn_it_on_and_runs_write_a_summary() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/library/scheduled",
            Some(r#"{"on":true,"hour":24}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/library/scheduled",
            Some(r#"{"on":true,"hour":3}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let summary = run(&state).await.unwrap();
        assert_eq!(summary.added, 0);
        let (_, body) = get_body(&state, "/api/library/scheduled", &admin).await;
        let status: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(status["on"], true);
        assert_eq!(status["hour"], 3);
        assert_eq!(status["last"]["added"], 0);
    }
}
