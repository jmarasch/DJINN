//! Radio and learning from the party: what radio draws from (the
//! [`Mixer`], in `smart.rs`, which knows how songs sound), how much each
//! song weighs after what parties did with it, and weighted draws.

use std::collections::{HashMap, HashSet};

use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

/// Where a radio starts from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RadioSeed {
    Track { id: i64 },
    Artist { id: i64 },
    Genre { name: String },
}

/// The radio playing, as saved in settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Radio {
    pub seed: RadioSeed,
    /// What it's named after ("Taped Up Heart", "KREAM", "house").
    pub name: String,
    /// Unix ms; only what played since counts as this radio's session.
    pub started_at: i64,
}

/// Picks the songs radio draws from.
pub trait Mixer: Send + Sync {
    /// Songs for `seed`, best first, with weights. `played` and `skipped`
    /// are this radio's songs so far, newest first.
    fn radio(&self, seed: &RadioSeed, played: &[i64], skipped: &[i64]) -> Vec<(i64, f32)>;
}

/// How much a song weighs in wild play and radio after what parties did
/// with it: requested songs more (up to 5x), often skipped ones less (down
/// to 0.15x). Songs with no history weigh 1.
pub fn party_weight(requests: i64, finished: i64, skipped: i64) -> f32 {
    let asked = 1.0 + 0.4 * requests.clamp(0, 10) as f32;
    let finished = finished.max(0) as f32;
    let kept = (finished + 2.0) / (finished + 2.0 + 1.5 * skipped.max(0) as f32);
    (asked * kept).clamp(0.15, 5.0)
}

/// Draws one song from `pool` (id, weight) by weight, leaving out
/// `recent` unless nothing else is left.
pub fn weighted_draw(
    pool: &[(i64, f32)],
    recent: &HashSet<i64>,
    party: &HashMap<i64, f32>,
    rng: &mut impl Rng,
) -> Option<i64> {
    let weigh = |id: i64, w: f32| w.max(0.0) * party.get(&id).copied().unwrap_or(1.0);
    let fresh: Vec<(i64, f32)> = pool
        .iter()
        .filter(|(id, _)| !recent.contains(id))
        .map(|&(id, w)| (id, weigh(id, w)))
        .collect();
    let choices = if fresh.iter().any(|(_, w)| *w > 0.0) {
        fresh
    } else {
        pool.iter()
            .map(|&(id, w)| (id, weigh(id, w).max(1e-6)))
            .collect()
    };
    let total: f32 = choices.iter().map(|(_, w)| w).sum();
    if choices.is_empty() || total <= 0.0 {
        return None;
    }
    let mut at = rng.random::<f32>() * total;
    for &(id, w) in &choices {
        if at < w {
            return Some(id);
        }
        at -= w;
    }
    choices.last().map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn party_weights() {
        assert!((party_weight(0, 0, 0) - 1.0).abs() < 1e-6, "no history");
        assert!(party_weight(3, 0, 0) > 2.0, "requested");
        assert!(party_weight(0, 0, 4) < 0.3, "skipped a lot");
        assert!(
            party_weight(0, 20, 2) > 0.8,
            "a skip or two among many full plays hardly matters"
        );
        assert!((party_weight(100, 0, 0) - 5.0).abs() < 1e-6, "capped");
        assert!((party_weight(0, 0, 1000) - 0.15).abs() < 1e-6, "floored");
    }

    #[test]
    fn draws_by_weight_and_skips_recent() {
        let mut rng = StdRng::seed_from_u64(7);
        let pool = [(1, 1.0), (2, 1.0), (3, 1.0)];
        let party: HashMap<i64, f32> = [(1, 5.0), (2, 0.15)].into_iter().collect();
        let mut counts = HashMap::new();
        for _ in 0..2000 {
            let id = weighted_draw(&pool, &HashSet::new(), &party, &mut rng).unwrap();
            *counts.entry(id).or_insert(0) += 1;
        }
        assert!(counts[&1] > counts[&3] * 3, "requested songs come up more");
        assert!(counts[&3] > counts.get(&2).copied().unwrap_or(0) * 3);

        let recent: HashSet<i64> = [1, 3].into_iter().collect();
        assert_eq!(
            weighted_draw(&pool, &recent, &party, &mut rng),
            Some(2),
            "only 2 is left"
        );
        let all: HashSet<i64> = [1, 2, 3].into_iter().collect();
        assert!(
            weighted_draw(&pool, &all, &party, &mut rng).is_some(),
            "a small pool never stalls"
        );
        assert_eq!(weighted_draw(&[], &all, &party, &mut rng), None);
    }
}
