//! Finds copies of a song in the library: the same title and artist at
//! about the same length, the same identified recording, or the same sound
//! (fingerprints from sound analysis). Nothing is ever deleted; the admin
//! sees groups with a suggested copy to keep.

use std::collections::{HashMap, HashSet};

use djinn_store::DuplicateCandidate;
use serde::Serialize;

use crate::playlist_match::{artist_names, core_title};

/// Copies found by name must be this close in length.
const NAME_GAP_MS: i64 = 3_000;
/// Copies of one identified recording may differ a little more (silence).
const RECORDING_GAP_MS: i64 = 5_000;
/// Songs compared by sound must be this close in length.
const SOUND_GAP_MS: i64 = 3_000;
/// Fingerprints this alike are the same recording.
pub const SAME_SOUND: f32 = 0.85;
/// Fingerprints less alike than this are different recordings, even with
/// the same name (another live take, say).
const DIFFERENT_SOUND: f32 = 0.65;
/// How far prints may be shifted against each other (about 2 seconds) and
/// how much must overlap (about 12 seconds).
pub const MAX_SHIFT: usize = 16;
pub const MIN_OVERLAP: usize = 100;

/// Why songs were grouped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Same title and artist, about the same length.
    Name,
    /// Identified as the same recording.
    Recording,
    /// They sound the same.
    Sound,
}

/// Copies of one song, the suggested one to keep first.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Group {
    pub songs: Vec<i64>,
    pub reasons: Vec<Reason>,
}

/// Groups of copies among `songs`. `similar` compares two fingerprints
/// (0 to 1); `dismissed` are pairs (smaller id first) the admin said
/// aren't copies. Groups come largest first.
pub fn find(
    songs: &[DuplicateCandidate],
    dismissed: &HashSet<(i64, i64)>,
    similar: impl Fn(&[u32], &[u32]) -> f32,
) -> Vec<Group> {
    let prints: Vec<Option<Vec<u32>>> = songs.iter().map(|s| s.print_values()).collect();
    let mut links = Links::new(songs.len());
    let allowed = |i: usize, j: usize| {
        let (a, b) = (songs[i].id, songs[j].id);
        !dismissed.contains(&(a.min(b), a.max(b)))
    };
    let close = |i: usize, j: usize, gap: i64| match (songs[i].duration_ms, songs[j].duration_ms) {
        (Some(x), Some(y)) => (x - y).abs() <= gap,
        _ => false,
    };
    let sound = |i: usize, j: usize| match (&prints[i], &prints[j]) {
        (Some(x), Some(y)) => Some(similar(x, y)),
        _ => None,
    };

    // By name: same core title, overlapping artists, about as long, unless
    // their prints say they're different recordings.
    let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, song) in songs.iter().enumerate() {
        if song.artist.is_some() && !song.title.trim().is_empty() {
            by_title.entry(core_title(&song.title)).or_default().push(i);
        }
    }
    let names: Vec<Option<HashSet<String>>> = songs
        .iter()
        .map(|s| s.artist.as_deref().map(artist_names))
        .collect();
    for members in by_title.values().filter(|m| m.len() > 1) {
        for (n, &i) in members.iter().enumerate() {
            for &j in &members[n + 1..] {
                let shared = match (&names[i], &names[j]) {
                    (Some(x), Some(y)) => !x.is_disjoint(y),
                    _ => false,
                };
                if shared
                    && close(i, j, NAME_GAP_MS)
                    && allowed(i, j)
                    && sound(i, j).is_none_or(|s| s >= DIFFERENT_SOUND)
                {
                    links.join(i, j, Reason::Name);
                }
            }
        }
    }

    // By identified recording.
    let mut by_recording: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, song) in songs.iter().enumerate() {
        if let Some(recording) = song.recording.as_deref().filter(|r| !r.is_empty()) {
            by_recording.entry(recording).or_default().push(i);
        }
    }
    for members in by_recording.values().filter(|m| m.len() > 1) {
        for (n, &i) in members.iter().enumerate() {
            for &j in &members[n + 1..] {
                if close(i, j, RECORDING_GAP_MS) && allowed(i, j) {
                    links.join(i, j, Reason::Recording);
                }
            }
        }
    }

    // By sound: compare each print with those of about the same length.
    let mut timed: Vec<(i64, usize)> = songs
        .iter()
        .enumerate()
        .filter(|(i, _)| prints[*i].is_some())
        .filter_map(|(i, s)| s.duration_ms.map(|d| (d, i)))
        .collect();
    timed.sort_unstable();
    for (n, &(length, i)) in timed.iter().enumerate() {
        for &(other, j) in &timed[n + 1..] {
            if other - length > SOUND_GAP_MS {
                break;
            }
            if allowed(i, j) && sound(i, j).is_some_and(|s| s >= SAME_SOUND) {
                links.join(i, j, Reason::Sound);
            }
        }
    }

    let mut groups: Vec<Group> = links
        .groups()
        .into_iter()
        .map(|(mut members, reasons)| {
            members.sort_by(|&a, &b| keep_score(&songs[b]).total_cmp(&keep_score(&songs[a])));
            let mut reasons: Vec<Reason> = reasons.into_iter().collect();
            reasons.sort();
            Group {
                songs: members.iter().map(|&i| songs[i].id).collect(),
                reasons,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.songs
            .len()
            .cmp(&a.songs.len())
            .then(a.songs.cmp(&b.songs))
    });
    groups
}

/// Formats that keep every bit of the original.
fn lossless(codec: Option<&str>) -> bool {
    codec.is_some_and(|c| {
        matches!(
            c.to_ascii_lowercase().as_str(),
            "flac" | "wav" | "aiff" | "ape" | "wavpack" | "alac"
        )
    })
}

/// Which copy to keep: lossless first, then the higher bit rate, then the
/// better tagged.
fn keep_score(song: &DuplicateCandidate) -> f64 {
    let kbps = match song.duration_ms {
        Some(ms) if ms > 0 => song.size as f64 * 8.0 / ms as f64,
        _ => 0.0,
    };
    let tagged = [song.album.is_some(), song.year.is_some(), song.has_art]
        .iter()
        .filter(|&&t| t)
        .count();
    let lossless = if lossless(song.codec.as_deref()) {
        1.0
    } else {
        0.0
    };
    // Bit rates stay well under 10,000 kbps, so each part outranks the next.
    lossless * 1e9 + kbps.min(9_999.0) * 1e3 + tagged as f64
}

/// Which songs are linked, and why (union-find).
struct Links {
    parent: Vec<usize>,
    reasons: HashMap<usize, HashSet<Reason>>,
}

impl Links {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            reasons: HashMap::new(),
        }
    }

    fn root(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn join(&mut self, a: usize, b: usize, reason: Reason) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra != rb {
            self.parent[rb] = ra;
            let moved = self.reasons.remove(&rb).unwrap_or_default();
            self.reasons.entry(ra).or_default().extend(moved);
        }
        self.reasons.entry(ra).or_default().insert(reason);
    }

    fn groups(mut self) -> Vec<(Vec<usize>, HashSet<Reason>)> {
        let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..self.parent.len() {
            let root = self.root(i);
            members.entry(root).or_default().push(i);
        }
        members
            .into_iter()
            .filter(|(_, m)| m.len() > 1)
            .map(|(root, m)| (m, self.reasons.remove(&root).unwrap_or_default()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: i64, title: &str, artist: &str, ms: i64) -> DuplicateCandidate {
        DuplicateCandidate {
            id,
            path: format!("/m/{id}.mp3"),
            title: title.into(),
            artist: Some(artist.into()),
            duration_ms: Some(ms),
            size: 5_000_000,
            codec: Some("mpeg".into()),
            ..Default::default()
        }
    }

    fn print(values: &[u32]) -> Option<Vec<u8>> {
        Some(values.iter().flat_map(|v| v.to_le_bytes()).collect())
    }

    /// Prints are "the same" when their first value matches.
    fn same_first(a: &[u32], b: &[u32]) -> f32 {
        if a.first() == b.first() {
            1.0
        } else {
            0.0
        }
    }

    #[test]
    fn finds_copies_by_name_and_keeps_the_best() {
        let mut flac = song(2, "Song (Remastered 2011)", "The Band", 200_500);
        flac.codec = Some("flac".into());
        flac.size = 30_000_000;
        let songs = [
            song(1, "Song", "Band", 200_000),
            flac,
            song(3, "Song (Live)", "Band", 200_000),
            song(4, "Song", "Band", 260_000),
            song(5, "Song", "Someone Else", 200_000),
        ];
        let groups = find(&songs, &HashSet::new(), same_first);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].songs, [2, 1], "lossless first");
        assert_eq!(groups[0].reasons, [Reason::Name]);
    }

    #[test]
    fn sound_finds_untagged_copies_and_vetoes_namesakes() {
        let mut a = song(1, "Track 01", "Unknown", 180_000);
        a.print = print(&[7, 1]);
        let mut b = song(2, "Intro", "Someone", 181_000);
        b.print = print(&[7, 2]);
        // Same name, but another recording.
        let mut c = song(3, "Track 01", "Unknown", 180_500);
        c.print = print(&[9, 1]);
        let groups = find(&[a, b, c], &HashSet::new(), same_first);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].songs.len(), 2);
        assert!(groups[0].songs.contains(&1) && groups[0].songs.contains(&2));
        assert_eq!(groups[0].reasons, [Reason::Sound]);
    }

    #[test]
    fn identified_recordings_group_and_dismissals_hold() {
        let mut a = song(1, "One", "X", 100_000);
        a.recording = Some("rec".into());
        let mut b = song(2, "Uno", "Y", 103_000);
        b.recording = Some("rec".into());
        let groups = find(&[a.clone(), b.clone()], &HashSet::new(), same_first);
        assert_eq!(groups[0].reasons, [Reason::Recording]);

        let dismissed = HashSet::from([(1, 2)]);
        assert!(find(&[a, b], &dismissed, same_first).is_empty());
    }
}
