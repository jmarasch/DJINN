//! Finds a song named by an imported playlist in the library. Strict: a
//! flagged gap is easy to see and fill, but a wrong song playing at the party
//! isn't. Tags differ between services in small ways, so "Song (Remastered
//! 2011)" finds "Song", but "Song (Live)" doesn't.

use std::collections::{HashMap, HashSet};

use djinn_store::{SongFacts, WantedSong};
use unicode_normalization::UnicodeNormalization;

/// Songs further apart than this are different cuts of a song.
const MAX_LENGTH_GAP_MS: i64 = 30_000;

/// Words in brackets, or after " - ", that don't make it a different song.
const NOISE: &[&str] = &[
    "feat",
    "ft",
    "featuring",
    "with ",
    "remaster",
    "explicit",
    "clean",
    "radio edit",
    "radio mix",
    "radio version",
    "hit radio",
    "original edit",
    "video edit",
    "album version",
    "single version",
    "original version",
    "original mix",
    "stereo",
    "mono",
    "official",
    "lyric",
    "audio",
    "music video",
    "hd",
    "hq",
    "4k",
    "bonus track",
    "deluxe",
    "anniversary",
    "edition",
];

/// Lowercase with accents, punctuation and extra spaces gone, `&` as "and".
fn folded(text: &str) -> String {
    let text = text.replace('&', " and ").to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.nfkd() {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        } else if c.is_whitespace() || c == '-' || c == '/' || c == '_' {
            space = true;
        }
    }
    out
}

fn is_noise(text: &str) -> bool {
    let text = text.to_lowercase();
    NOISE.iter().any(|noise| {
        text.match_indices(noise).any(|(at, word)| {
            // Whole words: "hd" isn't in "ahdoo", "clean" isn't in "cleaner".
            let before = text[..at].chars().next_back();
            let after = text[at + word.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric)
                && (word.ends_with(' ')
                    || word == "remaster"
                    || !after.is_some_and(char::is_alphanumeric))
        })
    })
}

/// The title without what only says which release it's from.
pub(crate) fn core_title(title: &str) -> String {
    let mut text = title.trim().to_owned();
    // Bracketed notes, from the end.
    loop {
        let trimmed = text.trim_end();
        let Some(close) = trimmed
            .chars()
            .next_back()
            .filter(|c| matches!(c, ')' | ']'))
        else {
            break;
        };
        let open = if close == ')' { '(' } else { '[' };
        let Some(at) = trimmed.rfind(open) else { break };
        // A title that's all brackets stays as it is.
        if at == 0 || !is_noise(&trimmed[at + 1..trimmed.len() - 1]) {
            break;
        }
        text = trimmed[..at].trim_end().to_owned();
    }
    // "Song - Remastered 2011" and "Song feat. Someone".
    for dash in [" - ", " \u{2013} ", " \u{2014} "] {
        if let Some((song, note)) = text.rsplit_once(dash) {
            if is_noise(note) && !song.trim().is_empty() {
                text = song.to_owned();
            }
        }
    }
    let lower = text.to_lowercase();
    for mark in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(at) = lower.find(mark) {
            if text.is_char_boundary(at) {
                text.truncate(at);
                break;
            }
        }
    }
    let core = folded(&text);
    if core.is_empty() {
        // All punctuation or symbols: the title is what it is.
        return title.trim().to_lowercase();
    }
    core
}

/// The names in an artist credit, comparable: "The Beatles & Billy Preston"
/// holds "beatles" and "billy preston".
pub(crate) fn artist_names(credit: &str) -> HashSet<String> {
    let mut lower = credit.replace('\u{a0}', " ").to_lowercase();
    for word in [
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " with ",
        " x ",
        " vs. ",
        " vs ",
    ] {
        lower = lower.replace(word, ",");
    }
    let mut names: HashSet<String> = lower
        .split([',', ';', '&', '/', '+'])
        .chain([lower.as_str()])
        .map(|name| {
            let name = folded(name);
            name.strip_prefix("the ").map(str::to_owned).unwrap_or(name)
        })
        .filter(|name| !name.is_empty())
        .collect();
    // Whatever's left of "A and B" counts too.
    let whole = folded(&lower);
    for part in whole.split(" and ") {
        let part = part.strip_prefix("the ").unwrap_or(part);
        if !part.is_empty() {
            names.insert(part.to_owned());
        }
    }
    names
}

/// The last two parts of a path, which survive moving a music folder.
fn path_tail(path: &str) -> Option<String> {
    let mut parts = path.rsplit(['/', '\\']).filter(|p| !p.is_empty());
    let file = parts.next()?;
    let folder = parts.next()?;
    Some(format!("{folder}/{file}").to_lowercase())
}

struct Song {
    id: i64,
    title: String,
    artists: HashSet<String>,
    duration_ms: Option<i64>,
}

/// The library, ready to look songs up in.
pub struct SongIndex {
    songs: Vec<Song>,
    by_core: HashMap<String, Vec<usize>>,
    by_path: HashMap<String, i64>,
    by_tail: HashMap<String, Vec<i64>>,
}

impl SongIndex {
    pub fn new(facts: Vec<SongFacts>) -> Self {
        let mut index = Self {
            songs: Vec::with_capacity(facts.len()),
            by_core: HashMap::new(),
            by_path: HashMap::new(),
            by_tail: HashMap::new(),
        };
        for fact in facts {
            index.by_path.insert(fact.path.to_lowercase(), fact.id);
            if let Some(tail) = path_tail(&fact.path) {
                index.by_tail.entry(tail).or_default().push(fact.id);
            }
            let mut artists = HashSet::new();
            for credit in [&fact.artist, &fact.album_artist].into_iter().flatten() {
                artists.extend(artist_names(credit));
            }
            index
                .by_core
                .entry(core_title(&fact.title))
                .or_default()
                .push(index.songs.len());
            index.songs.push(Song {
                id: fact.id,
                title: folded(&fact.title),
                artists,
                duration_ms: fact.duration_ms,
            });
        }
        index
    }

    /// The library's song for this one, if there's no doubt which it is.
    pub fn find(&self, wanted: &WantedSong) -> Option<i64> {
        if let Some(found) = wanted.path.as_deref().and_then(|p| self.by_file(p)) {
            return Some(found);
        }
        let artist = wanted.artist.as_deref();
        self.by_name(&wanted.title, artist, wanted.duration_ms)
            .or_else(|| {
                let other = artist.filter(|_| wanted.either_way)?;
                self.by_name(other, Some(&wanted.title), wanted.duration_ms)
            })
    }

    fn by_file(&self, path: &str) -> Option<i64> {
        if let Some(&id) = self.by_path.get(&path.replace('\\', "/").to_lowercase()) {
            return Some(id);
        }
        if let Some(&id) = self.by_path.get(&path.to_lowercase()) {
            return Some(id);
        }
        match self.by_tail.get(&path_tail(path)?)?.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    fn by_name(&self, title: &str, artist: Option<&str>, duration_ms: Option<i64>) -> Option<i64> {
        let candidates = self.by_core.get(&core_title(title))?;
        let wanted_artists = artist.map(artist_names);
        let exact = folded(title);
        let gap = |song: &Song| match (song.duration_ms, duration_ms) {
            (Some(a), Some(b)) => Some((a - b).abs()),
            _ => None,
        };
        let fitting: Vec<&Song> = candidates
            .iter()
            .map(|&i| &self.songs[i])
            .filter(|song| {
                wanted_artists
                    .as_ref()
                    .is_none_or(|names| !names.is_disjoint(&song.artists))
            })
            .filter(|song| gap(song).is_none_or(|g| g <= MAX_LENGTH_GAP_MS))
            .collect();
        // A title alone is a match only if just one artist has it.
        if wanted_artists.is_none() {
            let mut artists = fitting.iter().map(|s| {
                let mut names: Vec<&String> = s.artists.iter().collect();
                names.sort();
                names
            });
            let first = artists.next()?;
            if artists.any(|names| names != first) {
                return None;
            }
        }
        fitting
            .into_iter()
            .min_by_key(|song| (song.title != exact, gap(song).unwrap_or(i64::MAX), song.id))
            .map(|song| song.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: i64, title: &str, artist: &str, duration_ms: Option<i64>) -> SongFacts {
        SongFacts {
            id,
            path: format!("/music/{artist}/{title}.flac"),
            title: title.into(),
            artist: Some(artist.into()),
            album_artist: None,
            duration_ms,
        }
    }

    fn wanted(title: &str, artist: &str) -> WantedSong {
        WantedSong {
            title: title.into(),
            artist: Some(artist.into()),
            ..Default::default()
        }
    }

    fn library() -> SongIndex {
        SongIndex::new(vec![
            fact(1, "S&M", "Rihanna", Some(243_000)),
            fact(2, "Numb", "Marshmello & Khalid", Some(155_000)),
            fact(
                3,
                "More and More",
                "Captain Hollywood Project",
                Some(253_000),
            ),
            fact(4, "Hey Jude", "The Beatles", Some(431_000)),
            fact(5, "Hey Jude (Live)", "The Beatles", Some(500_000)),
            fact(6, "Björk's Song", "Björk", None),
            fact(7, "Halo", "Beyoncé", Some(261_000)),
            fact(8, "Halo", "Depeche Mode", Some(270_000)),
            fact(9, "Sigh No More", "Mumford & Sons", Some(208_000)),
            fact(
                10,
                "Teardrop (Remastered 2019)",
                "Massive Attack",
                Some(330_000),
            ),
        ])
    }

    #[test]
    fn the_shared_playlists_quirks_match() {
        let index = library();
        // As Spotify names them: extra note, no-break space between artists.
        assert_eq!(
            index.find(&wanted(
                "More And More - Single Version",
                "Captain Hollywood Project"
            )),
            Some(3)
        );
        assert_eq!(
            index.find(&wanted("Numb", "Marshmello,\u{a0}Khalid")),
            Some(2)
        );
        assert_eq!(index.find(&wanted("S&M", "Rihanna")), Some(1));
        assert_eq!(index.find(&wanted("S & M", "Rihanna")), Some(1));
    }

    #[test]
    fn small_differences_in_tags_dont_matter() {
        let index = library();
        assert_eq!(index.find(&wanted("hey jude", "Beatles")), Some(4));
        assert_eq!(
            index.find(&wanted("Hey Jude - Remastered 2015", "The Beatles")),
            Some(4)
        );
        assert_eq!(
            index.find(&wanted("Hey Jude [2015 Remaster]", "The Beatles")),
            Some(4)
        );
        assert_eq!(index.find(&wanted("Bjork's Song", "Bjork")), Some(6));
        assert_eq!(index.find(&wanted("Halo", "Beyonce")), Some(7));
        assert_eq!(
            index.find(&wanted("Sigh No More (feat. Someone)", "Mumford and Sons")),
            Some(9)
        );
        assert_eq!(index.find(&wanted("Teardrop", "Massive Attack")), Some(10));
        // Notes seen on a real YouTube Music playlist.
        for title in [
            "Hey Jude (Radio Mix)",
            "Hey Jude - Radio Mix",
            "Hey Jude (Original Edit)",
            "Hey Jude (Radio Version)",
            "Hey Jude (Video Edit)",
            "Hey Jude - Gabry Ponte Video Edit",
            "Hey Jude (1999 Original Hit Radio)",
        ] {
            assert_eq!(
                index.find(&wanted(title, "The Beatles")),
                Some(4),
                "{title}"
            );
        }
    }

    #[test]
    fn another_version_or_artist_isnt_the_song() {
        let index = library();
        assert_eq!(
            index.find(&wanted("Hey Jude (Live)", "The Beatles")),
            Some(5)
        );
        assert_eq!(
            index.find(&wanted("Hey Jude (Acoustic)", "The Beatles")),
            None
        );
        assert_eq!(index.find(&wanted("Halo", "Someone Else")), None);
        assert_eq!(index.find(&wanted("Not In The Library", "Rihanna")), None);
    }

    #[test]
    fn a_much_longer_cut_isnt_the_song() {
        let index = library();
        let mut extended = wanted("Hey Jude", "The Beatles");
        extended.duration_ms = Some(431_000 + MAX_LENGTH_GAP_MS + 1);
        assert_eq!(index.find(&extended), None);
        extended.duration_ms = Some(431_000 + 2000);
        assert_eq!(index.find(&extended), Some(4));
    }

    #[test]
    fn a_title_alone_needs_to_be_clear() {
        let index = library();
        let only_title = |title: &str| WantedSong {
            title: title.into(),
            ..Default::default()
        };
        assert_eq!(index.find(&only_title("S&M")), Some(1));
        assert_eq!(
            index.find(&only_title("Halo")),
            None,
            "two artists have one"
        );
    }

    #[test]
    fn text_that_could_be_either_way_round() {
        let index = library();
        let mut text = wanted("Rihanna", "S&M");
        assert_eq!(index.find(&text), None);
        text.either_way = true;
        assert_eq!(index.find(&text), Some(1));
    }

    #[test]
    fn files_named_by_other_players() {
        let index = library();
        let by_path = |path: &str| WantedSong {
            title: "Something else".into(),
            path: Some(path.into()),
            ..Default::default()
        };
        assert_eq!(index.find(&by_path("/music/Rihanna/S&M.flac")), Some(1));
        assert_eq!(index.find(&by_path("D:\\Old\\Rihanna\\S&M.flac")), Some(1));
        assert_eq!(index.find(&by_path("/elsewhere/Other/S&M.flac")), None);
    }

    #[test]
    fn titles_of_symbols_still_compare() {
        let index = SongIndex::new(vec![fact(1, "...", "Someone", None)]);
        assert_eq!(index.find(&wanted("...", "Someone")), Some(1));
        assert_eq!(index.find(&wanted("!!!", "Someone")), None);
    }

    #[test]
    fn words_inside_other_words_arent_noise() {
        assert!(!is_noise("Cleaner"));
        assert!(is_noise("2011 Remaster"));
        assert!(is_noise("Remastered 2011"));
        assert!(is_noise("feat. Someone"));
        assert!(!is_noise("Live"));
        assert_eq!(core_title("Clean (Cleaner Mix)"), "clean cleaner mix");
    }
}
