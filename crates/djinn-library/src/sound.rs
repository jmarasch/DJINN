//! Finding songs and artists that sound alike, from the features the
//! engine measures.
//!
//! Each feature is turned into a z-score against the whole library (so
//! "bright" means bright for this collection), then weighted: tempo and
//! timbre say most about how a song feels, key profile least. Distance is
//! plain Euclidean on those, small enough to scan the whole library per
//! query.

use std::collections::HashMap;

use djinn_store::SoundRow;

/// How much each feature counts, in `djinn_engine::features` order. Tuned
/// on a real library: timbre identifies a sound best, tempo keeps a mix
/// flowing, and zero crossings, dynamics, and loudness add little.
const WEIGHTS: [f32; 37] = [
    2.0,  // tempo
    1.0,  // punch
    1.1,  // brightness
    0.4,  // brightness spread
    0.8,  // rolloff
    0.3,  // rolloff spread
    1.0,  // noisiness
    0.4,  // noisiness spread
    0.25, // zero crossings
    0.3,  // dynamics
    0.3,  // loudness
    0.6,  // major vs minor
    0.3,  // key clarity
    // Timbre (mel cepstrum), the low ones most.
    1.35, 1.2, 1.05, 0.9, 0.9, 0.75, 0.75, 0.6, 0.6, 0.6, 0.45, 0.45, // Harmony profile.
    0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15,
];

/// How much each feature says about a song's energy (chill to hype), in
/// `djinn_engine::features` order: tempo and punch most, brightness,
/// noisiness and loudness some; wide dynamics lean calm.
const ENERGY: [(usize, f32); 6] = [
    (0, 1.0),  // tempo
    (1, 1.4),  // punch
    (2, 0.6),  // brightness
    (6, 0.4),  // noisiness
    (10, 0.6), // loudness
    (9, -0.3), // dynamics
];

/// Artists need this many analyzed songs to count as similar artists; a
/// guest on one song is too little to go on.
const MIN_ARTIST_SONGS: usize = 2;

/// Most songs from one album, and from one artist, in a "more like this"
/// list, so it isn't just the rest of the record.
const PER_ALBUM: usize = 2;
const PER_ARTIST: usize = 3;

struct Entry {
    track_id: i64,
    album_id: Option<i64>,
    /// The same song in another version (album and single, remixes):
    /// same key.
    song_key: String,
    artist_ids: Vec<i64>,
    vector: Vec<f32>,
}

/// A song near some point in sound space.
#[derive(Clone, Debug, PartialEq)]
pub struct Near {
    pub track_id: i64,
    pub distance: f32,
    pub artist: Option<i64>,
}

/// The library's songs as points in sound space.
#[derive(Default)]
pub struct SoundIndex {
    entries: Vec<Entry>,
    by_track: HashMap<i64, usize>,
    artists: HashMap<i64, Vec<f32>>,
    artist_songs: HashMap<i64, usize>,
    /// Each song's energy, -1 (the calmest here) to 1 (the most hyped).
    energy: HashMap<i64, f32>,
}

impl SoundIndex {
    pub fn build(rows: Vec<SoundRow>) -> Self {
        let dims = rows.first().map_or(0, |r| r.features.len());
        let rows: Vec<SoundRow> = rows
            .into_iter()
            .filter(|r| r.features.len() == dims)
            .collect();
        if rows.is_empty() {
            return Self::default();
        }
        let n = rows.len() as f32;
        let mut mean = vec![0.0f32; dims];
        for row in &rows {
            for (m, v) in mean.iter_mut().zip(&row.features) {
                *m += v / n;
            }
        }
        let mut spread = vec![0.0f32; dims];
        for row in &rows {
            for ((s, v), m) in spread.iter_mut().zip(&row.features).zip(&mean) {
                *s += (v - m).powi(2) / n;
            }
        }
        for s in &mut spread {
            *s = s.sqrt().max(1e-6);
        }
        let energy = energies(&rows, &mean, &spread);
        let entries: Vec<Entry> = rows
            .into_iter()
            .map(|row| Entry {
                vector: row
                    .features
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        (v - mean[i]) / spread[i] * WEIGHTS.get(i).copied().unwrap_or(1.0)
                    })
                    .collect(),
                song_key: format!(
                    "{}|{}",
                    base_title(&row.title),
                    row.artist_ids.first().copied().unwrap_or_default()
                ),
                track_id: row.track_id,
                album_id: row.album_id,
                artist_ids: row.artist_ids,
            })
            .collect();
        let by_track = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.track_id, i))
            .collect();
        // An artist sounds like the average of their songs.
        let mut sums: HashMap<i64, (Vec<f32>, f32)> = HashMap::new();
        let mut songs: HashMap<i64, std::collections::HashSet<&str>> = HashMap::new();
        for entry in &entries {
            for &artist in &entry.artist_ids {
                songs
                    .entry(artist)
                    .or_default()
                    .insert(entry.song_key.split('|').next().unwrap_or_default());
                let (sum, count) = sums.entry(artist).or_insert_with(|| (vec![0.0; dims], 0.0));
                for (s, v) in sum.iter_mut().zip(&entry.vector) {
                    *s += v;
                }
                *count += 1.0;
            }
        }
        // Distinct songs, so a guest on five remixes of one song counts once.
        let artist_songs = songs
            .into_iter()
            .map(|(artist, titles)| (artist, titles.len()))
            .collect();
        let artists = sums
            .into_iter()
            .map(|(artist, (sum, count))| (artist, sum.into_iter().map(|s| s / count).collect()))
            .collect();
        Self {
            entries,
            by_track,
            artists,
            artist_songs,
            energy,
        }
    }

    /// Each analyzed song's energy, -1 (the calmest in this library) to 1
    /// (the most hyped), spread evenly by rank.
    pub fn energy(&self) -> &HashMap<i64, f32> {
        &self.energy
    }

    /// Songs analyzed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Songs that sound most like `track_id`, closest first, spread over
    /// albums and artists. Empty if the song hasn't been analyzed.
    pub fn similar_tracks(&self, track_id: i64, limit: usize) -> Vec<i64> {
        let Some(&i) = self.by_track.get(&track_id) else {
            return Vec::new();
        };
        let seed = &self.entries[i];
        self.diverse(&seed.vector, limit, |e| {
            e.track_id == track_id || e.song_key == seed.song_key
        })
    }

    /// Songs that sound most like an artist overall, excluding the artist's
    /// own songs unless `include_own`.
    pub fn tracks_like_artist(&self, artist_id: i64, limit: usize, include_own: bool) -> Vec<i64> {
        let Some(center) = self.artists.get(&artist_id) else {
            return Vec::new();
        };
        self.diverse(center, limit, |e| {
            !include_own && e.artist_ids.contains(&artist_id)
        })
    }

    /// The closest `limit` songs to a point, honoring the per-album and
    /// per-artist caps.
    fn diverse(&self, point: &[f32], limit: usize, skip: impl Fn(&Entry) -> bool) -> Vec<i64> {
        let mut ranked: Vec<(f32, &Entry)> = self
            .entries
            .iter()
            .filter(|e| !skip(e))
            .map(|e| (distance(point, &e.vector), e))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut albums: HashMap<i64, usize> = HashMap::new();
        let mut artists: HashMap<i64, usize> = HashMap::new();
        let mut songs = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(limit);
        for (_, entry) in ranked {
            if out.len() >= limit {
                break;
            }
            // One copy of each song.
            if !songs.insert(entry.song_key.as_str()) {
                continue;
            }
            if let Some(album) = entry.album_id {
                if albums.get(&album).copied().unwrap_or(0) >= PER_ALBUM {
                    continue;
                }
            }
            if entry
                .artist_ids
                .first()
                .is_some_and(|a| artists.get(a).copied().unwrap_or(0) >= PER_ARTIST)
            {
                continue;
            }
            if let Some(album) = entry.album_id {
                *albums.entry(album).or_default() += 1;
            }
            if let Some(&artist) = entry.artist_ids.first() {
                *artists.entry(artist).or_default() += 1;
            }
            out.push(entry.track_id);
        }
        out
    }

    /// Every analyzed song by how close it is to the weighted middle of
    /// `seeds` (track id, weight), nearest first, with its distance and
    /// main artist. One copy of each song (the seeds included). Empty if
    /// no seed was analyzed.
    pub fn around(&self, seeds: &[(i64, f32)]) -> Vec<Near> {
        let dims = self.entries.first().map_or(0, |e| e.vector.len());
        let mut center = vec![0.0f32; dims];
        let mut total = 0.0;
        for &(track, weight) in seeds {
            let Some(&i) = self.by_track.get(&track) else {
                continue;
            };
            let entry = &self.entries[i];
            for (c, v) in center.iter_mut().zip(&entry.vector) {
                *c += v * weight;
            }
            total += weight;
        }
        if total <= 0.0 {
            return Vec::new();
        }
        for c in &mut center {
            *c /= total;
        }
        let mut ranked: Vec<(f32, &Entry)> = self
            .entries
            .iter()
            .map(|e| (distance(&center, &e.vector), e))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut songs = std::collections::HashSet::new();
        ranked
            .into_iter()
            .filter(|(_, e)| songs.insert(e.song_key.as_str()))
            .map(|(distance, e)| Near {
                track_id: e.track_id,
                distance,
                artist: e.artist_ids.first().copied(),
            })
            .collect()
    }

    /// The average sound of `tracks` and how many of them were analyzed.
    /// `None` if none were.
    pub fn center(&self, tracks: impl IntoIterator<Item = i64>) -> Option<(Vec<f32>, usize)> {
        let dims = self.entries.first()?.vector.len();
        let mut sum = vec![0.0f32; dims];
        let mut count = 0;
        for track in tracks {
            let Some(&i) = self.by_track.get(&track) else {
                continue;
            };
            for (s, v) in sum.iter_mut().zip(&self.entries[i].vector) {
                *s += v;
            }
            count += 1;
        }
        if count == 0 {
            return None;
        }
        for s in &mut sum {
            *s /= count as f32;
        }
        Some((sum, count))
    }

    /// How far apart two points from [`SoundIndex::center`] are.
    pub fn distance(a: &[f32], b: &[f32]) -> f32 {
        distance(a, b)
    }

    /// How far every other artist's sound is from `artist_id`'s, with how
    /// many distinct songs each has. Empty if the artist wasn't analyzed.
    pub fn artist_distances(&self, artist_id: i64) -> Vec<(i64, f32, usize)> {
        let Some(center) = self.artists.get(&artist_id) else {
            return Vec::new();
        };
        self.artists
            .iter()
            .filter(|(&id, _)| id != artist_id)
            .map(|(&id, v)| {
                (
                    id,
                    distance(center, v),
                    self.artist_songs.get(&id).copied().unwrap_or(0),
                )
            })
            .collect()
    }

    /// Artists who sound most like `artist_id`, closest first.
    pub fn similar_artists(&self, artist_id: i64, limit: usize) -> Vec<i64> {
        let Some(center) = self.artists.get(&artist_id) else {
            return Vec::new();
        };
        let mut ranked: Vec<(f32, i64)> = self
            .artists
            .iter()
            .filter(|(&id, _)| {
                id != artist_id
                    && self.artist_songs.get(&id).copied().unwrap_or(0) >= MIN_ARTIST_SONGS
            })
            .map(|(&id, v)| (distance(center, v), id))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        ranked.into_iter().take(limit).map(|(_, id)| id).collect()
    }
}

/// A title without its version notes: "Taped Up Heart (feat. Clara Mae)
/// [Disto Remix]" and "Taped Up Heart - Acoustic" are both "taped up heart".
fn base_title(title: &str) -> String {
    let cut = [" (", " [", " - "]
        .iter()
        .filter_map(|m| title.find(m))
        .min()
        .unwrap_or(title.len());
    title[..cut].trim().to_lowercase()
}

fn distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>()
}

/// Energy by rank: songs ordered by a mix of z-scores ([`ENERGY`]), the
/// calmest at -1 and the most hyped at 1, so the scale means the same in
/// every library.
fn energies(rows: &[SoundRow], mean: &[f32], spread: &[f32]) -> HashMap<i64, f32> {
    let mut raw: Vec<(f32, i64)> = rows
        .iter()
        .map(|row| {
            let score = ENERGY
                .iter()
                .filter_map(|&(i, weight)| {
                    let v = row.features.get(i)?;
                    Some((v - mean.get(i)?) / spread.get(i)? * weight)
                })
                .sum();
            (score, row.track_id)
        })
        .collect();
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));
    let last = raw.len().saturating_sub(1).max(1) as f32;
    raw.into_iter()
        .enumerate()
        .map(|(rank, (_, id))| (id, rank as f32 / last * 2.0 - 1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(track_id: i64, album: i64, artist: i64, tempo: f32, bright: f32) -> SoundRow {
        let mut features = vec![0.0; 37];
        features[0] = tempo;
        features[2] = bright;
        // A little spread elsewhere so no feature is constant.
        features[5] = (track_id % 3) as f32 * 0.01;
        SoundRow {
            track_id,
            album_id: Some(album),
            artist_ids: vec![artist],
            title: format!("Song {track_id}"),
            features,
        }
    }

    #[test]
    fn energy_runs_from_calm_to_hyped() {
        let mut fast = row(1, 1, 1, 7.4, 0.5);
        fast.features[1] = 3.0;
        let mut slow = row(2, 2, 2, 6.2, 0.2);
        slow.features[1] = 0.5;
        let middle = row(3, 3, 3, 6.9, 0.3);
        let index = SoundIndex::build(vec![fast, slow, middle]);
        let energy = index.energy();
        assert_eq!(energy.get(&1), Some(&1.0));
        assert_eq!(energy.get(&2), Some(&-1.0));
        assert_eq!(energy.get(&3), Some(&0.0));
        assert!(SoundIndex::build(Vec::new()).energy().is_empty());
    }

    #[test]
    fn versions_of_a_song_share_a_title() {
        assert_eq!(
            base_title("Taped Up Heart (feat. Clara Mae) [Disto Remix]"),
            "taped up heart"
        );
        assert_eq!(base_title("Taped Up Heart - Acoustic"), "taped up heart");
        assert_eq!(base_title("Wish"), "wish");
    }

    #[test]
    fn nearest_songs_and_artists() {
        // Two fast, bright artists (1 and 2) and a slow, dark one (3).
        let rows = vec![
            row(1, 10, 1, 7.0, 6.8),
            row(2, 10, 1, 7.0, 6.9),
            row(3, 10, 1, 7.0, 6.85),
            row(4, 20, 2, 7.02, 6.82),
            row(5, 20, 2, 6.98, 6.88),
            row(6, 30, 3, 6.3, 5.5),
            row(7, 30, 3, 6.32, 5.6),
        ];
        let index = SoundIndex::build(rows);
        assert_eq!(index.len(), 7);

        let similar = index.similar_tracks(1, 3);
        assert_eq!(similar.len(), 3);
        assert!(!similar.contains(&1), "not itself");
        assert!(
            similar.iter().all(|t| *t <= 5),
            "fast and bright first: {similar:?}"
        );
        // At most two from the same album.
        assert!(similar.iter().filter(|t| **t <= 3).count() <= 2);

        assert_eq!(index.similar_artists(1, 2), [2, 3]);
        // Five songs by others, but album 10 is capped at two.
        assert_eq!(index.tracks_like_artist(3, 5, false).len(), 4);
        assert!(index.similar_tracks(99, 5).is_empty(), "unknown song");

        // Another copy of the same song is never "like" itself.
        let mut rows = vec![
            row(1, 10, 1, 7.0, 6.8),
            row(2, 20, 1, 7.0, 6.8),
            row(3, 30, 2, 6.0, 5.0),
        ];
        rows[1].title = "Song 1".into();
        let index = SoundIndex::build(rows);
        assert_eq!(index.similar_tracks(1, 5), [3]);
    }
}
