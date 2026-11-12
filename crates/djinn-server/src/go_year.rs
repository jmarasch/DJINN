//! Your year on DJINN GO: how much you listened, your top songs and
//! artists, your busiest month and your first song, for any year you
//! played something (this computer's calendar).

use axum::extract::{Query, State};
use axum::Json;
use chrono::{Datelike, TimeZone};
use djinn_store::{StatArtist, StatSong, StatsFilter, Track};
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::go_library::GoUser;
use crate::state::AppState;

const TOP: i64 = 5;

#[derive(Deserialize)]
pub struct YearQuery {
    year: Option<i32>,
}

#[derive(Serialize)]
pub struct Year {
    year: i32,
    /// Years with plays, newest first, to choose from.
    years: Vec<i64>,
    plays: i64,
    minutes: i64,
    /// Different songs.
    songs: i64,
    skips: i64,
    /// Songs asked for at the party.
    requests: i64,
    top_songs: Vec<StatSong>,
    top_artists: Vec<StatArtist>,
    /// Plays in each month, January first.
    months: Vec<i64>,
    /// 1 to 12.
    busiest_month: Option<u32>,
    first: Option<Track>,
}

/// Unix ms at the start of a year here.
fn start_of(year: i32) -> i64 {
    chrono::Local
        .with_ymd_and_hms(year, 1, 1, 0, 0, 0)
        .earliest()
        .map_or(0, |t| t.timestamp_millis())
}

pub async fn year(
    me: GoUser,
    State(state): State<AppState>,
    Query(query): Query<YearQuery>,
) -> ApiResult<Json<Year>> {
    let year = query.year.unwrap_or_else(|| chrono::Local::now().year());
    let (from, until) = (start_of(year), start_of(year + 1));
    let id = me.user.id;
    let listened = StatsFilter {
        from,
        until,
        user: Some(id),
        party: false,
        go: true,
    };
    let asked = StatsFilter {
        party: true,
        go: false,
        ..listened
    };
    let store = &state.store;
    let summary = store.stats_summary(&listened).await?;
    let mut months = vec![0i64; 12];
    for (day, plays) in store.plays_by_day(&listened).await? {
        if let Some(m) = day.get(5..7).and_then(|m| m.parse::<usize>().ok()) {
            if let Some(slot) = months.get_mut(m.wrapping_sub(1)) {
                *slot += plays;
            }
        }
    }
    let busiest_month = months
        .iter()
        .enumerate()
        .filter(|(_, &n)| n > 0)
        .max_by_key(|(_, &n)| n)
        .and_then(|(i, _)| u32::try_from(i + 1).ok());
    let first = match store.first_go_play(id, from, until).await? {
        Some((track, _)) => store.track(track).await?,
        None => None,
    };
    Ok(Json(Year {
        year,
        years: store.go_years(id).await?,
        plays: summary.plays,
        minutes: summary.listened_ms / 60_000,
        songs: summary.songs,
        skips: summary.skips,
        requests: store.stats_summary(&asked).await?.requests,
        top_songs: store.top_songs(&listened, TOP).await?,
        top_artists: store.top_artists(&listened, TOP).await?,
        months,
        busiest_month,
        first,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years_start_in_order() {
        assert!(start_of(2026) < start_of(2027));
        assert!(start_of(2027) - start_of(2026) >= 365 * 86_400_000 - 3_600_000);
    }
}
