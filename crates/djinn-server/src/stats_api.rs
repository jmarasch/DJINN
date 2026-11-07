//! Admin > Stats: what's been played and asked for, at the party and on
//! DJINN GO, over a time range and optionally for one person; the last
//! party's recap; and clearing history by range or person.

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use djinn_store::{
    HistoryCounts, StatArtist, StatPerson, StatRequested, StatSong, StatsFilter, StatsSummary,
};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Rows in each top list.
const TOP: i64 = 25;
/// A break this long with nothing playing ends a party.
const PARTY_GAP_MS: i64 = 6 * 60 * 60 * 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats", get(stats))
        .route("/stats/recap", get(recap))
        .route("/stats/clear", get(clear_preview).post(clear))
        .route("/stats/export.csv", get(export))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct FilterQuery {
    from: Option<i64>,
    until: Option<i64>,
    user: Option<i64>,
    #[serde(default = "yes")]
    party: bool,
    #[serde(default = "yes")]
    go: bool,
}

fn yes() -> bool {
    true
}

impl From<FilterQuery> for StatsFilter {
    fn from(q: FilterQuery) -> Self {
        Self {
            from: q.from.unwrap_or(0),
            until: q.until.unwrap_or(i64::MAX),
            user: q.user,
            party: q.party,
            go: q.go,
        }
    }
}

#[derive(Serialize)]
struct Stats {
    /// Where the range starts (the recap's party start).
    from: Option<i64>,
    summary: StatsSummary,
    top_songs: Vec<StatSong>,
    top_artists: Vec<StatArtist>,
    top_requested: Vec<StatRequested>,
    top_requesters: Vec<StatPerson>,
    most_skipped: Vec<StatSong>,
    /// (hour 0 to 23, plays), this computer's time.
    by_hour: Vec<(i64, i64)>,
    /// (day 0 Sunday to 6, plays).
    by_weekday: Vec<(i64, i64)>,
    /// ("2026-11-06", plays).
    by_day: Vec<(String, i64)>,
}

async fn gather(state: &AppState, filter: &StatsFilter) -> anyhow::Result<Stats> {
    let store = &state.store;
    Ok(Stats {
        from: (filter.from > 0).then_some(filter.from),
        summary: store.stats_summary(filter).await?,
        top_songs: store.top_songs(filter, TOP).await?,
        top_artists: store.top_artists(filter, TOP).await?,
        top_requested: store.top_requested(filter, TOP).await?,
        top_requesters: store.top_requesters(filter, TOP).await?,
        most_skipped: store.most_skipped(filter, TOP).await?,
        by_hour: store.plays_by_hour(filter).await?,
        by_weekday: store.plays_by_weekday(filter).await?,
        by_day: store.plays_by_day(filter).await?,
    })
}

async fn stats(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<FilterQuery>,
) -> ApiResult<Json<Stats>> {
    Ok(Json(gather(&state, &query.into()).await?))
}

/// The last party: from the first song after a long break until now.
async fn recap(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Option<Stats>>> {
    let Some(from) = state.store.last_party_start(PARTY_GAP_MS).await? else {
        return Ok(Json(None));
    };
    let filter = StatsFilter {
        from,
        go: false,
        ..StatsFilter::default()
    };
    Ok(Json(Some(gather(&state, &filter).await?)))
}

async fn clear_preview(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<FilterQuery>,
) -> ApiResult<Json<HistoryCounts>> {
    Ok(Json(state.store.history_counts(&query.into()).await?))
}

/// Forgets the plays and requests the filter covers.
async fn clear(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<FilterQuery>,
) -> ApiResult<Json<HistoryCounts>> {
    if !body.party && !body.go {
        return Err(ApiError::bad_request("choose party, GO or both"));
    }
    let done = state.store.clear_history_matching(&body.into()).await?;
    // What the auto mix learned came from these.
    state.jukebox.lock().await.party_changed();
    tracing::warn!(
        party_plays = done.party_plays,
        go_plays = done.go_plays,
        requests = done.requests,
        "history cleared"
    );
    Ok(Json(done))
}

/// The top lists as a spreadsheet.
async fn export(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<FilterQuery>,
) -> ApiResult<impl IntoResponse> {
    let stats = gather(&state, &query.into()).await?;
    let cell = |text: &str| crate::problems_api::csv_cell(text);
    let mut out = String::from("list,rank,name,artist,count,skips\n");
    let mut row = |list: &str, rank: usize, name: &str, artist: &str, count: i64, skips: i64| {
        out.push_str(&format!(
            "{list},{rank},{},{},{count},{skips}\n",
            cell(name),
            cell(artist)
        ));
    };
    for (i, s) in stats.top_songs.iter().enumerate() {
        row(
            "top songs",
            i + 1,
            &s.title,
            s.artist.as_deref().unwrap_or(""),
            s.plays,
            s.skips,
        );
    }
    for (i, a) in stats.top_artists.iter().enumerate() {
        row("top artists", i + 1, &a.name, "", a.plays, a.skips);
    }
    for (i, r) in stats.top_requested.iter().enumerate() {
        let title = r.title.as_deref().unwrap_or("(removed song)");
        row(
            "most requested",
            i + 1,
            title,
            r.artist.as_deref().unwrap_or(""),
            r.requests,
            0,
        );
    }
    for (i, p) in stats.top_requesters.iter().enumerate() {
        row(
            "top requesters",
            i + 1,
            p.name.as_deref().unwrap_or("unknown"),
            "",
            p.requests,
            0,
        );
    }
    for (i, s) in stats.most_skipped.iter().enumerate() {
        row(
            "most skipped",
            i + 1,
            &s.title,
            s.artist.as_deref().unwrap_or(""),
            s.plays,
            s.skips,
        );
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"djinn-stats.csv\"",
            ),
        ],
        out,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    #[tokio::test]
    async fn admins_read_stats_and_clear_a_range() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        let listener = session(&state, "lu", Role::User).await;
        let (status, _) = get_body(&state, "/api/stats", &listener).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = get_body(&state, "/api/stats?go=false&from=5", &admin).await;
        assert_eq!(status, StatusCode::OK);
        let stats: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(stats["summary"]["plays"], 0);
        assert_eq!(stats["from"], 5);

        let (status, body) = get_body(&state, "/api/stats/recap", &admin).await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "null"));

        let (status, body) = get_body(&state, "/api/stats/clear?until=10", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"requests\":0"));
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/stats/clear",
            Some(r#"{"party":false,"go":false}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/stats/clear",
            Some(r#"{"until":10}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = get_body(&state, "/api/stats/export.csv", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.starts_with("list,rank,name"));
    }
}
