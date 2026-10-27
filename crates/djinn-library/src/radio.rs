//! Radio: an endless mix that sounds like a song, an artist, or a genre.
//!
//! [`rank`] picks the songs radio draws from next, each with a weight: the
//! closest in sound to the seed, a little pulled toward what just played
//! all the way through (so it drifts the way the party goes), away from
//! songs like the ones people skipped, favoring songs that share the
//! seed's genres and styles, and spread over artists.

use std::collections::{HashMap, HashSet};

use crate::sound::SoundIndex;

/// Where a radio starts from.
#[derive(Clone, Debug, Default)]
pub struct Seeds {
    /// Songs it should sound like.
    pub tracks: Vec<i64>,
    /// Only these songs may play (a genre's), if set.
    pub only: Option<HashSet<i64>>,
    /// Genres and styles the seed has, normalized.
    pub genres: HashSet<String>,
}

/// What the party did since the radio started, newest first.
#[derive(Clone, Debug, Default)]
pub struct Session {
    /// Songs that played to the end.
    pub played: Vec<i64>,
    /// Songs that were skipped.
    pub skipped: Vec<i64>,
}

/// How many recent songs pull the radio along, and how hard each pulls
/// (the seed weighs 1 in all).
const DRIFT_SONGS: usize = 3;
const DRIFT_WEIGHT: f32 = 0.15;
/// Songs this close to a skipped one count as "like it".
const LIKE_SKIPPED: usize = 10;
const SKIPPED_FACTOR: f32 = 0.25;
/// Each further song by the same artist weighs this much less.
const SAME_ARTIST_FACTOR: f32 = 0.7;
/// Every this many places down the list, a song's weight halves, so the
/// closest few dozen songs make up most of the radio.
const HALF_RANK: f32 = 12.0;
/// Songs whose tags share nothing with the seed's genres weigh this much.
const OFF_GENRE_FACTOR: f32 = 0.4;

/// Up to `limit` songs for the radio, best first, with weights for a
/// weighted draw. `tags` holds each song's genres and styles.
pub fn rank(
    index: &SoundIndex,
    seeds: &Seeds,
    tags: &HashMap<i64, Vec<String>>,
    session: &Session,
    limit: usize,
) -> Vec<(i64, f32)> {
    let skipped: HashSet<i64> = session.skipped.iter().copied().collect();
    // Songs that just played are held back by the draw, so the seeds
    // themselves can stay (artist radio plays the artist).
    let allowed = |id: i64| {
        !skipped.contains(&id) && seeds.only.as_ref().is_none_or(|only| only.contains(&id))
    };

    let share = 1.0 / seeds.tracks.len().max(1) as f32;
    let mut points: Vec<(i64, f32)> = seeds.tracks.iter().map(|&t| (t, share)).collect();
    points.extend(
        session
            .played
            .iter()
            .take(DRIFT_SONGS)
            .map(|&t| (t, DRIFT_WEIGHT)),
    );
    let mut near: Vec<(i64, Option<i64>)> = index
        .around(&points)
        .into_iter()
        .filter(|n| allowed(n.track_id))
        .map(|n| (n.track_id, n.artist))
        .collect();
    // Songs not analyzed yet (or no sound to go by): the allowed ones
    // after the analyzed ones, best tag match first.
    let placed: HashSet<i64> = near.iter().map(|(id, _)| *id).collect();
    let mut rest: Vec<(usize, i64)> = match &seeds.only {
        Some(only) => only.iter().copied().collect::<Vec<_>>(),
        None if near.is_empty() => tags.keys().copied().collect(),
        None => Vec::new(),
    }
    .into_iter()
    .filter(|id| allowed(*id) && !placed.contains(id))
    .map(|id| (overlap_count(tags.get(&id), &seeds.genres), id))
    .filter(|(shared, _)| seeds.only.is_some() || *shared > 0)
    .collect();
    rest.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    near.extend(rest.into_iter().map(|(_, id)| (id, None)));

    let like_skipped: HashSet<i64> = session
        .skipped
        .iter()
        .flat_map(|&k| {
            index
                .around(&[(k, 1.0)])
                .into_iter()
                .take(LIKE_SKIPPED)
                .map(|n| n.track_id)
        })
        .collect();

    let mut by_artist: HashMap<i64, i32> = HashMap::new();
    let mut weighted: Vec<(i64, f32)> = near
        .into_iter()
        .enumerate()
        .map(|(rank, (id, artist))| {
            let mut weight = 0.5f32.powf(rank as f32 / HALF_RANK);
            let has_tags = tags.get(&id).is_some_and(|t| !t.is_empty());
            if !seeds.genres.is_empty() && has_tags {
                let shared = overlap_count(tags.get(&id), &seeds.genres) as f32;
                weight *= if shared > 0.0 {
                    1.0 + 0.6 * shared / seeds.genres.len() as f32
                } else {
                    OFF_GENRE_FACTOR
                };
            }
            if like_skipped.contains(&id) {
                weight *= SKIPPED_FACTOR;
            }
            if let Some(artist) = artist {
                let seen = by_artist.entry(artist).or_default();
                weight *= SAME_ARTIST_FACTOR.powi(*seen);
                *seen += 1;
            }
            (id, weight)
        })
        .collect();
    weighted.sort_by(|a, b| b.1.total_cmp(&a.1));
    weighted.truncate(limit);
    weighted
}

fn overlap_count(tags: Option<&Vec<String>>, genres: &HashSet<String>) -> usize {
    tags.map_or(0, |t| t.iter().filter(|g| genres.contains(*g)).count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use djinn_store::SoundRow;

    /// Songs on a line by tempo: 1 is next to 2, far from 9.
    fn index() -> SoundIndex {
        let rows = (1..=9)
            .map(|id| {
                let mut features = vec![0.0; 37];
                features[0] = id as f32;
                features[5] = (id % 3) as f32 * 0.01;
                SoundRow {
                    track_id: id,
                    album_id: Some(id),
                    artist_ids: vec![if id <= 4 { 100 } else { id }],
                    title: format!("Song {id}"),
                    features,
                }
            })
            .collect();
        SoundIndex::build(rows)
    }

    fn ids(ranked: &[(i64, f32)]) -> Vec<i64> {
        ranked.iter().map(|(id, _)| *id).collect()
    }

    #[test]
    fn plays_what_sounds_like_the_seed() {
        let seeds = Seeds {
            tracks: vec![1],
            ..Seeds::default()
        };
        let ranked = rank(&index(), &seeds, &HashMap::new(), &Session::default(), 5);
        assert_eq!(ranked.len(), 5);
        assert_eq!(ranked[0].0, 1, "the seed sounds most like itself");
        assert!(!ids(&ranked).contains(&9), "the farthest song falls off");
    }

    #[test]
    fn spreads_artists_and_steers_away_from_skips() {
        let seeds = Seeds {
            tracks: vec![1],
            ..Seeds::default()
        };
        let calm = rank(&index(), &seeds, &HashMap::new(), &Session::default(), 20);
        let weight = |r: &[(i64, f32)], id| r.iter().find(|x| x.0 == id).map(|x| x.1);
        // 2, 3 and 4 share an artist: 4 gets pushed below 5's neighbor rank.
        assert!(weight(&calm, 4) < weight(&calm, 5));

        let skipped = Session {
            skipped: vec![3],
            ..Session::default()
        };
        let after = rank(&index(), &seeds, &HashMap::new(), &skipped, 20);
        assert!(!ids(&after).contains(&3), "skipped songs don't come back");
        assert!(
            weight(&after, 4) < weight(&calm, 4),
            "songs like the skipped one weigh less"
        );
    }

    #[test]
    fn songs_outside_the_seeds_genres_weigh_less() {
        let tags: HashMap<i64, Vec<String>> = [
            (5, vec!["dance pop".to_owned()]),
            (6, vec!["gothic rock".to_owned()]),
        ]
        .into_iter()
        .collect();
        let seeds = Seeds {
            tracks: vec![1],
            only: None,
            genres: ["dance pop".to_owned()].into_iter().collect(),
        };
        let ranked = rank(&index(), &seeds, &tags, &Session::default(), 20);
        let weight = |id| ranked.iter().find(|x| x.0 == id).map_or(0.0, |x| x.1);
        assert!(
            weight(6) < weight(5) * 0.5,
            "6 is right next to 5 but off genre"
        );
        assert!(weight(7) > weight(6), "untagged songs aren't held against");
    }

    #[test]
    fn genre_radio_stays_in_the_genre() {
        let tags: HashMap<i64, Vec<String>> = [
            (2, vec!["house".to_owned()]),
            (8, vec!["house".to_owned()]),
            (20, vec!["house".to_owned()]),
        ]
        .into_iter()
        .collect();
        let seeds = Seeds {
            tracks: vec![2, 8],
            only: Some([2, 8, 20].into_iter().collect()),
            genres: ["house".to_owned()].into_iter().collect(),
        };
        // Only the genre's songs, the unanalyzed one too.
        let ranked = rank(&index(), &seeds, &tags, &Session::default(), 10);
        let mut got = ids(&ranked);
        got.sort();
        assert_eq!(got, [2, 8, 20]);
    }
}
