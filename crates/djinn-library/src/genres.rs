//! The library's genres and how they relate, for browsing by genre and the
//! genre cloud: two genres are linked when many songs have both, and when
//! their songs sound alike on average.

use std::collections::HashMap;

use serde::Serialize;

use crate::sound::SoundIndex;

/// A genre and how many songs have it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Genre {
    pub name: String,
    pub songs: usize,
}

/// Two related genres (indexes into [`GenreMap::genres`]), and how related
/// they are, from 0 to 1.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    pub strength: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GenreMap {
    /// By name.
    pub genres: Vec<Genre>,
    pub links: Vec<Link>,
}

/// Each genre gets at most this many links, its strongest, so big genres
/// don't tie everything together; each keeps at least its strongest one.
const LINKS_PER_GENRE: usize = 4;
/// Weaker links aren't drawn.
const MIN_STRENGTH: f32 = 0.2;
/// Songs a genre needs analyzed before its sound counts.
const MIN_ANALYZED: usize = 3;
/// How much shared songs count against sounding alike.
const SHARED_WEIGHT: f32 = 0.6;
/// Added to the smaller genre's songs when measuring overlap, so a genre
/// with one song isn't "all overlap" with every genre that song has.
const OVERLAP_DAMPING: f32 = 2.0;

/// Builds the map from each song's genres and styles (`tags`) and how the
/// songs sound.
pub fn map(tags: &HashMap<i64, Vec<String>>, index: &SoundIndex) -> GenreMap {
    let mut songs: HashMap<&str, Vec<i64>> = HashMap::new();
    for (&song, names) in tags {
        let mut seen: Vec<&str> = Vec::new();
        for name in names {
            if !seen.contains(&name.as_str()) {
                seen.push(name);
                songs.entry(name).or_default().push(song);
            }
        }
    }
    let mut genres: Vec<Genre> = songs
        .iter()
        .map(|(name, ids)| Genre {
            name: (*name).to_owned(),
            songs: ids.len(),
        })
        .collect();
    genres.sort_by(|a, b| a.name.cmp(&b.name));
    let place: HashMap<&str, usize> = genres
        .iter()
        .enumerate()
        .map(|(i, g)| (g.name.as_str(), i))
        .collect();

    // Songs each pair of genres shares.
    let mut shared: HashMap<(usize, usize), usize> = HashMap::new();
    for names in tags.values() {
        let mut ids: Vec<usize> = names
            .iter()
            .filter_map(|n| place.get(n.as_str()).copied())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                *shared.entry((a, b)).or_default() += 1;
            }
        }
    }

    // Each genre's average sound, and how far apart genres usually are.
    let centers: Vec<Option<Vec<f32>>> = genres
        .iter()
        .map(|g| {
            index
                .center(songs[g.name.as_str()].iter().copied())
                .filter(|(_, analyzed)| *analyzed >= MIN_ANALYZED)
                .map(|(center, _)| center)
        })
        .collect();
    let mut gaps: Vec<((usize, usize), f32)> = Vec::new();
    for a in 0..genres.len() {
        for b in a + 1..genres.len() {
            if let (Some(ca), Some(cb)) = (&centers[a], &centers[b]) {
                gaps.push(((a, b), SoundIndex::distance(ca, cb)));
            }
        }
    }
    let mut sorted: Vec<f32> = gaps.iter().map(|(_, d)| *d).collect();
    sorted.sort_by(f32::total_cmp);
    let typical = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
    let alike: HashMap<(usize, usize), f32> = gaps
        .into_iter()
        .filter(|_| typical > 0.0)
        .map(|(pair, d)| (pair, (1.0 - d / typical).clamp(0.0, 1.0)))
        .collect();

    let mut candidates: Vec<Link> = Vec::new();
    for a in 0..genres.len() {
        for b in a + 1..genres.len() {
            let both = shared.get(&(a, b)).copied().unwrap_or(0);
            let smaller = genres[a].songs.min(genres[b].songs) as f32;
            let overlap = both as f32 / (smaller + OVERLAP_DAMPING);
            let strength = match alike.get(&(a, b)) {
                Some(sound) => SHARED_WEIGHT * overlap + (1.0 - SHARED_WEIGHT) * sound,
                None => overlap,
            };
            if strength >= MIN_STRENGTH {
                candidates.push(Link { a, b, strength });
            }
        }
    }
    candidates.sort_by(|x, y| y.strength.total_cmp(&x.strength));
    let mut kept = vec![0usize; genres.len()];
    let mut links = Vec::new();
    let mut left = Vec::new();
    for link in candidates {
        if kept[link.a] < LINKS_PER_GENRE && kept[link.b] < LINKS_PER_GENRE {
            kept[link.a] += 1;
            kept[link.b] += 1;
            links.push(link);
        } else {
            left.push(link);
        }
    }
    // A genre crowded out by busier ones still gets its strongest link.
    for link in left {
        if kept[link.a] == 0 || kept[link.b] == 0 {
            kept[link.a] += 1;
            kept[link.b] += 1;
            links.push(link);
        }
    }
    GenreMap { genres, links }
}

#[cfg(test)]
mod tests {
    use super::*;
    use djinn_store::SoundRow;

    fn tags(list: &[(i64, &[&str])]) -> HashMap<i64, Vec<String>> {
        list.iter()
            .map(|(id, names)| (*id, names.iter().map(|n| (*n).to_owned()).collect()))
            .collect()
    }

    fn linked(map: &GenreMap, a: &str, b: &str) -> bool {
        let at = |n: &str| map.genres.iter().position(|g| g.name == n);
        let (Some(a), Some(b)) = (at(a), at(b)) else {
            return false;
        };
        map.links.iter().any(|l| (l.a, l.b) == (a.min(b), a.max(b)))
    }

    #[test]
    fn genres_that_share_songs_are_linked() {
        let tags = tags(&[
            (1, &["house", "deep house"]),
            (2, &["house", "deep house"]),
            (3, &["house"]),
            (4, &["rock"]),
            (5, &["rock", "metal"]),
        ]);
        let map = map(&tags, &SoundIndex::default());
        let names: Vec<(&str, usize)> = map
            .genres
            .iter()
            .map(|g| (g.name.as_str(), g.songs))
            .collect();
        assert_eq!(
            names,
            [("deep house", 2), ("house", 3), ("metal", 1), ("rock", 2)]
        );
        assert!(linked(&map, "house", "deep house"));
        assert!(linked(&map, "rock", "metal"));
        assert!(!linked(&map, "house", "rock"));
    }

    #[test]
    fn genres_that_sound_alike_are_linked_without_sharing_songs() {
        // Songs on a line by tempo: 1-3 and 4-6 sound alike, 7-9 far off.
        let rows = (1..=9)
            .map(|id| {
                let mut features = vec![0.0; 37];
                features[0] = match id {
                    1..=3 => 100.0 + id as f32,
                    4..=6 => 104.0 + id as f32,
                    _ => 300.0 + id as f32,
                };
                features[5] = (id % 3) as f32;
                SoundRow {
                    track_id: id,
                    album_id: Some(id),
                    artist_ids: vec![id],
                    title: format!("Song {id}"),
                    features,
                }
            })
            .collect();
        let index = SoundIndex::build(rows);
        let tags = tags(&[
            (1, &["synthwave"]),
            (2, &["synthwave"]),
            (3, &["synthwave"]),
            (4, &["darkwave"]),
            (5, &["darkwave"]),
            (6, &["darkwave"]),
            (7, &["polka"]),
            (8, &["polka"]),
            (9, &["polka"]),
        ]);
        let map = map(&tags, &index);
        assert!(linked(&map, "synthwave", "darkwave"));
        assert!(!linked(&map, "synthwave", "polka"));
    }
}
