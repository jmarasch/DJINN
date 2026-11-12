//! The mood dial: everyone at the party can nudge the auto mix from chill
//! (-1) to hype (1). Each person's latest setting counts for a while; the
//! party schedule can set where the dial rests, which counts like a couple
//! of people. The auto mix then favors songs whose energy fits (see
//! `mix::mood_weight`); in-order playlists keep their order.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::mix::SongLinks;
use super::Jukebox;
use crate::events::ServerEvent;
use crate::settings;

/// Setting: 1 lets everyone use the mood dial (on unless turned off).
pub const MOOD_DIAL: &str = "mood_dial";
/// Setting: how much the dial moves the odds, 0 to 100 (percent).
pub const MOOD_STRENGTH: &str = "mood_strength";
pub const DEFAULT_MOOD_STRENGTH: i64 = 60;
/// How long one person's setting counts.
const MOOD_MEMORY: Duration = Duration::from_secs(40 * 60);
/// The resting mood (from the schedule) counts like this many people.
const BASE_VOICES: f32 = 2.0;
/// The songs planned ahead are drawn again once the mood moves this far.
const PLAN_AGAIN_AT: f32 = 0.1;

/// Where the dial is, from everyone's settings.
#[derive(Debug, Default)]
pub(super) struct Mood {
    /// Each person's setting and when they made it.
    votes: HashMap<i64, (f32, Instant)>,
    /// Where it rests with nobody's settings (the schedule's).
    base: f32,
    /// The mood the songs planned ahead were drawn for.
    planned_for: f32,
}

impl Mood {
    fn forget_old(&mut self, now: Instant) {
        self.votes
            .retain(|_, (_, at)| now.saturating_duration_since(*at) < MOOD_MEMORY);
    }

    /// The resting mood and everyone's settings, averaged.
    pub(super) fn current(&self, now: Instant) -> f32 {
        let fresh: Vec<f32> = self
            .votes
            .values()
            .filter(|(_, at)| now.saturating_duration_since(*at) < MOOD_MEMORY)
            .map(|(value, _)| *value)
            .collect();
        let sum = self.base * BASE_VOICES + fresh.iter().sum::<f32>();
        (sum / (BASE_VOICES + fresh.len() as f32)).clamp(-1.0, 1.0)
    }

    fn vote(&mut self, user: i64, value: f32, now: Instant) {
        self.forget_old(now);
        self.votes.insert(user, (value.clamp(-1.0, 1.0), now));
    }

    fn rest_at(&mut self, base: f32) {
        self.votes.clear();
        self.base = base.clamp(-1.0, 1.0);
    }
}

/// The dial as screens show it.
#[derive(Clone, Debug, Default, Serialize)]
pub struct MoodState {
    /// The dial is in use (Admin > Smart play).
    pub on: bool,
    /// Where it is, -1 chill to 1 hype.
    pub value: f32,
    /// Where it rests without anyone's settings.
    pub base: f32,
    /// Each person's setting, by user id, so screens can show their own.
    pub votes: HashMap<i64, f32>,
    /// Songs have been listened to, so there's something to go by.
    pub works: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MoodError {
    #[error("the mood dial is off")]
    Off,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Jukebox {
    pub fn mood_state(&self) -> MoodState {
        let now = Instant::now();
        MoodState {
            on: self.mood_on,
            value: self.mood.current(now),
            base: self.mood.base,
            votes: self
                .mood
                .votes
                .iter()
                .filter(|(_, (_, at))| now.saturating_duration_since(*at) < MOOD_MEMORY)
                .map(|(&user, &(value, _))| (user, value))
                .collect(),
            works: !self.links().energy.is_empty(),
        }
    }

    /// Turns the dial on or off (Admin > Smart play).
    pub async fn set_mood_on(&mut self, on: bool) -> anyhow::Result<()> {
        self.mood_on = on;
        self.mood_moved().await
    }

    /// One person's setting, -1 chill to 1 hype.
    pub async fn vote_mood(&mut self, user: i64, value: f32) -> Result<MoodState, MoodError> {
        if !self.mood_on {
            return Err(MoodError::Off);
        }
        self.mood.vote(user, value, Instant::now());
        self.mood_moved().await?;
        Ok(self.mood_state())
    }

    /// Where the dial rests (the party schedule); everyone's settings start
    /// over from there.
    pub async fn set_mood_base(&mut self, base: f32) -> anyhow::Result<()> {
        self.mood.rest_at(base);
        self.mood_moved().await
    }

    /// Draws the songs planned ahead again when the mood moved enough, and
    /// tells the screens.
    async fn mood_moved(&mut self) -> anyhow::Result<()> {
        let now = self.mood_lean_value(Instant::now());
        if (now - self.mood.planned_for).abs() >= PLAN_AGAIN_AT {
            self.mood.planned_for = now;
            self.clear_plan();
            self.top_up_plan().await?;
            self.broadcast_now_playing();
        }
        self.send(ServerEvent::Mood(self.mood_state()));
        Ok(())
    }

    /// The mood the auto mix goes by: 0 with the dial off.
    fn mood_lean_value(&self, now: Instant) -> f32 {
        if self.mood_on {
            self.mood.current(now)
        } else {
            0.0
        }
    }

    /// What the auto mix leans by: the mood, its strength (0 to 1) and each
    /// song's energy. None when the dial is off, centered or has nothing to
    /// go by.
    pub(super) async fn mood_lean(&self) -> anyhow::Result<Option<(f32, f32, Arc<SongLinks>)>> {
        let mood = self.mood_lean_value(Instant::now());
        if mood.abs() < 0.01 {
            return Ok(None);
        }
        let links = self.links();
        if links.energy.is_empty() {
            return Ok(None);
        }
        let strength = settings::number(&self.store, MOOD_STRENGTH, DEFAULT_MOOD_STRENGTH)
            .await?
            .clamp(0, 100) as f32
            / 100.0;
        Ok(Some((mood, strength, links)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_average_with_the_resting_mood() {
        let now = Instant::now();
        let mut mood = Mood::default();
        assert_eq!(mood.current(now), 0.0);
        mood.vote(1, 1.0, now);
        assert!(
            (mood.current(now) - 1.0 / 3.0).abs() < 1e-6,
            "one person nudges"
        );
        mood.vote(2, 1.0, now);
        mood.vote(3, 1.0, now);
        mood.vote(4, 1.0, now);
        assert!(mood.current(now) > 0.6, "a crowd moves it far");
        mood.vote(1, 9.0, now);
        assert!(mood.current(now) <= 1.0, "settings are clamped");

        mood.rest_at(-0.5);
        assert_eq!(mood.current(now), -0.5, "the schedule starts everyone over");
        mood.vote(5, 0.5, now);
        assert!((mood.current(now) - (-1.0 + 0.5) / 3.0).abs() < 1e-6);
    }

    #[test]
    fn old_settings_stop_counting() {
        let then = Instant::now();
        let later = then + MOOD_MEMORY + Duration::from_secs(1);
        let mut mood = Mood::default();
        mood.vote(1, 1.0, then);
        assert_eq!(mood.current(later), 0.0);
        mood.vote(2, -1.0, later);
        assert_eq!(mood.votes.len(), 1, "forgotten when someone sets it again");
    }
}
