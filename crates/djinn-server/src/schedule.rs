//! The party schedule (Admin > Smart play): time rules like "a quiet
//! dinner playlist until 9, then wild play turned up". Each rule says on
//! which days and between which times it holds, and what to set when it
//! starts: the auto mix, where the mood dial rests, and the volume. A rule
//! is applied once as it starts (so admins can still change things by
//! hand during it), and a restart doesn't apply it again. When a rule ends
//! with none after it, the music carries on and the dial goes back to the
//! middle. Each room that plays on its own has its own schedule.

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::jukebox::{room_key, Jukebox, WILD_PLAY};
use crate::state::AppState;

/// Setting (per room): the schedule, as JSON ([`Schedule`]).
pub const SCHEDULE: &str = "party_schedule";
/// Setting (per room): the rule occurrence applied last, so a restart
/// doesn't apply it again.
const APPLIED: &str = "party_schedule_applied";
/// How often the schedule is checked.
const EVERY: Duration = Duration::from_secs(20);
/// Most rules in one schedule.
const MAX_RULES: usize = 24;
const MINUTES_A_DAY: u16 = 24 * 60;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// The schedule is followed.
    pub on: bool,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Days it starts on, 0 Monday to 6 Sunday; empty for every day.
    #[serde(default)]
    pub days: Vec<u8>,
    /// Minutes after midnight it starts.
    pub from: u16,
    /// Minutes after midnight it ends; earlier than `from` runs past
    /// midnight, the same as `from` lasts the whole day.
    pub until: u16,
    /// What the auto mix plays.
    #[serde(default)]
    pub play: Play,
    /// Where the mood dial rests, -1 chill to 1 hype; None leaves it in
    /// the middle.
    #[serde(default)]
    pub mood: Option<f32>,
    /// Volume, 0 to 100; None leaves it.
    #[serde(default)]
    pub volume: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Play {
    /// Leave the auto mix as it is.
    #[default]
    Keep,
    Playlist {
        id: i64,
    },
    /// Random songs from the whole library.
    Wild,
}

impl Schedule {
    fn check(&self) -> Result<(), String> {
        if self.rules.len() > MAX_RULES {
            return Err(format!("at most {MAX_RULES} rules"));
        }
        for rule in &self.rules {
            if rule.from >= MINUTES_A_DAY || rule.until >= MINUTES_A_DAY {
                return Err("times are 00:00 to 23:59".into());
            }
            if rule.days.iter().any(|&d| d > 6) {
                return Err("days are 0 (Monday) to 6 (Sunday)".into());
            }
            if rule
                .mood
                .is_some_and(|m| !m.is_finite() || !(-1.0..=1.0).contains(&m))
            {
                return Err("the mood is -1 (chill) to 1 (hype)".into());
            }
            if rule.volume.is_some_and(|v| v > 100) {
                return Err("volume is 0 to 100".into());
            }
        }
        Ok(())
    }
}

/// The rule holding at `now`, and the day its stretch started: of those
/// holding, the one that started last.
fn holding(rules: &[Rule], now: NaiveDateTime) -> Option<(usize, NaiveDate)> {
    let today = now.date();
    let mut best: Option<(usize, NaiveDate, NaiveDateTime)> = None;
    for (i, rule) in rules.iter().enumerate() {
        for day in [today.pred_opt(), Some(today)].into_iter().flatten() {
            let weekday = day.weekday().num_days_from_monday() as u8;
            if !rule.days.is_empty() && !rule.days.contains(&weekday) {
                continue;
            }
            let Some(midnight) = day.and_hms_opt(0, 0, 0) else {
                continue;
            };
            let start = midnight + chrono::Duration::minutes(i64::from(rule.from));
            let length = if rule.until > rule.from {
                rule.until - rule.from
            } else {
                MINUTES_A_DAY - rule.from + rule.until
            };
            let end = start + chrono::Duration::minutes(i64::from(length));
            if start <= now && now < end && best.is_none_or(|(_, _, s)| start > s) {
                best = Some((i, day, start));
            }
        }
    }
    best.map(|(i, day, _)| (i, day))
}

/// Names one stretch of one rule, so it's applied once (and again if the
/// rule is changed meanwhile).
fn stretch_key(rule: &Rule, day: NaiveDate) -> String {
    format!("{day}|{}", serde_json::to_string(rule).unwrap_or_default())
}

pub async fn load(state: &AppState, room: i64) -> anyhow::Result<Schedule> {
    Ok(state
        .store
        .get_setting(&room_key(room, SCHEDULE))
        .await?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// Checks every room's schedule now and then.
pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = follow(&state).await {
                tracing::warn!(error = %e, "party schedule check failed");
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

async fn follow(state: &AppState) -> anyhow::Result<()> {
    let now = chrono::Local::now().naive_local();
    for room in state.store.rooms().await? {
        // A linked room plays its leader's music, so the leader's rules.
        if room.linked_to.is_some() {
            continue;
        }
        let jukebox = state.jukebox_for(room.id);
        // A room whose player isn't running yet would get Main's.
        if room.id != djinn_store::MAIN_ROOM && std::sync::Arc::ptr_eq(&jukebox, &state.jukebox) {
            continue;
        }
        let schedule = load(state, room.id).await?;
        let applied_key = room_key(room.id, APPLIED);
        let applied = state.store.get_setting(&applied_key).await?;
        let holding = schedule.on.then(|| holding(&schedule.rules, now)).flatten();
        match holding {
            Some((i, day)) => {
                let rule = &schedule.rules[i];
                let key = stretch_key(rule, day);
                if applied.as_deref() == Some(key.as_str()) {
                    continue;
                }
                tracing::info!(room = room.id, rule = i, "party schedule: a rule starts");
                apply(state, &mut *jukebox.lock().await, rule).await?;
                state.store.set_setting(&applied_key, &key).await?;
            }
            None if applied.as_deref().is_some_and(|a| !a.is_empty()) => {
                // The rule ended with none after it: the dial goes back to
                // the middle, the music carries on.
                jukebox.lock().await.set_mood_base(0.0).await?;
                state.store.set_setting(&applied_key, "").await?;
            }
            None => {}
        }
    }
    Ok(())
}

async fn apply(state: &AppState, jukebox: &mut Jukebox, rule: &Rule) -> anyhow::Result<()> {
    match rule.play {
        Play::Keep => {}
        Play::Wild => {
            if jukebox.active_playlist() != Some(WILD_PLAY) {
                jukebox.play_playlist(WILD_PLAY, None).await?;
            }
        }
        Play::Playlist { id } => {
            if state.store.playlist_summary(id).await?.is_none() {
                tracing::warn!(playlist = id, "party schedule: that playlist is gone");
            } else if jukebox.active_playlist() != Some(id) {
                jukebox.play_playlist(id, None).await?;
            }
        }
    }
    jukebox.set_mood_base(rule.mood.unwrap_or(0.0)).await?;
    if let Some(volume) = rule.volume {
        jukebox.set_volume(volume).await?;
    }
    Ok(())
}

pub fn router() -> Router<AppState> {
    Router::new().route("/party/schedule", get(read).put(save))
}

#[derive(Deserialize)]
struct RoomQuery {
    room: Option<i64>,
}

#[derive(Serialize)]
struct Overview {
    #[serde(flatten)]
    schedule: Schedule,
    /// The rule holding now (index), if the schedule is on.
    holding: Option<usize>,
}

async fn read(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<RoomQuery>,
) -> ApiResult<Json<Overview>> {
    let room = query.room.unwrap_or(djinn_store::MAIN_ROOM);
    let schedule = load(&state, room).await?;
    let now = chrono::Local::now().naive_local();
    let holding = schedule
        .on
        .then(|| holding(&schedule.rules, now).map(|(i, _)| i))
        .flatten();
    Ok(Json(Overview { schedule, holding }))
}

async fn save(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<RoomQuery>,
    Json(schedule): Json<Schedule>,
) -> ApiResult<StatusCode> {
    let room = query.room.unwrap_or(djinn_store::MAIN_ROOM);
    if state.store.room(room).await?.is_none() {
        return Err(ApiError::not_found());
    }
    schedule.check().map_err(ApiError::bad_request)?;
    state
        .store
        .set_setting(
            &room_key(room, SCHEDULE),
            &serde_json::to_string(&schedule)?,
        )
        .await?;
    // Changes take effect at the next check (a rule changed while it
    // holds is applied again).
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = follow(&state).await {
            tracing::warn!(error = %e, "party schedule check failed");
        }
    });
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        // November 2026: the 9th is a Monday.
        NaiveDate::from_ymd_opt(2026, 11, day)
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .unwrap()
    }

    fn rule(days: &[u8], from: u16, until: u16) -> Rule {
        Rule {
            days: days.to_vec(),
            from,
            until,
            play: Play::Keep,
            mood: None,
            volume: None,
        }
    }

    #[test]
    fn quiet_until_nine_then_dance() {
        let rules = [rule(&[], 18 * 60, 21 * 60), rule(&[], 21 * 60, 2 * 60)];
        assert_eq!(holding(&rules, at(13, 17, 59)), None);
        assert_eq!(holding(&rules, at(13, 18, 0)).map(|h| h.0), Some(0));
        assert_eq!(holding(&rules, at(13, 20, 59)).map(|h| h.0), Some(0));
        assert_eq!(holding(&rules, at(13, 21, 0)).map(|h| h.0), Some(1));
        assert_eq!(
            holding(&rules, at(14, 1, 30)),
            Some((1, NaiveDate::from_ymd_opt(2026, 11, 13).unwrap())),
            "past midnight it's still the evening before"
        );
        assert_eq!(holding(&rules, at(14, 2, 0)), None);
    }

    #[test]
    fn days_count_from_the_start() {
        // Fridays (4) from 22:00 to 03:00.
        let rules = [rule(&[4], 22 * 60, 3 * 60)];
        assert_eq!(holding(&rules, at(12, 23, 0)), None, "Thursday");
        assert!(holding(&rules, at(13, 23, 0)).is_some(), "Friday");
        assert!(holding(&rules, at(14, 2, 0)).is_some(), "early Saturday");
        assert_eq!(holding(&rules, at(14, 23, 0)), None, "Saturday night");
    }

    #[test]
    fn the_latest_start_wins_and_whole_days_hold() {
        let rules = [rule(&[], 0, 0), rule(&[5, 6], 12 * 60, 14 * 60)];
        assert_eq!(holding(&rules, at(11, 3, 0)).map(|h| h.0), Some(0));
        assert_eq!(holding(&rules, at(14, 13, 0)).map(|h| h.0), Some(1));
    }

    #[test]
    fn a_changed_rule_is_another_stretch() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let mut changed = rule(&[], 60, 120);
        let before = stretch_key(&changed, day);
        changed.volume = Some(40);
        assert_ne!(before, stretch_key(&changed, day));
    }

    #[test]
    fn bad_schedules_are_refused() {
        let mut schedule = Schedule {
            on: true,
            rules: vec![rule(&[7], 0, 60)],
        };
        assert!(schedule.check().is_err());
        schedule.rules = vec![rule(&[], 24 * 60, 60)];
        assert!(schedule.check().is_err());
        schedule.rules = vec![Rule {
            mood: Some(2.0),
            ..rule(&[], 0, 60)
        }];
        assert!(schedule.check().is_err());
        schedule.rules = vec![rule(&[0, 6], 0, 60)];
        assert!(schedule.check().is_ok());
    }
}
