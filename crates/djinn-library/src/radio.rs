//! Radio: an endless mix that sounds like a song, an artist, or a genre.
//!
//! [`rank`] picks the songs radio draws from next, each with a weight: the
//! closest in sound to the seed (the analyzer's measurements and, when it's
//! on, what the AI heard), a little pulled toward what just played all the
//! way through (so it drifts the way the party goes), away from songs like
//! the ones people skipped, favoring songs that share the seed's genres
//! (rare ones counting more than "rock") and its era, and songs by the
//! artist it started from, and spread over the other artists.

use std::collections::{HashMap, HashSet};

use crate::sound::SoundIndex;

/// What radio knows about the library.
pub struct Library<'a> {
    pub index: &'a SoundIndex,
    /// Each song's genres: its own, its album's and its artists' (not what
    /// the AI guessed, unless that's all it has).
    pub genres: &'a HashMap<i64, Vec<String>>,
    /// How much sharing each genre says: more for rare ones.
    pub genre_weight: Option<&'a HashMap<String, f32>>,
    /// What the AI heard in each song (unit vectors).
    pub heard: Option<&'a HashMap<i64, Vec<f32>>>,
    /// The year each song belongs to.
    pub eras: Option<&'a HashMap<i64, i64>>,
}

impl<'a> Library<'a> {
    pub fn new(index: &'a SoundIndex, genres: &'a HashMap<i64, Vec<String>>) -> Self {
        Self {
            index,
            genres,
            genre_weight: None,
            heard: None,
            eras: None,
        }
    }
}

/// How much each genre says: the log of how rare it is in `genres`.
pub fn genre_weights(genres: &HashMap<i64, Vec<String>>) -> HashMap<String, f32> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for list in genres.values() {
        for g in list {
            *counts.entry(g.as_str()).or_default() += 1;
        }
    }
    let n = genres.len() as f32 + 1.0;
    counts
        .into_iter()
        .map(|(g, c)| (g.to_owned(), (n / (c as f32 + 1.0)).ln().max(0.1)))
        .collect()
}

/// Where a radio starts from.
#[derive(Clone, Debug, Default)]
pub struct Seeds {
    /// Songs it should sound like.
    pub tracks: Vec<i64>,
    /// Only these songs may play (a genre's), if set.
    pub only: Option<HashSet<i64>>,
    /// Genres and styles the seed has, normalized.
    pub genres: HashSet<String>,
    /// Songs by the artist or artists the radio started from.
    pub favored: HashSet<i64>,
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
/// Songs by the artist the radio started from weigh this much more, worth
/// about 19 places down the list.
const FAVORED_FACTOR: f32 = 3.0;
/// They thin out more slowly than other artists do, so the radio keeps coming
/// back to them. Any slower and an artist with many songs takes the radio over.
const FAVORED_SPREAD: f32 = 0.85;
/// Every this many places down the list, a song's weight halves, so the
/// closest few dozen songs make up most of the radio.
const HALF_RANK: f32 = 12.0;
/// Songs whose genres share nothing with the seed's weigh this much; ones
/// that share them all, `1 + GENRE_BONUS`.
const OFF_GENRE_FACTOR: f32 = 0.2;
const GENRE_BONUS: f32 = 0.6;
/// How much what the AI heard counts in how close a song is, against the
/// analyzer's measurements (which hear tempo and punch but not style).
const HEARD_SHARE: f32 = 0.6;
/// Songs within this many years of the seed's era (plus half its own
/// spread) weigh in full; further out, half as much every `ERA_HALF` years.
const ERA_EASY: f32 = 4.0;
const ERA_HALF: f32 = 4.0;
/// Seeds spread wider than this (in years, middle half) have no era.
const ERA_SPREAD_MAX: i64 = 12;
/// A song whose year isn't known, when the seed has an era.
const NO_ERA_FACTOR: f32 = 0.7;

/// Up to `limit` songs for the radio, best first, with weights for a
/// weighted draw.
pub fn rank(library: &Library, seeds: &Seeds, session: &Session, limit: usize) -> Vec<(i64, f32)> {
    let index = library.index;
    let tags = library.genres;
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
    let mut near: Vec<(i64, Option<i64>)> = closest(library, &points)
        .into_iter()
        .filter(|(id, _)| allowed(*id))
        .collect();
    // Songs not analyzed yet (or no sound to go by): the allowed ones
    // after the analyzed ones, best tag match first.
    let placed: HashSet<i64> = near.iter().map(|(id, _)| *id).collect();
    let mut rest: Vec<(usize, i64)> = match &seeds.only {
        Some(only) => only.iter().copied().collect::<Vec<_>>(),
        None if near.is_empty() => tags.keys().chain(&seeds.favored).copied().collect(),
        // The artist's own songs still count before they've been analyzed.
        None => seeds.favored.iter().copied().collect(),
    }
    .into_iter()
    .filter(|id| allowed(*id) && !placed.contains(id))
    .map(|id| (overlap_count(tags.get(&id), &seeds.genres), id))
    .filter(|(shared, id)| seeds.only.is_some() || *shared > 0 || seeds.favored.contains(id))
    .collect();
    rest.sort_unstable();
    rest.dedup();
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

    let era = seed_era(library, seeds, session);
    let genre_weight = |g: &String| {
        library
            .genre_weight
            .and_then(|w| w.get(g))
            .copied()
            .unwrap_or(1.0)
    };
    let seed_weight: f32 = seeds.genres.iter().map(genre_weight).sum();
    let mut by_artist: HashMap<i64, i32> = HashMap::new();
    let mut weighted: Vec<(i64, f32)> = near
        .into_iter()
        .enumerate()
        .map(|(rank, (id, artist))| {
            let mut weight = 0.5f32.powf(rank as f32 / HALF_RANK);
            let has_tags = tags.get(&id).is_some_and(|t| !t.is_empty());
            if !seeds.genres.is_empty() && has_tags {
                let shared: f32 = tags
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .filter(|g| seeds.genres.contains(*g))
                    .map(genre_weight)
                    .sum();
                weight *= if shared > 0.0 {
                    1.0 + GENRE_BONUS * (shared / seed_weight.max(1e-6)).min(1.0)
                } else {
                    OFF_GENRE_FACTOR
                };
            }
            if let Some((center, easy)) = era {
                weight *= match library.eras.and_then(|e| e.get(&id)) {
                    Some(&year) => {
                        let off = (year as f32 - center).abs() - easy;
                        if off > 0.0 {
                            0.5f32.powf(off / ERA_HALF)
                        } else {
                            1.0
                        }
                    }
                    None => NO_ERA_FACTOR,
                };
            }
            if like_skipped.contains(&id) {
                weight *= SKIPPED_FACTOR;
            }
            let favored = seeds.favored.contains(&id);
            if favored {
                weight *= FAVORED_FACTOR;
            }
            if let Some(artist) = artist {
                let seen = by_artist.entry(artist).or_default();
                let spread = if favored {
                    FAVORED_SPREAD
                } else {
                    SAME_ARTIST_FACTOR
                };
                weight *= spread.powi(*seen);
                *seen += 1;
            }
            (id, weight)
        })
        .collect();
    weighted.sort_by(|a, b| b.1.total_cmp(&a.1));
    weighted.truncate(limit);
    weighted
}

/// Analyzed songs by how close they are to `points` (track id, weight),
/// nearest first, with their main artist: the analyzer's measurements and,
/// for songs the AI heard, what it heard, each by its place in the library.
fn closest(library: &Library, points: &[(i64, f32)]) -> Vec<(i64, Option<i64>)> {
    let around = library.index.around(points);
    let Some(heard) = library.heard.filter(|h| !h.is_empty()) else {
        return around.into_iter().map(|n| (n.track_id, n.artist)).collect();
    };
    // The middle of what the AI heard in the points.
    let mut center: Vec<f32> = Vec::new();
    for (track, weight) in points {
        if let Some(v) = heard.get(track) {
            center.resize(v.len(), 0.0);
            for (c, x) in center.iter_mut().zip(v) {
                *c += x * weight;
            }
        }
    }
    if center.is_empty() {
        return around.into_iter().map(|n| (n.track_id, n.artist)).collect();
    }
    let mut by_ear: Vec<(f32, i64)> = heard
        .iter()
        .map(|(id, v)| (v.iter().zip(&center).map(|(a, b)| a * b).sum(), *id))
        .collect();
    by_ear.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let n = by_ear.len().max(2) as f32 - 1.0;
    let ear: HashMap<i64, f32> = by_ear
        .into_iter()
        .enumerate()
        .map(|(i, (_, id))| (id, 1.0 - i as f32 / n))
        .collect();
    let m = around.len().max(2) as f32 - 1.0;
    let mut blended: Vec<(f32, i64, Option<i64>)> = around
        .into_iter()
        .enumerate()
        .map(|(i, near)| {
            let measured = 1.0 - i as f32 / m;
            // Not heard yet: in the middle.
            let heard = ear.get(&near.track_id).copied().unwrap_or(0.5);
            (
                HEARD_SHARE * heard + (1.0 - HEARD_SHARE) * measured,
                near.track_id,
                near.artist,
            )
        })
        .collect();
    blended.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    blended
        .into_iter()
        .map(|(_, id, artist)| (id, artist))
        .collect()
}

/// The year the seed belongs to, pulled a little toward what played
/// through since, and how many years around it count in full. `None` when
/// the seed's songs span too many years to have one (a long career, a
/// genre over decades).
fn seed_era(library: &Library, seeds: &Seeds, session: &Session) -> Option<(f32, f32)> {
    let eras = library.eras?;
    let mut years: Vec<i64> = seeds
        .tracks
        .iter()
        .filter_map(|t| eras.get(t).copied())
        .collect();
    if years.is_empty() {
        return None;
    }
    years.sort_unstable();
    let at = |p: f32| years[((years.len() - 1) as f32 * p).round() as usize];
    let spread = at(0.75) - at(0.25);
    if spread > ERA_SPREAD_MAX {
        return None;
    }
    let mut center = at(0.5) as f32;
    let mut total = 1.0;
    for year in session
        .played
        .iter()
        .take(DRIFT_SONGS)
        .filter_map(|t| eras.get(t))
    {
        center = (center * total + *year as f32 * DRIFT_WEIGHT) / (total + DRIFT_WEIGHT);
        total += DRIFT_WEIGHT;
    }
    Some((center, ERA_EASY + spread as f32 / 2.0))
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
    fn the_artist_it_started_from_counts_for_more() {
        let plain = Seeds {
            tracks: vec![1],
            ..Seeds::default()
        };
        let favored = Seeds {
            tracks: vec![1],
            favored: [1, 2, 3, 4].into_iter().collect(),
            ..Seeds::default()
        };
        let weight = |r: &[(i64, f32)], id| r.iter().find(|x| x.0 == id).unwrap().1;
        let before = rank(
            &Library::new(&index(), &HashMap::new()),
            &plain,
            &Session::default(),
            20,
        );
        let after = rank(
            &Library::new(&index(), &HashMap::new()),
            &favored,
            &Session::default(),
            20,
        );
        for song in [2, 3, 4] {
            assert!(
                weight(&after, song) > weight(&before, song) * 2.0,
                "song {song} should weigh far more"
            );
        }
        // 4 fell behind 5 for being a third song by one artist; now it leads.
        assert!(weight(&before, 4) < weight(&before, 5));
        assert!(weight(&after, 4) > weight(&after, 5));
        // Other artists stay in the running, as they were.
        assert_eq!(weight(&after, 5), weight(&before, 5));
    }

    /// 40 artists of 5 songs, plus one with `songs` of their own, all mixed
    /// together in sound. Returns that artist's share of the radio's weight.
    fn favored_share(songs: i64, favored: bool) -> f32 {
        let star = 1000;
        let mut rows: Vec<SoundRow> = (0..200)
            .map(|id| SoundRow {
                track_id: id,
                album_id: Some(id),
                artist_ids: vec![id / 5],
                title: format!("Song {id}"),
                features: {
                    let mut f = vec![0.0; 37];
                    f[0] = (id * 7 % 200) as f32;
                    f[5] = (id % 3) as f32 * 0.01;
                    f
                },
            })
            .collect();
        let own: Vec<i64> = (0..songs).map(|n| 5000 + n).collect();
        for (n, &id) in own.iter().enumerate() {
            rows.push(SoundRow {
                track_id: id,
                album_id: Some(id),
                artist_ids: vec![star],
                title: format!("Own {id}"),
                features: {
                    let mut f = vec![0.0; 37];
                    f[0] = n as f32 * 200.0 / songs as f32;
                    f[5] = (n % 3) as f32 * 0.01;
                    f
                },
            });
        }
        let seeds = Seeds {
            tracks: vec![own[0]],
            favored: if favored {
                own.iter().copied().collect()
            } else {
                HashSet::new()
            },
            ..Seeds::default()
        };
        let ranked = rank(
            &Library::new(&SoundIndex::build(rows), &HashMap::new()),
            &seeds,
            &Session::default(),
            150,
        );
        let total: f32 = ranked.iter().map(|r| r.1).sum();
        let mine: f32 = ranked
            .iter()
            .filter(|r| own.contains(&r.0))
            .map(|r| r.1)
            .sum();
        mine / total
    }

    #[test]
    fn the_artist_gets_a_big_share_but_not_the_whole_radio() {
        for songs in [10, 40, 200] {
            let without = favored_share(songs, false);
            let with = favored_share(songs, true);
            assert!(
                with > without * 1.5,
                "{songs} songs: {without} became {with}"
            );
            assert!(with > 0.2, "{songs} songs: only {with}");
            assert!(
                with < 0.7,
                "{songs} songs: {with} leaves too little for anyone else"
            );
        }
    }

    #[test]
    fn the_artist_still_comes_first_with_songs_not_analyzed_yet() {
        let tags: HashMap<i64, Vec<String>> = HashMap::new();
        let seeds = Seeds {
            tracks: vec![1],
            // 77 isn't in the sound index, and has no tags to match on.
            favored: [1, 77].into_iter().collect(),
            ..Seeds::default()
        };
        let ranked = rank(
            &Library::new(&index(), &tags),
            &seeds,
            &Session::default(),
            20,
        );
        assert!(ids(&ranked).contains(&77));
    }

    #[test]
    fn skipped_songs_stay_out_even_by_the_artist() {
        let seeds = Seeds {
            tracks: vec![1],
            favored: [1, 2, 3].into_iter().collect(),
            ..Seeds::default()
        };
        let session = Session {
            skipped: vec![2],
            ..Session::default()
        };
        let ranked = rank(
            &Library::new(&index(), &HashMap::new()),
            &seeds,
            &session,
            20,
        );
        assert!(!ids(&ranked).contains(&2));
    }

    #[test]
    fn plays_what_sounds_like_the_seed() {
        let seeds = Seeds {
            tracks: vec![1],
            ..Seeds::default()
        };
        let ranked = rank(
            &Library::new(&index(), &HashMap::new()),
            &seeds,
            &Session::default(),
            5,
        );
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
        let calm = rank(
            &Library::new(&index(), &HashMap::new()),
            &seeds,
            &Session::default(),
            20,
        );
        let weight = |r: &[(i64, f32)], id| r.iter().find(|x| x.0 == id).map(|x| x.1);
        // 2, 3 and 4 share an artist: 4 gets pushed below 5's neighbor rank.
        assert!(weight(&calm, 4) < weight(&calm, 5));

        let skipped = Session {
            skipped: vec![3],
            ..Session::default()
        };
        let after = rank(
            &Library::new(&index(), &HashMap::new()),
            &seeds,
            &skipped,
            20,
        );
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
            ..Seeds::default()
        };
        let ranked = rank(
            &Library::new(&index(), &tags),
            &seeds,
            &Session::default(),
            20,
        );
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
            ..Seeds::default()
        };
        // Only the genre's songs, the unanalyzed one too.
        let ranked = rank(
            &Library::new(&index(), &tags),
            &seeds,
            &Session::default(),
            10,
        );
        let mut got = ids(&ranked);
        got.sort();
        assert_eq!(got, [2, 8, 20]);
    }

    #[test]
    fn stays_near_the_seeds_era() {
        let tags = HashMap::new();
        // 2 and 9 are both near 1 in sound order terms; 2 is from 2013.
        let eras: HashMap<i64, i64> = (1..=9)
            .map(|id| (id, if id == 2 { 2013 } else { 1987 }))
            .collect();
        let seeds = Seeds {
            tracks: vec![1],
            ..Seeds::default()
        };
        let index = index();
        let plain = rank(
            &Library::new(&index, &tags),
            &seeds,
            &Session::default(),
            20,
        );
        let mut library = Library::new(&index, &tags);
        library.eras = Some(&eras);
        let dated = rank(&library, &seeds, &Session::default(), 20);
        let weight = |r: &[(i64, f32)], id| r.iter().find(|x| x.0 == id).map_or(0.0, |x| x.1);
        assert!(weight(&dated, 2) < weight(&plain, 2) * 0.1, "26 years off");
        assert_eq!(
            weight(&dated, 3),
            weight(&plain, 3),
            "same era, same weight"
        );
    }

    #[test]
    fn rare_shared_genres_count_more_than_common_ones() {
        let mut tags: HashMap<i64, Vec<String>> =
            (1..=9).map(|id| (id, vec!["rock".to_owned()])).collect();
        for id in 100..200 {
            tags.insert(id, vec!["rock".to_owned()]);
        }
        tags.insert(1, vec!["rock".into(), "glam metal".into()]);
        tags.insert(6, vec!["rock".into(), "glam metal".into()]);
        let weights = genre_weights(&tags);
        assert!(weights["glam metal"] > weights["rock"] * 3.0);
        let index = index();
        let mut library = Library::new(&index, &tags);
        library.genre_weight = Some(&weights);
        let seeds = Seeds {
            tracks: vec![1],
            genres: tags[&1].iter().cloned().collect(),
            ..Seeds::default()
        };
        let ranked = rank(&library, &seeds, &Session::default(), 20);
        let weight = |id| ranked.iter().find(|x| x.0 == id).map_or(0.0, |x| x.1);
        // 5 is closer in sound than 6, but shares only "rock".
        assert!(weight(6) > weight(5), "{ranked:?}");
    }

    #[test]
    fn what_the_ai_heard_moves_songs_closer() {
        let tags = HashMap::new();
        // By ear, 9 is the seed's twin; by measurement it's the farthest.
        let heard: HashMap<i64, Vec<f32>> = (1..=9)
            .map(|id| {
                let v = if id == 1 || id == 9 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                (id, v)
            })
            .collect();
        let seeds = Seeds {
            tracks: vec![1],
            ..Seeds::default()
        };
        let index = index();
        let plain = rank(&Library::new(&index, &tags), &seeds, &Session::default(), 5);
        assert!(!ids(&plain).contains(&9));
        let mut library = Library::new(&index, &tags);
        library.heard = Some(&heard);
        let by_ear = rank(&library, &seeds, &Session::default(), 5);
        assert!(ids(&by_ear).contains(&9), "{by_ear:?}");
    }
}
