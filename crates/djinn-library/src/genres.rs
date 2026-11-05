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
    /// Links among the biggest genres only, for the cloud's top level. Among
    /// all genres, a tiny one can take a big one's places and leave it few
    /// lines to anything shown.
    pub top_links: Vec<Link>,
}

/// How much of the map to keep.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Genres at the cloud's top level, the biggest.
    pub top: usize,
    /// Links each genre keeps, its strongest.
    pub links_per_genre: usize,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            top: 100,
            links_per_genre: LINKS_PER_GENRE,
        }
    }
}

/// A genre related to another, as on the genre cloud.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Related {
    pub name: String,
    pub songs: usize,
    pub strength: f32,
}

impl GenreMap {
    /// The genres linked to `name` (the lines from it on the cloud),
    /// strongest first.
    pub fn related(&self, name: &str) -> Vec<Related> {
        let Some(at) = self.genres.iter().position(|g| g.name == name) else {
            return Vec::new();
        };
        let mut found: Vec<Related> = self
            .links
            .iter()
            .filter_map(|l| match (l.a == at, l.b == at) {
                (true, _) => Some((l.b, l.strength)),
                (_, true) => Some((l.a, l.strength)),
                _ => None,
            })
            .filter_map(|(other, strength)| {
                let genre = self.genres.get(other)?;
                Some(Related {
                    name: genre.name.clone(),
                    songs: genre.songs,
                    strength,
                })
            })
            .collect();
        found.sort_by(|a, b| b.strength.total_cmp(&a.strength).then(a.name.cmp(&b.name)));
        found
    }
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
    shaped(tags, index, Shape::default())
}

/// [`map`], keeping as much as `shape` says.
pub fn shaped(tags: &HashMap<i64, Vec<String>>, index: &SoundIndex, shape: Shape) -> GenreMap {
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
    let mut biggest: Vec<usize> = (0..genres.len()).collect();
    biggest.sort_by_key(|&g| std::cmp::Reverse(genres[g].songs));
    let mut on_top = vec![false; genres.len()];
    for g in biggest.into_iter().take(shape.top) {
        on_top[g] = true;
    }
    let top: Vec<Link> = candidates
        .iter()
        .filter(|l| on_top[l.a] && on_top[l.b])
        .cloned()
        .collect();
    let top_links = strongest(top, genres.len(), shape.links_per_genre);
    let links = strongest(candidates, genres.len(), shape.links_per_genre);
    GenreMap {
        genres,
        links,
        top_links,
    }
}

/// Keeps each genre's strongest links, of `candidates` sorted strongest first.
fn strongest(candidates: Vec<Link>, genres: usize, per_genre: usize) -> Vec<Link> {
    let mut kept = vec![0usize; genres];
    let mut links = Vec::new();
    let mut left = Vec::new();
    for link in candidates {
        if kept[link.a] < per_genre && kept[link.b] < per_genre {
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
    links
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
    fn tiny_genres_dont_take_a_big_ones_places_at_the_top_level() {
        // Rock shares songs with two tiny genres, and more loosely with pop.
        let mut list: Vec<(i64, Vec<&str>)> = Vec::new();
        for song in 0..40 {
            list.push((song, vec!["rock"]));
        }
        for song in 40..80 {
            list.push((song, vec!["pop"]));
        }
        for song in 0..20 {
            list[song as usize].1.push("pop");
        }
        for song in 0..3 {
            list[song as usize].1.push("tiny one");
        }
        for song in 3..6 {
            list[song as usize].1.push("tiny two");
        }
        let tags: HashMap<i64, Vec<String>> = list
            .into_iter()
            .map(|(song, names)| (song, names.into_iter().map(str::to_owned).collect()))
            .collect();
        let map = shaped(
            &tags,
            &SoundIndex::default(),
            Shape {
                top: 2,
                links_per_genre: 2,
            },
        );
        let at = |n: &str| map.genres.iter().position(|g| g.name == n).unwrap();
        let (rock, pop) = (at("rock"), at("pop"));
        assert!(
            !map.links
                .iter()
                .any(|l| (l.a, l.b) == (rock.min(pop), rock.max(pop))),
            "over all genres rock's places go to the tiny ones"
        );
        assert_eq!(map.top_links.len(), 1);
        assert_eq!(
            (map.top_links[0].a, map.top_links[0].b),
            (rock.min(pop), rock.max(pop))
        );
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
        let near: Vec<String> = map.related("rock").into_iter().map(|r| r.name).collect();
        assert_eq!(near, ["metal"]);
        assert!(map.related("polka").is_empty());
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
