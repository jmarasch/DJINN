//! Radio and learning from the party: what radio draws from (the
//! [`Mixer`], in `smart.rs`, which knows how songs sound), how much each
//! song weighs after what parties did with it, and weighted draws.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use djinn_store::SongHistory;
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

    /// Each song's artists and genres, for learning beyond the song.
    fn links(&self) -> Arc<SongLinks> {
        Arc::default()
    }
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

/// Which artists and genres each song has, so what a party did with one
/// song carries over to others by the same artists or in the same genres.
#[derive(Debug, Default)]
pub struct SongLinks {
    pub artists: HashMap<i64, Vec<i64>>,
    /// Normalized genre names.
    pub genres: HashMap<i64, Vec<String>>,
}

/// What parties of one style liked: a factor per song, artist and genre
/// (above 1 favored, below 1 avoided). Artists and genres pool their
/// songs' history and count for less than a song's own.
#[derive(Debug, Default)]
pub struct Learned {
    pub songs: HashMap<i64, f32>,
    pub artists: HashMap<i64, f32>,
    pub genres: HashMap<String, f32>,
}

/// How much an artist's pooled history counts, as a power of its weight.
const ARTIST_SHARE: f32 = 0.5;
/// The same for a genre, which pools far more songs.
const GENRE_SHARE: f32 = 0.25;

impl Learned {
    pub fn new(history: &[SongHistory], links: &SongLinks) -> Self {
        let mut artists: HashMap<i64, (i64, i64, i64)> = HashMap::new();
        let mut genres: HashMap<&str, (i64, i64, i64)> = HashMap::new();
        let add = |sum: &mut (i64, i64, i64), h: &SongHistory| {
            sum.0 += h.requests;
            sum.1 += h.finished;
            sum.2 += h.skipped;
        };
        for h in history {
            for artist in links.artists.get(&h.track_id).into_iter().flatten() {
                add(artists.entry(*artist).or_default(), h);
            }
            for genre in links.genres.get(&h.track_id).into_iter().flatten() {
                add(genres.entry(genre.as_str()).or_default(), h);
            }
        }
        let pooled = |(r, f, s): (i64, i64, i64), share: f32| party_weight(r, f, s).powf(share);
        Self {
            songs: history
                .iter()
                .map(|h| (h.track_id, party_weight(h.requests, h.finished, h.skipped)))
                .collect(),
            artists: artists
                .into_iter()
                .map(|(id, sum)| (id, pooled(sum, ARTIST_SHARE)))
                .collect(),
            genres: genres
                .into_iter()
                .map(|(name, sum)| (name.to_owned(), pooled(sum, GENRE_SHARE)))
                .collect(),
        }
    }

    /// A song's weight: its own factor times its artists' and genres'
    /// (each averaged when it has several), at `strength` (0 ignores the
    /// party, 1 is full effect).
    pub fn weight(&self, id: i64, links: &SongLinks, strength: f32) -> f32 {
        let song = self.songs.get(&id).copied().unwrap_or(1.0);
        let artists = mean_of(links.artists.get(&id), |a| self.artists.get(a));
        let genres = mean_of(links.genres.get(&id), |g| self.genres.get(g));
        (song * artists * genres)
            .clamp(0.1, 8.0)
            .powf(strength.clamp(0.0, 1.0))
    }

    /// Every song's weight that isn't 1, for the weighted draws.
    pub fn weights(&self, links: &SongLinks, strength: f32) -> HashMap<i64, f32> {
        let ids: HashSet<i64> = self
            .songs
            .keys()
            .chain(links.artists.keys())
            .chain(links.genres.keys())
            .copied()
            .collect();
        ids.into_iter()
            .map(|id| (id, self.weight(id, links, strength)))
            .filter(|(_, w)| (w - 1.0).abs() > 1e-3)
            .collect()
    }
}

/// Geometric mean of the factors `factor` finds for `keys` (1 for none).
fn mean_of<'a, K>(keys: Option<&Vec<K>>, factor: impl Fn(&K) -> Option<&'a f32>) -> f32 {
    let logs: Vec<f32> = keys
        .into_iter()
        .flatten()
        .map(|k| factor(k).copied().unwrap_or(1.0).ln())
        .collect();
    if logs.is_empty() {
        1.0
    } else {
        (logs.iter().sum::<f32>() / logs.len() as f32).exp()
    }
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

    fn history(track_id: i64, requests: i64, finished: i64, skipped: i64) -> SongHistory {
        SongHistory {
            track_id,
            requests,
            finished,
            skipped,
        }
    }

    /// Songs 1 and 2 by artist 10 (rock), song 3 by artist 20 (rock, jazz),
    /// song 4 by artist 30 (polka).
    fn links() -> SongLinks {
        SongLinks {
            artists: [(1, vec![10]), (2, vec![10]), (3, vec![20]), (4, vec![30])]
                .into_iter()
                .collect(),
            genres: [
                (1, vec!["rock".to_owned()]),
                (2, vec!["rock".to_owned()]),
                (3, vec!["rock".to_owned(), "jazz".to_owned()]),
                (4, vec!["polka".to_owned()]),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn learning_reaches_artists_and_genres() {
        let links = links();
        let learned = Learned::new(&[history(1, 4, 4, 0), history(4, 0, 0, 3)], &links);

        let requested = learned.weight(1, &links, 1.0);
        let same_artist = learned.weight(2, &links, 1.0);
        let same_genre = learned.weight(3, &links, 1.0);
        let skipped = learned.weight(4, &links, 1.0);
        assert!(requested > same_artist, "the song itself counts most");
        assert!(same_artist > same_genre, "then its artist");
        assert!(same_genre > 1.0, "then its genre");
        assert!(skipped < 0.5, "skipped polka sinks");
    }

    #[test]
    fn strength_scales_what_was_learned() {
        let links = links();
        let learned = Learned::new(&[history(1, 4, 4, 0)], &links);
        assert!((learned.weight(1, &links, 0.0) - 1.0).abs() < 1e-6, "off");
        assert!(learned.weight(1, &links, 0.5) < learned.weight(1, &links, 1.0));
        assert!(learned.weight(1, &links, 0.5) > 1.0);
    }

    #[test]
    fn weights_cover_songs_never_played() {
        let links = links();
        let learned = Learned::new(&[history(1, 4, 4, 0)], &links);
        let weights = learned.weights(&links, 1.0);
        assert!(weights.contains_key(&2), "same artist, never played");
        assert!(!weights.contains_key(&4), "nothing learned about polka");
        assert!(Learned::default().weights(&links, 1.0).is_empty());
    }
}
