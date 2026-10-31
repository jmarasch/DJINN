//! When the heavy background work (sound analysis and AI listening) may
//! run: any time, only while no music plays, or only between set hours.
//! The work waits its turn between batches, so it pauses within a few
//! seconds.

use std::future::Future;
use std::time::Duration;

use chrono::Timelike;
use serde::{Deserialize, Serialize};

use crate::events::PlayState;
use crate::state::AppState;

/// Setting key: "any", "idle" (no music playing) or "hours".
pub const WHEN: &str = "background_when";
/// Setting keys: the hours (0 to 23, this computer's time) work may run
/// from and until, with `WHEN` "hours". From 22 until 6 runs overnight.
pub const FROM: &str = "background_from";
pub const UNTIL: &str = "background_until";
pub const DEFAULT_FROM: u8 = 1;
pub const DEFAULT_UNTIL: u8 = 7;

/// How often waiting work checks again.
const CHECK: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    #[default]
    Any,
    Idle,
    Hours,
}

/// Why background work is waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "lowercase")]
pub enum Held {
    /// Music is playing.
    Music,
    /// Outside the hours it may run.
    Hours { from: u8, until: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub when: When,
    pub from: u8,
    pub until: u8,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            when: When::Any,
            from: DEFAULT_FROM,
            until: DEFAULT_UNTIL,
        }
    }
}

impl Schedule {
    pub async fn load(store: &djinn_store::Store) -> anyhow::Result<Self> {
        let when = match store.get_setting(WHEN).await?.as_deref() {
            Some("idle") => When::Idle,
            Some("hours") => When::Hours,
            _ => When::Any,
        };
        let hour =
            |v: i64, default: u8| u8::try_from(v).ok().filter(|h| *h < 24).unwrap_or(default);
        Ok(Self {
            when,
            from: hour(
                crate::settings::number(store, FROM, DEFAULT_FROM.into()).await?,
                DEFAULT_FROM,
            ),
            until: hour(
                crate::settings::number(store, UNTIL, DEFAULT_UNTIL.into()).await?,
                DEFAULT_UNTIL,
            ),
        })
    }

    pub async fn save(&self, store: &djinn_store::Store) -> anyhow::Result<()> {
        let when = match self.when {
            When::Any => "any",
            When::Idle => "idle",
            When::Hours => "hours",
        };
        store.set_setting(WHEN, when).await?;
        store.set_setting(FROM, &self.from.to_string()).await?;
        store.set_setting(UNTIL, &self.until.to_string()).await?;
        Ok(())
    }

    /// Why work can't run now, given whether music plays and the hour.
    fn held(&self, playing: bool, hour: u8) -> Option<Held> {
        match self.when {
            When::Any => None,
            When::Idle => playing.then_some(Held::Music),
            When::Hours => (!in_hours(hour, self.from, self.until)).then_some(Held::Hours {
                from: self.from,
                until: self.until,
            }),
        }
    }
}

/// Whether `hour` falls from `from` until (not including) `until`; the
/// same hour for both means all day.
fn in_hours(hour: u8, from: u8, until: u8) -> bool {
    if from == until {
        true
    } else if from < until {
        (from..until).contains(&hour)
    } else {
        hour >= from || hour < until
    }
}

/// Why background work can't run right now, if it can't.
pub async fn held(state: &AppState) -> Option<Held> {
    let schedule = Schedule::load(&state.store).await.unwrap_or_default();
    if schedule.when == When::Any {
        return None;
    }
    let playing = state.jukebox.lock().await.player_state().state == PlayState::Playing;
    let hour = u8::try_from(chrono::Local::now().hour()).unwrap_or(0);
    schedule.held(playing, hour)
}

/// What the work in progress is waiting for, for the progress display.
pub fn waiting(state: &AppState) -> Option<Held> {
    state.scan.held.lock().ok().and_then(|h| *h)
}

/// Waits until background work may run. `wanted` is asked again while
/// waiting; false (switched off or stopped meanwhile) gives up and returns
/// false. `changed` is called when the reason to wait changes, to update
/// what the Library page shows.
pub async fn wait_turn<W, WF, C, CF>(state: &AppState, wanted: W, changed: C) -> bool
where
    W: Fn() -> WF,
    WF: Future<Output = bool>,
    C: Fn() -> CF,
    CF: Future<Output = ()>,
{
    loop {
        let now = held(state).await;
        let before = state
            .scan
            .held
            .lock()
            .map(|mut h| std::mem::replace(&mut *h, now))
            .unwrap_or(None);
        if before != now {
            changed().await;
        }
        if now.is_none() {
            return true;
        }
        if !wanted().await {
            if let Ok(mut h) = state.scan.held.lock() {
                *h = None;
            }
            changed().await;
            return false;
        }
        tokio::time::sleep(CHECK).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_wrap_past_midnight() {
        assert!(in_hours(1, 1, 7));
        assert!(in_hours(6, 1, 7));
        assert!(!in_hours(7, 1, 7), "until is not included");
        assert!(!in_hours(0, 1, 7));
        assert!(in_hours(23, 22, 6));
        assert!(in_hours(3, 22, 6));
        assert!(!in_hours(12, 22, 6));
        assert!(in_hours(12, 5, 5), "same hour means all day");
    }

    #[test]
    fn holds_for_the_right_reason() {
        let any = Schedule::default();
        assert_eq!(any.held(true, 12), None);
        let idle = Schedule {
            when: When::Idle,
            ..any
        };
        assert_eq!(idle.held(true, 3), Some(Held::Music));
        assert_eq!(idle.held(false, 12), None);
        let hours = Schedule {
            when: When::Hours,
            ..any
        };
        assert_eq!(hours.held(false, 3), None);
        assert_eq!(
            hours.held(true, 3),
            None,
            "music doesn't matter by the hour"
        );
        assert_eq!(
            hours.held(false, 12),
            Some(Held::Hours { from: 1, until: 7 })
        );
    }
}
