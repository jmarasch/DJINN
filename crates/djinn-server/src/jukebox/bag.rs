//! Shuffle bag for the auto pool: shuffle everything, play through it in
//! order, reshuffle when it runs out, and skip anything played recently.
//! With party weights, each cycle is shuffled so favored songs tend to come
//! early and avoided ones late, but every song still plays once a cycle.

use std::collections::{HashMap, HashSet};

use rand::seq::SliceRandom;
use rand::{Rng, RngExt};

/// With the mood dial turned, the next song is picked from this many at
/// the front of the cycle, so the mood reorders what's near without
/// keeping any song out of its cycle.
const LEAN_AHEAD: usize = 8;

#[derive(Debug, Default)]
pub struct ShuffleBag {
    /// Tracks left this cycle, next one last (so drawing pops from the end).
    remaining: Vec<i64>,
    /// Tracks already drawn this cycle.
    drawn: HashSet<i64>,
    /// Party weights for the next cycle's order; empty for an even shuffle.
    weights: HashMap<i64, f32>,
}

impl ShuffleBag {
    /// Draws the next track from `pool` (the pool's current track ids),
    /// avoiding the `window` most recent entries of `recent` (newest first).
    ///
    /// The window shrinks to `pool.len() - 1` so a small pool never stalls,
    /// and tracks that joined the pool mid-cycle are mixed into the rest of
    /// the cycle.
    #[cfg(test)]
    pub fn draw(
        &mut self,
        pool: &[i64],
        recent: &[i64],
        window: usize,
        rng: &mut impl Rng,
    ) -> Option<i64> {
        self.draw_leaning(pool, recent, window, None, rng)
    }

    /// Like `draw`, but with `lean` (a weight per song) the next
    /// song is picked by weight from the few at the front of the cycle.
    pub fn draw_leaning(
        &mut self,
        pool: &[i64],
        recent: &[i64],
        window: usize,
        lean: Option<&dyn Fn(i64) -> f32>,
        rng: &mut impl Rng,
    ) -> Option<i64> {
        if pool.is_empty() {
            return None;
        }
        let pool_set: HashSet<i64> = pool.iter().copied().collect();
        self.remaining.retain(|id| pool_set.contains(id));
        self.drawn.retain(|id| pool_set.contains(id));
        if self.remaining.is_empty() && self.drawn.is_empty() {
            self.new_cycle(pool, rng);
        }
        let queued: HashSet<i64> = self.remaining.iter().copied().collect();
        for &id in pool {
            if !queued.contains(&id) && !self.drawn.contains(&id) {
                let at = rng.random_range(0..=self.remaining.len());
                self.remaining.insert(at, id);
            }
        }

        let window = window.min(pool.len() - 1);
        let recent: HashSet<i64> = recent.iter().take(window).copied().collect();
        if self.remaining.is_empty() {
            self.new_cycle(pool, rng);
        }
        if let Some(id) = self.take_next(&recent, lean, rng) {
            return Some(id);
        }
        // Everything left this cycle was played recently (through requests,
        // say). Start the next cycle early.
        self.new_cycle(pool, rng);
        self.take_next(&recent, lean, rng)
            .or_else(|| self.remaining.pop())
    }

    /// Weights the order of cycles from the next one on.
    pub fn set_weights(&mut self, weights: HashMap<i64, f32>) {
        self.weights = weights;
    }

    /// Puts back a drawn track that ended up not playing, so it comes next.
    pub fn put_back(&mut self, id: i64) {
        if self.drawn.remove(&id) {
            self.remaining.push(id);
        }
    }

    fn new_cycle(&mut self, pool: &[i64], rng: &mut impl Rng) {
        self.drawn.clear();
        self.remaining = pool.to_vec();
        if self.weights.is_empty() {
            self.remaining.shuffle(rng);
            return;
        }
        // Weighted shuffle (Efraimidis-Spirakis): each song's key is
        // u^(1/weight), and higher keys come first. Draws pop from the end.
        let mut keyed: Vec<(f32, i64)> = self
            .remaining
            .iter()
            .map(|&id| {
                let w = self.weights.get(&id).copied().unwrap_or(1.0).max(1e-3);
                (rng.random::<f32>().powf(1.0 / w), id)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.remaining = keyed.into_iter().map(|(_, id)| id).collect();
    }

    /// The next song not in `exclude`: the first, or with `lean` one of
    /// the first few by weight.
    fn take_next(
        &mut self,
        exclude: &HashSet<i64>,
        lean: Option<&dyn Fn(i64) -> f32>,
        rng: &mut impl Rng,
    ) -> Option<i64> {
        let first = self
            .remaining
            .iter()
            .rposition(|id| !exclude.contains(id))?;
        let index = match lean {
            None => first,
            Some(weigh) => {
                let near: Vec<(usize, f32)> = self
                    .remaining
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, id)| !exclude.contains(id))
                    .take(LEAN_AHEAD)
                    .map(|(i, &id)| (i, weigh(id).max(0.0)))
                    .collect();
                let total: f32 = near.iter().map(|(_, w)| w).sum();
                if total > 0.0 {
                    let mut at = rng.random::<f32>() * total;
                    near.iter()
                        .find(|(_, w)| {
                            at -= w;
                            at < 0.0
                        })
                        .map_or(first, |(i, _)| *i)
                } else {
                    first
                }
            }
        };
        let id = self.remaining.remove(index);
        self.drawn.insert(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(7)
    }

    #[test]
    fn leaning_reorders_but_keeps_the_cycle() {
        let pool: Vec<i64> = (1..=40).collect();
        let lean = |id: i64| if id > 20 { 50.0 } else { 1.0 };
        // Favored songs in the first half of a cycle, over many shuffles.
        let early = |leaning: bool| -> usize {
            (0..40)
                .map(|seed| {
                    let mut bag = ShuffleBag::default();
                    let mut rng = StdRng::seed_from_u64(seed);
                    let weigh: Option<&dyn Fn(i64) -> f32> = leaning.then_some(&lean);
                    let cycle: Vec<i64> = (0..40)
                        .filter_map(|_| bag.draw_leaning(&pool, &[], 0, weigh, &mut rng))
                        .collect();
                    let unique: HashSet<i64> = cycle.iter().copied().collect();
                    assert_eq!(unique.len(), 40, "every song still plays once a cycle");
                    cycle[..20].iter().filter(|&&id| id > 20).count()
                })
                .sum()
        };
        let (plain, leaning) = (early(false), early(true));
        assert!(
            leaning * 10 > plain * 12,
            "favored songs come earlier: {leaning} against {plain}"
        );
    }

    #[test]
    fn plays_every_track_once_per_cycle() {
        let pool: Vec<i64> = (1..=20).collect();
        let mut bag = ShuffleBag::default();
        let mut rng = rng();
        let mut history = Vec::new();
        for _ in 0..3 {
            let mut cycle: Vec<i64> = (0..20)
                .map(|_| {
                    let id = bag.draw(&pool, &history, 0, &mut rng).unwrap();
                    history.insert(0, id);
                    id
                })
                .collect();
            cycle.sort_unstable();
            assert_eq!(cycle, pool);
        }
    }

    #[test]
    fn respects_history_window_across_cycles() {
        let pool: Vec<i64> = (1..=30).collect();
        let mut bag = ShuffleBag::default();
        let mut rng = rng();
        let mut history: Vec<i64> = Vec::new();
        for _ in 0..300 {
            let id = bag.draw(&pool, &history, 25, &mut rng).unwrap();
            assert!(
                !history.iter().take(25).any(|&h| h == id),
                "{id} repeated too soon"
            );
            history.insert(0, id);
        }
    }

    #[test]
    fn small_pool_never_stalls() {
        // Default window (25) is larger than the pool.
        let pool = [1, 2, 3];
        let mut bag = ShuffleBag::default();
        let mut rng = rng();
        let mut history: Vec<i64> = Vec::new();
        for _ in 0..30 {
            let id = bag.draw(&pool, &history, 25, &mut rng).unwrap();
            // With the window shrunk to 2, the last two never repeat.
            assert!(!history.iter().take(2).any(|&h| h == id));
            history.insert(0, id);
        }
        let single = bag.draw(&[9], &[9], 25, &mut rng);
        assert_eq!(single, Some(9));
    }

    #[test]
    fn skips_tracks_played_through_requests() {
        let pool: Vec<i64> = (1..=10).collect();
        let mut bag = ShuffleBag::default();
        let mut rng = rng();
        // Tracks 1..=5 were just requested and played.
        let history = [5, 4, 3, 2, 1];
        for _ in 0..5 {
            let id = bag.draw(&pool, &history, 5, &mut rng).unwrap();
            assert!(id > 5);
        }
    }

    #[test]
    fn follows_pool_changes() {
        let mut bag = ShuffleBag::default();
        let mut rng = rng();
        let first = bag.draw(&[1, 2, 3], &[], 0, &mut rng).unwrap();
        // Track 4 joins, and the drawn one leaves the pool.
        let pool: Vec<i64> = [1, 2, 3, 4].into_iter().filter(|&i| i != first).collect();
        let mut rest: Vec<i64> = (0..3)
            .map(|_| bag.draw(&pool, &[], 0, &mut rng).unwrap())
            .collect();
        rest.sort_unstable();
        assert_eq!(rest, pool);
        assert_eq!(bag.draw(&[], &[], 0, &mut rng), None);
    }

    #[test]
    fn weights_order_the_cycle_but_everything_plays() {
        let pool: Vec<i64> = (1..=10).collect();
        let mut rng = rng();
        let mut favored_first = 0;
        for _ in 0..200 {
            let mut bag = ShuffleBag::default();
            bag.set_weights([(1, 5.0), (2, 0.15)].into_iter().collect());
            let mut cycle: Vec<i64> = (0..10)
                .map(|_| bag.draw(&pool, &[], 0, &mut rng).unwrap())
                .collect();
            let at = |id| cycle.iter().position(|&x| x == id).unwrap();
            if at(1) < at(2) {
                favored_first += 1;
            }
            cycle.sort_unstable();
            assert_eq!(cycle, pool, "every song once a cycle");
        }
        assert!(
            favored_first > 180,
            "favored before avoided: {favored_first}"
        );
    }

    #[test]
    fn put_back_comes_next() {
        let pool: Vec<i64> = (1..=10).collect();
        let mut bag = ShuffleBag::default();
        let mut rng = rng();
        let id = bag.draw(&pool, &[], 0, &mut rng).unwrap();
        bag.put_back(id);
        assert_eq!(bag.draw(&pool, &[], 0, &mut rng), Some(id));
    }
}
