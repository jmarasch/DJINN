//! Admin > Library > Problems: what a library check couldn't add, songs
//! that won't play or whose tags couldn't be read, and gaps in the tags.
//! The admin can hide kinds they already know about.

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::header;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use djinn_store::{ProblemItem, ProblemKind, Store, REPORT_REASONS};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Setting key: kinds the admin hid, comma separated.
pub const HIDDEN_KEY: &str = "problems_hidden";
const PAGE: i64 = 100;
const MAX_PAGE: i64 = 500;
/// Longest note on a report, in characters.
const MAX_NOTE: usize = 300;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library/problems", get(summary))
        .route("/library/problems.csv", get(csv))
        .route("/library/problems/hidden", put(set_hidden))
        .route("/library/problems/{kind}", get(list))
        .route("/library/reports/{track_id}", delete(clear_reports))
        .route("/tracks/{id}/report", post(report))
}

#[derive(Deserialize)]
struct Report {
    reason: String,
    #[serde(default)]
    note: Option<String>,
}

/// Anyone signed in (guests too) says a song needs fixing.
async fn report(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    RoutePath(id): RoutePath<i64>,
    Json(body): Json<Report>,
) -> ApiResult<StatusCode> {
    if !REPORT_REASONS.contains(&body.reason.as_str()) {
        return Err(ApiError::bad_request("unknown reason"));
    }
    let note: Option<String> = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(MAX_NOTE).collect());
    if !state
        .store
        .report_song(id, user.id, &body.reason, note.as_deref())
        .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The song's been fixed: its reports go.
async fn clear_reports(
    _: AdminUser,
    State(state): State<AppState>,
    RoutePath(track_id): RoutePath<i64>,
) -> ApiResult<StatusCode> {
    state.store.clear_song_reports(track_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct KindCount {
    kind: ProblemKind,
    count: i64,
    /// Not added, or added but broken (rather than a gap in the tags).
    trouble: bool,
    /// Rows are albums.
    per_album: bool,
    hidden: bool,
}

#[derive(Serialize)]
struct Summary {
    kinds: Vec<KindCount>,
    /// Trouble the admin hasn't hidden, for a badge.
    trouble: i64,
}

pub async fn hidden(store: &Store) -> anyhow::Result<Vec<ProblemKind>> {
    Ok(store
        .get_setting(HIDDEN_KEY)
        .await?
        .unwrap_or_default()
        .split(',')
        .filter_map(|name| ProblemKind::parse(name.trim()))
        .collect())
}

async fn summary(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Summary>> {
    let hidden = hidden(&state.store).await?;
    let kinds: Vec<KindCount> = state
        .store
        .problem_counts()
        .await?
        .into_iter()
        .map(|(kind, count)| KindCount {
            kind,
            count,
            trouble: kind.is_trouble(),
            per_album: kind.per_album(),
            hidden: hidden.contains(&kind),
        })
        .collect();
    let trouble = kinds
        .iter()
        .filter(|k| k.trouble && !k.hidden)
        .map(|k| k.count)
        .sum();
    Ok(Json(Summary { kinds, trouble }))
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
}

async fn list(
    _: AdminUser,
    State(state): State<AppState>,
    RoutePath(kind): RoutePath<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<ProblemItem>>> {
    let kind = ProblemKind::parse(&kind).ok_or_else(ApiError::not_found)?;
    let limit = page.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
    Ok(Json(state.store.problems(kind, page.offset, limit).await?))
}

#[derive(Deserialize, Serialize)]
struct Hidden {
    kinds: Vec<String>,
}

async fn set_hidden(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Hidden>,
) -> ApiResult<Json<Hidden>> {
    let mut kinds = Vec::new();
    for name in &body.kinds {
        let kind = ProblemKind::parse(name)
            .ok_or_else(|| ApiError::bad_request(format!("no such kind: {name}")))?;
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    let names: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
    state
        .store
        .set_setting(HIDDEN_KEY, &names.join(","))
        .await?;
    Ok(Json(Hidden {
        kinds: names.into_iter().map(str::to_owned).collect(),
    }))
}

/// Every problem as a spreadsheet, hidden kinds included.
async fn csv(_: AdminUser, State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let mut out = String::from("kind,path,detail,title,artist,album\n");
    for kind in ProblemKind::ALL {
        let mut offset = 0;
        loop {
            let items = state.store.problems(kind, offset, MAX_PAGE).await?;
            for item in &items {
                let row = [
                    kind.as_str(),
                    item.path.as_deref().unwrap_or(""),
                    &item.detail,
                    item.title.as_deref().unwrap_or(""),
                    item.artist.as_deref().unwrap_or(""),
                    item.album.as_deref().unwrap_or(""),
                ];
                let cells: Vec<String> = row.iter().map(|c| csv_cell(c)).collect();
                out.push_str(&cells.join(","));
                out.push('\n');
            }
            if (items.len() as i64) < MAX_PAGE {
                break;
            }
            offset += MAX_PAGE;
        }
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"djinn-library-problems.csv\"",
            ),
        ],
        out,
    ))
}

/// A CSV cell, quoted when needed. A leading =, +, - or @ gets a ' so a
/// spreadsheet doesn't run it as a formula.
pub fn csv_cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::{Role, ScanProblem};

    #[test]
    fn csv_cells_are_quoted_and_defused() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("=SUM(A1)"), "'=SUM(A1)");
    }

    #[tokio::test]
    async fn admins_see_problems_and_hide_kinds() {
        let state = AppState::for_tests().await;
        state
            .store
            .replace_walk_problems(&[ScanProblem {
                path: "/m/a.wma".into(),
                kind: ProblemKind::Unsupported,
                detail: "wma".into(),
            }])
            .await
            .unwrap();
        let admin = session(&state, "ana", Role::Admin).await;
        let listener = session(&state, "lu", Role::User).await;

        let (status, _) = get_body(&state, "/api/library/problems", &listener).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = get_body(&state, "/api/library/problems", &admin).await;
        assert_eq!(status, StatusCode::OK);
        let summary: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(summary["trouble"], 1);

        let (_, body) = get_body(&state, "/api/library/problems/unsupported", &admin).await;
        let items: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(items[0]["path"], "/m/a.wma");

        let (status, _) = get_body(&state, "/api/library/problems/nonsense", &admin).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/library/problems/hidden",
            Some(r#"{"kinds":["unsupported"]}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = get_body(&state, "/api/library/problems", &admin).await;
        let summary: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(summary["trouble"], 0);

        let (status, body) = get_body(&state, "/api/library/problems.csv", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("unsupported,/m/a.wma,wma"));
    }

    #[tokio::test]
    async fn listeners_report_songs_and_admins_clear_them() {
        let state = AppState::for_tests().await;
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: "/m/s.mp3".into(),
                title: "S".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.library_track_ids().await.unwrap()[0];
        let admin = session(&state, "ana", Role::Admin).await;
        let listener = session(&state, "lu", Role::User).await;
        let path = format!("/api/tracks/{id}/report");

        let (status, _) = call(
            &state,
            Method::POST,
            &path,
            Some(r#"{"reason":"nonsense"}"#),
            Some(&listener),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &state,
            Method::POST,
            &path,
            Some(r#"{"reason":"wrong_cover","note":"  it's the single  "}"#),
            Some(&listener),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/tracks/999/report",
            Some(r#"{"reason":"other"}"#),
            Some(&listener),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (_, body) = get_body(&state, "/api/library/problems/reported", &admin).await;
        assert!(body.contains("wrong_cover\\u001fit's the single"), "{body}");

        let clear = format!("/api/library/reports/{id}");
        let (status, _) = call(&state, Method::DELETE, &clear, None, Some(&listener)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::DELETE, &clear, None, Some(&admin)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = get_body(&state, "/api/library/problems/reported", &admin).await;
        assert_eq!(body, "[]");
    }
}
