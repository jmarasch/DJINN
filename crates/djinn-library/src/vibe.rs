//! Searching by description: "80's hair metal", "chill house", "upbeat
//! songs like Zara Larsson, nothing too slow".
//!
//! [`parse`] turns the words into an [`Intent`] (decades, genres, tempo,
//! energy, mood, artists to sound like), and [`rank`] finds the songs that
//! fit, using genres from tags and MusicBrainz, years, and what the
//! analyzer heard. A query with nothing recognizable isn't a description
//! and gets `None`, so plain title and artist search stays as it was.

use std::collections::HashSet;

use crate::sound::SoundIndex;

/// What a description asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Intent {
    /// Each group is one genre asked for, as the genre names that count
    /// for it; a song must fit every group.
    pub genres: Vec<Vec<String>>,
    /// Neighboring genres ("hard rock" for hair metal): a song with only
    /// these counts for a little, so it fills in after the real fits.
    pub near: Vec<String>,
    /// The years a genre asked for belongs to ("classic rock"), when no
    /// years were asked for.
    pub genre_era: Option<(i64, i64)>,
    /// Genres ruled out ("no rap"), grouped the same way; a song fitting
    /// any group is left out.
    pub not_genres: Vec<Vec<String>>,
    /// Inclusive year range.
    pub years: Option<(i64, i64)>,
    /// BPM range.
    pub tempo: Option<(f32, f32)>,
    pub energy: Option<Level>,
    pub mood: Option<Mood>,
    /// Artists whose sound to match.
    pub like_artists: Vec<i64>,
    /// What was understood, for showing back ("1980s · hair metal").
    pub summary: Vec<String>,
    /// Words that weren't understood (a language model can take these).
    pub unknown: Vec<String>,
}

impl Intent {
    pub fn is_empty(&self) -> bool {
        self.genres.is_empty()
            && self.not_genres.is_empty()
            && self.years.is_none()
            && self.tempo.is_none()
            && self.energy.is_none()
            && self.mood.is_none()
            && self.like_artists.is_empty()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    High,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mood {
    Happy,
    Sad,
}

/// What the library knows about: its genres and artists.
#[derive(Default)]
pub struct Vocabulary {
    /// Genre names in the library, normalized.
    genres: HashSet<String>,
    /// (artist id, normalized name).
    artists: Vec<(i64, String)>,
}

impl Vocabulary {
    pub fn new<'a>(
        genres: impl IntoIterator<Item = &'a str>,
        artists: impl IntoIterator<Item = (i64, &'a str)>,
    ) -> Self {
        let mut artists: Vec<(i64, String)> = artists
            .into_iter()
            .map(|(id, name)| (id, normalize(name)))
            .filter(|(_, name)| !name.is_empty())
            .collect();
        // Longest names first, so "zara larsson" wins over "zara".
        artists.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));
        Self {
            genres: genres
                .into_iter()
                .map(normalize)
                .filter(|g| !g.is_empty())
                .collect(),
            artists,
        }
    }
}

/// One song, as search sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SongFacts {
    pub id: i64,
    pub year: Option<i64>,
    /// The year it belongs to, when that isn't its own: a greatest hits,
    /// live album or reissue goes by when its artist made their records.
    pub era: Option<i64>,
    /// Normalized genre names: its own, its album's and its artists'.
    pub genres: Vec<String>,
    /// The song's own and its album's genres, which say the most.
    pub own_genres: Vec<String>,
    /// Genres the AI heard in it, which say the least.
    pub styles: Vec<String>,
    /// The artist it's by (not a featured one).
    pub artist: Option<i64>,
    pub popularity: i64,
    pub bpm: Option<f32>,
    /// Onset strength (features[1]).
    pub punch: Option<f32>,
    /// Major minus minor fit (features[11]).
    pub mode: Option<f32>,
}

/// Lowercase, hyphens and odd quotes as spaces, single spaces.
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .replace(['\u{2019}', '\u{2018}', '`'], "'")
        .replace(['-', '_', '/'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Everyday genre names and the tag or MusicBrainz names that count for
/// them. Names the library itself uses also work as they are.
const GENRE_WORDS: &[(&str, &[&str])] = &[
    ("hair metal", &["glam metal", "hair metal"]),
    ("glam metal", &["glam metal", "hair metal", "glam rock"]),
    ("glam", &["glam metal", "glam rock", "glam punk"]),
    ("metal", &["metal"]),
    (
        "classic rock",
        &["classic rock", "album rock", "blues rock"],
    ),
    ("rock", &["rock"]),
    ("punk", &["punk"]),
    ("emo", &["emo"]),
    ("grunge", &["grunge"]),
    ("goth", &["gothic rock", "goth", "darkwave", "deathrock"]),
    ("hip hop", &["hip hop", "rap", "trap"]),
    ("hiphop", &["hip hop", "rap", "trap"]),
    ("rap", &["rap", "hip hop", "trap"]),
    ("rnb", &["r&b", "rnb", "soul"]),
    ("r&b", &["r&b", "rnb", "soul"]),
    ("r and b", &["r&b", "rnb", "soul"]),
    (
        "edm",
        &[
            "edm",
            "electro",
            "house",
            "dubstep",
            "trance",
            "future rave",
            "big room",
        ],
    ),
    (
        "electronic",
        &[
            "electronic",
            "electro",
            "electropop",
            "synth pop",
            "house",
            "techno",
            "trance",
            "edm",
        ],
    ),
    (
        "dance",
        &["dance", "dance pop", "edm", "house", "disco", "club"],
    ),
    ("club", &["club", "house", "dance", "edm"]),
    ("house", &["house"]),
    ("techno", &["techno"]),
    ("trance", &["trance"]),
    ("dubstep", &["dubstep"]),
    (
        "drum and bass",
        &[
            "drum and bass",
            "drum & bass",
            "dnb",
            "jungle",
            "liquid funk",
        ],
    ),
    (
        "dnb",
        &[
            "drum and bass",
            "drum & bass",
            "dnb",
            "jungle",
            "liquid funk",
        ],
    ),
    ("synthwave", &["synthwave", "retrowave"]),
    ("synth", &["synth pop", "synthwave", "electropop"]),
    ("new wave", &["new wave"]),
    ("pop", &["pop"]),
    ("kpop", &["k pop"]),
    ("k pop", &["k pop"]),
    ("indie", &["indie"]),
    ("alternative", &["alternative"]),
    ("country", &["country"]),
    ("folk", &["folk"]),
    ("jazz", &["jazz"]),
    ("blues", &["blues"]),
    ("soul", &["soul", "r&b"]),
    ("funk", &["funk"]),
    ("disco", &["disco"]),
    ("reggae", &["reggae", "dancehall"]),
    ("latin", &["latin", "reggaeton", "salsa", "bachata"]),
    ("reggaeton", &["reggaeton"]),
    ("afrobeat", &["afrobeat", "afrobeats"]),
    ("afrobeats", &["afrobeats", "afrobeat"]),
    ("lofi", &["lo fi", "lofi", "chillhop"]),
    ("lo fi", &["lo fi", "lofi", "chillhop"]),
    (
        "classical",
        &["classical", "orchestral", "baroque", "romantic"],
    ),
    ("soundtrack", &["soundtrack", "score", "film score"]),
    ("christmas", &["christmas", "holiday"]),
    ("ska", &["ska"]),
    ("gospel", &["gospel"]),
];

/// Genres next to an everyday name: a song with only these fits a little.
const NEAR_GENRES: &[(&str, &[&str])] = &[
    (
        "hair metal",
        &[
            "glam rock",
            "hard rock",
            "heavy metal",
            "arena rock",
            "glam punk",
        ],
    ),
    ("glam metal", &["hard rock", "heavy metal", "arena rock"]),
    (
        "classic rock",
        &[
            "hard rock",
            "arena rock",
            "soft rock",
            "southern rock",
            "rock",
        ],
    ),
    ("goth", &["industrial", "post punk"]),
    ("synthwave", &["synth pop", "new wave", "electropop"]),
    ("synth", &["new wave"]),
    ("new wave", &["synth pop", "post punk"]),
    ("disco", &["funk", "soul", "boogie", "dance"]),
];

/// Genres that belong to an era: a song from outside it fits less, unless
/// the request names its own years.
const GENRE_ERAS: &[(&str, i64, i64)] = &[
    ("classic rock", 1960, 1989),
    ("hair metal", 1980, 1993),
    ("grunge", 1987, 1999),
    ("disco", 1972, 1984),
    ("new wave", 1976, 1990),
];
/// How much a song from outside its genre's era counts.
const OUT_OF_ERA: f32 = 0.4;

/// Words that say nothing about the music.
const FILLER: &[&str] = &[
    "music",
    "songs",
    "song",
    "tracks",
    "track",
    "stuff",
    "some",
    "the",
    "and",
    "with",
    "of",
    "a",
    "an",
    "for",
    "me",
    "play",
    "give",
    "something",
    "kind",
    "type",
    "style",
    "vibe",
    "vibes",
    "era",
    "from",
    "in",
    "please",
    "good",
    "best",
    "great",
    "only",
    "just",
    "mix",
    "playlist",
    "any",
    "that",
    "is",
    "are",
    "all",
    "or",
    "to",
    "my",
    "our",
    "us",
    "by",
    "but",
];

const FAST: &[&str] = &[
    "fast",
    "upbeat",
    "uptempo",
    "up tempo",
    "high energy",
    "energetic",
    "party",
    "hype",
    "pumping",
    "banger",
    "bangers",
];
const SLOW: &[&str] = &[
    "slow",
    "chill",
    "chilled",
    "mellow",
    "relaxing",
    "relaxed",
    "calm",
    "laid back",
    "downtempo",
    "quiet",
    "soft",
    "sleepy",
];
const SAD: &[&str] = &[
    "sad",
    "melancholy",
    "melancholic",
    "moody",
    "dark",
    "emotional",
    "heartbreak",
    "breakup",
];
const HAPPY: &[&str] = &[
    "happy",
    "feel good",
    "feelgood",
    "uplifting",
    "sunny",
    "cheerful",
    "joyful",
    "summer",
    "summery",
];

/// Understands what it can of `query`. `None` if nothing in it describes
/// music (so it's a plain search).
pub fn parse(query: &str, vocab: &Vocabulary) -> Option<Intent> {
    let words: Vec<String> = normalize(query)
        .split(|c: char| c.is_whitespace() || c == ',' || c == '.' || c == '!' || c == '?')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    let mut intent = Intent::default();
    let mut i = 0;
    while i < words.len() {
        let rest = &words[i..];
        let phrase = |n: usize| rest[..n.min(rest.len())].join(" ");

        // "nothing too slow", "not too fast"
        if let Some(n) = match_negated_tempo(rest, &mut intent) {
            i += n;
            continue;
        }
        // "no rap", "nothing country", "without metal"
        if let Some(n) = match_negated_genre(rest, vocab, &mut intent) {
            i += n;
            continue;
        }
        // "like X", "similar to X", "sounds like X"
        let lead = if rest.first().map(String::as_str) == Some("like") {
            1
        } else if rest.len() > 1
            && (rest[0] == "similar" && rest[1] == "to" || rest[0] == "sounds" && rest[1] == "like")
        {
            2
        } else {
            0
        };
        if lead > 0 {
            let after = rest[lead..].join(" ");
            if let Some((id, name)) = vocab
                .artists
                .iter()
                .find(|(_, name)| after == *name || after.starts_with(&format!("{name} ")))
            {
                intent.like_artists.push(*id);
                intent.summary.push(format!("like {name}"));
                i += lead + name.split(' ').count();
                continue;
            }
        }
        // Decades and years.
        if let Some(n) = match_years(rest, &mut intent) {
            i += n;
            continue;
        }
        // Tempo, energy, mood words (longest phrases first).
        let mut matched = false;
        for n in (1..=3).rev() {
            if n > rest.len() {
                continue;
            }
            let p = phrase(n);
            let p = p.as_str();
            if FAST.contains(&p) {
                intent.energy = Some(Level::High);
                intent.summary.push(p.to_owned());
            } else if SLOW.contains(&p) {
                intent.energy = Some(Level::Low);
                intent.summary.push(p.to_owned());
            } else if SAD.contains(&p) {
                intent.mood = Some(Mood::Sad);
                intent.summary.push(p.to_owned());
            } else if HAPPY.contains(&p) {
                intent.mood = Some(Mood::Happy);
                intent.summary.push(p.to_owned());
            } else if let Some(group) = genre_group(p, vocab) {
                intent.genres.push(group);
                intent.near.extend(near_group(p));
                intent.genre_era = intent.genre_era.or(genre_era(p));
                intent.summary.push(p.to_owned());
            } else {
                continue;
            }
            i += n;
            matched = true;
            break;
        }
        if matched {
            continue;
        }
        if !FILLER.contains(&rest[0].as_str()) {
            intent.unknown.push(rest[0].clone());
        }
        i += 1;
    }
    (!intent.is_empty()).then_some(intent)
}

/// The genre names that count for `phrase`: from the everyday list, or
/// the phrase itself if the library has that genre.
fn genre_group(phrase: &str, vocab: &Vocabulary) -> Option<Vec<String>> {
    if let Some((_, names)) = GENRE_WORDS.iter().find(|(word, _)| *word == phrase) {
        return Some(names.iter().map(|n| normalize(n)).collect());
    }
    let singular = phrase.strip_suffix('s').unwrap_or(phrase);
    [phrase, singular]
        .into_iter()
        .find(|p| vocab.genres.contains(*p))
        .map(|p| vec![p.to_owned()])
}

/// The neighboring genres of an everyday genre name.
fn near_group(phrase: &str) -> Vec<String> {
    near_genres(phrase)
}

/// The years an everyday genre name belongs to, if it does.
fn genre_era(phrase: &str) -> Option<(i64, i64)> {
    GENRE_ERAS
        .iter()
        .find(|(word, _, _)| *word == phrase)
        .map(|(_, from, to)| (*from, *to))
}

fn near_genres(phrase: &str) -> Vec<String> {
    NEAR_GENRES
        .iter()
        .find(|(word, _)| *word == phrase)
        .map(|(_, names)| names.iter().map(|n| normalize(n)).collect())
        .unwrap_or_default()
}

fn match_negated_tempo(rest: &[String], intent: &mut Intent) -> Option<usize> {
    let lead = match rest.first().map(String::as_str) {
        Some("nothing" | "not" | "no") => 1,
        _ => return None,
    };
    let mut n = lead;
    if rest.get(n).map(String::as_str) == Some("too") {
        n += 1;
    }
    match rest.get(n).map(String::as_str) {
        Some("slow" | "chill" | "mellow") => {
            let (_, max) = intent.tempo.unwrap_or((0.0, 250.0));
            intent.tempo = Some((95.0, max));
            intent.summary.push("not too slow".into());
        }
        Some("fast" | "upbeat" | "hype") => {
            let (min, _) = intent.tempo.unwrap_or((0.0, 250.0));
            intent.tempo = Some((min, 130.0));
            intent.summary.push("not too fast".into());
        }
        _ => return None,
    }
    Some(n + 1)
}

fn match_negated_genre(rest: &[String], vocab: &Vocabulary, intent: &mut Intent) -> Option<usize> {
    if !matches!(
        rest.first().map(String::as_str),
        Some("no" | "not" | "nothing" | "without")
    ) {
        return None;
    }
    let lead = if rest.get(1).map(String::as_str) == Some("too") {
        2
    } else {
        1
    };
    (1..=3).rev().find_map(|n| {
        let words = rest.get(lead..lead + n)?;
        let phrase = words.join(" ");
        let group = genre_group(&phrase, vocab)?;
        intent.summary.push(format!("no {phrase}"));
        intent.not_genres.push(group);
        Some(lead + n)
    })
}

fn match_years(rest: &[String], intent: &mut Intent) -> Option<usize> {
    let mut n = 0;
    let part = match rest.first().map(String::as_str) {
        Some("early") => Some(0),
        Some("mid") => Some(1),
        Some("late") => Some(2),
        _ => None,
    };
    if part.is_some() {
        n += 1;
    }
    let word = rest.get(n)?;
    let (from, to, label) = if let Some(decade) = decade(word) {
        let (from, to) = match part {
            Some(0) => (decade, decade + 3),
            Some(1) => (decade + 3, decade + 6),
            Some(2) => (decade + 7, decade + 9),
            _ => (decade, decade + 9),
        };
        let label = match part {
            Some(0) => format!("early {decade}s"),
            Some(1) => format!("mid {decade}s"),
            Some(2) => format!("late {decade}s"),
            _ => format!("{decade}s"),
        };
        (from, to, label)
    } else if part.is_none() {
        // "1995", "1990 1995" (a hyphen was turned into a space), "since 2010".
        let year = |w: &str| w.parse::<i64>().ok().filter(|y| (1900..=2099).contains(y));
        let qualifier = match word.as_str() {
            "since" | "after" => Some(true),
            "before" => Some(false),
            _ => None,
        };
        if let Some(after) = qualifier {
            let y = year(rest.get(n + 1)?)?;
            n += 1;
            if after {
                (y, 2100, format!("since {y}"))
            } else {
                (1900, y - 1, format!("before {y}"))
            }
        } else {
            let from = year(word)?;
            match rest.get(n + 1).and_then(|w| year(w)) {
                Some(to) if to >= from => {
                    n += 1;
                    (from, to, format!("{from} to {to}"))
                }
                _ => (from, from, from.to_string()),
            }
        }
    } else {
        return None;
    };
    intent.years = Some((from, to));
    intent.summary.push(label);
    Some(n + 1)
}

/// "80s", "80's", "'80s", "1980s", "eighties" to 1980.
fn decade(word: &str) -> Option<i64> {
    let names = [
        ("fifties", 1950),
        ("sixties", 1960),
        ("seventies", 1970),
        ("eighties", 1980),
        ("nineties", 1990),
        ("noughties", 2000),
    ];
    if let Some((_, year)) = names.iter().find(|(name, _)| *name == word) {
        return Some(*year);
    }
    let digits = word
        .trim_start_matches('\'')
        .strip_suffix("'s")
        .or_else(|| word.trim_start_matches('\'').strip_suffix('s'))?;
    let value: i64 = digits.parse().ok()?;
    match digits.len() {
        4 if (1900..=2090).contains(&value) && value % 10 == 0 => Some(value),
        2 if value % 10 == 0 => Some(if value <= 20 {
            2000 + value
        } else {
            1900 + value
        }),
        _ => None,
    }
}

/// What a language model read in a request (DJINN's AI reader).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asked {
    pub genres: Vec<String>,
    pub not_genres: Vec<String>,
    pub years: Option<(i64, i64)>,
    /// "slow", "medium", or "fast".
    pub tempo: Option<String>,
    pub mood: Option<String>,
    pub like_artists: Vec<String>,
}

/// Fills in what the phrase reader missed in `request` (its `phrase`
/// reading) with what a language model read. Years, tempo, and mood the
/// phrase reader found stay. Genres come from the model, which read the
/// whole sentence ("first dance" isn't dance music), as long as DJINN
/// knows them (or the last words of one, "80s rock" as rock). Artists and
/// exclusions count only if the request names them (small models make
/// artists up), and the model's tempo guess only when nothing else
/// narrows the songs down.
pub fn merge(
    request: &str,
    phrase: Option<Intent>,
    asked: &Asked,
    vocab: &Vocabulary,
) -> Option<Intent> {
    let words = format!(
        " {} ",
        normalize(&request.replace([',', '.', '!', '?'], " "))
    );
    let named = |name: &str| !name.is_empty() && words.contains(&format!(" {name} "));
    let mut intent = phrase.unwrap_or_default();
    // The model read those.
    intent.unknown.clear();
    let mut energy: Option<(Level, String)> = None;
    let mut mood: Option<(Mood, String)> = None;
    let mut feel = |word: &str| -> bool {
        let w = word.to_owned();
        if FAST.contains(&word) || word == "energetic" {
            energy.get_or_insert((Level::High, w));
        } else if SLOW.contains(&word) {
            energy.get_or_insert((Level::Low, w));
        } else if SAD.contains(&word) {
            mood.get_or_insert((Mood::Sad, w));
        } else if HAPPY.contains(&word) {
            mood.get_or_insert((Mood::Happy, w));
        } else {
            return false;
        }
        true
    };
    if let Some(tempo) = &asked.tempo {
        feel(tempo);
    }
    if let Some(m) = &asked.mood {
        feel(&normalize(m));
    }
    // A genre the library doesn't know, by its last words.
    let known = |genre: &str| -> Option<(String, Vec<String>)> {
        let words: Vec<&str> = genre.split(' ').collect();
        (0..words.len()).find_map(|skip| {
            let p = words[skip..].join(" ");
            genre_group(&p, vocab).map(|g| (p, g))
        })
    };

    let mut group: Vec<String> = Vec::new();
    let mut near: Vec<String> = Vec::new();
    let mut era: Option<(i64, i64)> = None;
    let mut labels: Vec<String> = Vec::new();
    for genre in asked.genres.iter().map(|g| normalize(g)) {
        // Small models file "chill" or "sad" as a genre.
        if feel(&genre) {
            continue;
        }
        if let Some((label, names)) = known(&genre) {
            for name in names {
                if !group.contains(&name) {
                    group.push(name);
                }
            }
            era = era.or(genre_era(&label));
            for name in near_group(&label) {
                if !near.contains(&name) {
                    near.push(name);
                }
            }
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    if !group.is_empty() || asked.genres.is_empty() {
        // The phrase reader's genres were words out of context.
        let dropped: Vec<String> = intent.genres.drain(..).flatten().collect();
        intent.near.clear();
        intent.genre_era = None;
        intent.summary.retain(|label| {
            genre_group(label, vocab).is_none_or(|g| g.iter().all(|n| !dropped.contains(n)))
        });
    }
    if !group.is_empty() {
        intent.genres.push(group);
        intent.near.extend(near);
        intent.genre_era = intent.genre_era.or(era);
        intent.summary.push(labels.join(" or "));
    }
    for genre in asked.not_genres.iter().map(|g| normalize(g)) {
        if !named(&genre) {
            continue;
        }
        if let Some(group) = genre_group(&genre, vocab) {
            if !intent.not_genres.contains(&group) {
                intent.not_genres.push(group);
                intent.summary.push(format!("no {genre}"));
            }
        }
    }
    if let (None, Some((from, to))) = (intent.years, asked.years) {
        intent.years = Some((from, to));
        intent.summary.push(if from % 10 == 0 && to == from + 9 {
            format!("{from}s")
        } else if from == to {
            from.to_string()
        } else {
            format!("{from} to {to}")
        });
    }
    for name in asked.like_artists.iter().map(|a| normalize(a)) {
        if !named(&name) {
            continue;
        }
        if let Some((id, _)) = vocab.artists.iter().find(|(_, n)| *n == name) {
            if !intent.like_artists.contains(id) {
                intent.like_artists.push(*id);
                intent.summary.push(format!("like {name}"));
            }
        }
    }
    if let (None, Some((m, word))) = (intent.mood, mood) {
        intent.mood = Some(m);
        intent.summary.push(word);
    }
    let narrowed = intent.tempo.is_some()
        || !intent.genres.is_empty()
        || intent.years.is_some()
        || !intent.like_artists.is_empty();
    if let (None, false, Some((level, word))) = (intent.energy, narrowed, energy) {
        intent.energy = Some(level);
        intent.summary.push(word);
    }
    (!intent.is_empty()).then_some(intent)
}

/// How much a genre counts when it comes from the song's artist (and not the
/// song or its album), from what the AI heard, or from a neighboring genre.
const ARTIST_GENRE: f32 = 0.75;
const HEARD_GENRE: f32 = 0.45;
const NEAR_GENRE: f32 = 0.25;
/// A song from a few years outside the years asked for.
const NEAR_YEAR: f32 = 0.5;
const NEAR_YEARS: i64 = 2;
/// A song in the wrong mood still counts this much.
const OTHER_MOOD: f32 = 0.8;
/// How much the AI's sense of the description and popularity move a song
/// up, at most (the rest of its score is how well it fits). A description
/// of only a feeling ("party bangers") leans on the sound more.
const SOUND_PART: f32 = 0.45;
const FEELING_SOUND_PART: f32 = 0.85;
const POPULAR_PART: f32 = 0.15;

/// Songs that fit, best first with artists spread out, and how many fit in
/// all. `heard` is how close each song sounds to the description (the AI's
/// match), when it's on.
pub fn rank(
    intent: &Intent,
    songs: &[SongFacts],
    sound: &SoundIndex,
    heard: Option<&std::collections::HashMap<i64, f32>>,
    limit: usize,
) -> (Vec<i64>, usize) {
    // Energy thresholds from this library's own spread of punch.
    let mut punches: Vec<f32> = songs.iter().filter_map(|s| s.punch).collect();
    punches.sort_by(f32::total_cmp);
    let percentile = |p: f32| -> f32 {
        punches
            .get(((punches.len() as f32 - 1.0) * p).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let punch = (percentile(0.4), percentile(0.6));
    let mut matches: Vec<(&SongFacts, f32)> = songs
        .iter()
        .map(|s| (s, fit(intent, s, punch)))
        .filter(|(_, f)| *f > 0.0)
        .collect();
    let total = matches.len();
    let artist_of: std::collections::HashMap<i64, i64> = songs
        .iter()
        .filter_map(|s| s.artist.map(|a| (s.id, a)))
        .collect();
    let artist = |id: i64| artist_of.get(&id).copied();

    // Sound like the named artists: their closest songs by the analyzer's
    // measurements and (in `heard`, when on) by ear, sharing their genres.
    if !intent.like_artists.is_empty() {
        let mut order: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for &artist in &intent.like_artists {
            for (rank, id) in sound
                .tracks_like_artist(artist, sound.len(), true)
                .into_iter()
                .enumerate()
            {
                let best = order.entry(id).or_insert(rank);
                *best = (*best).min(rank);
            }
        }
        let theirs = like_genres(songs, &intent.like_artists);
        let n = order.len().max(2) as f32 - 1.0;
        let by_ear = heard.map(|heard| {
            let mut ranked: Vec<(i64, f32)> = matches
                .iter()
                .filter_map(|(s, _)| heard.get(&s.id).map(|h| (s.id, *h)))
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            let m = ranked.len().max(2) as f32 - 1.0;
            ranked
                .into_iter()
                .enumerate()
                .map(|(i, (id, _))| (id, 1.0 - i as f32 / m))
                .collect::<std::collections::HashMap<i64, f32>>()
        });
        matches.retain(|(s, _)| order.contains_key(&s.id));
        let scored = matches
            .iter()
            .map(|(s, f)| {
                let measured = 1.0 - order[&s.id] as f32 / n;
                let close = match &by_ear {
                    Some(ear) => {
                        LIKE_EAR * ear.get(&s.id).copied().unwrap_or(0.5)
                            + (1.0 - LIKE_EAR) * measured
                    }
                    None => measured,
                };
                // Only the very closest count: square it twice.
                let close = close.powi(16);
                (s.id, f * close * theirs.share(&s.genres))
            })
            .collect();
        return (spread(scored, artist, limit), total.min(order.len()));
    }

    // Otherwise how well each fits, moved up by how it sounds (from the
    // middle of the songs that fit, 0, to the closest, 1) and by how often
    // it's been asked for and played.
    let sounds: Option<std::collections::HashMap<i64, f32>> = heard.map(|heard| {
        let mut values: Vec<f32> = matches
            .iter()
            .filter_map(|(s, _)| heard.get(&s.id).copied())
            .collect();
        values.sort_by(f32::total_cmp);
        let middle = values.get(values.len() / 2).copied().unwrap_or(0.0);
        let best = values.last().copied().unwrap_or(1.0);
        let range = (best - middle).max(1e-6);
        matches
            .iter()
            .filter_map(|(s, _)| {
                heard
                    .get(&s.id)
                    .map(|h| (s.id, ((h - middle) / range).clamp(0.0, 1.0)))
            })
            .collect()
    });
    let most_popular = matches
        .iter()
        .map(|(s, _)| s.popularity)
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let sound_part = if intent.genres.is_empty() && intent.years.is_none() {
        FEELING_SOUND_PART
    } else {
        SOUND_PART
    };
    let scored = matches
        .iter()
        .map(|(s, f)| {
            // Not heard yet: like a song in the middle.
            let sounds = sounds
                .as_ref()
                .map_or(0.5, |p| p.get(&s.id).copied().unwrap_or(0.0));
            let popular = (s.popularity.max(0) as f32).ln_1p() / most_popular.ln_1p();
            // A stable shuffle settles ties, so the list doesn't read as an
            // alphabetical dump.
            let tie = (scramble(s.id) % 1000) as f32 / 1_000_000.0;
            let score =
                f * (1.0 - sound_part + sound_part * sounds) * (1.0 + POPULAR_PART * popular) + tie;
            (s.id, score)
        })
        .collect();
    (spread(scored, artist, limit), total)
}

/// How much what the AI heard counts in sounding like an artist.
const LIKE_EAR: f32 = 0.7;

/// The genres of artists asked to sound like, each weighted by how rare it
/// is in the library (so sharing "glam metal" says more than "rock").
struct LikeGenres {
    weights: std::collections::HashMap<String, f32>,
    total: f32,
}

impl LikeGenres {
    /// 0.25 for a song sharing none of them, up to 1 for all.
    fn share(&self, genres: &[String]) -> f32 {
        if self.total <= 0.0 {
            return 1.0;
        }
        let shared: f32 = genres.iter().filter_map(|g| self.weights.get(g)).sum();
        0.25 + 0.75 * (shared / self.total).min(1.0)
    }
}

fn like_genres(songs: &[SongFacts], artists: &[i64]) -> LikeGenres {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut theirs: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut their_songs = 0;
    for song in songs {
        let mine = song.artist.is_some_and(|a| artists.contains(&a));
        their_songs += usize::from(mine);
        for g in &song.genres {
            *counts.entry(g.as_str()).or_default() += 1;
            if mine {
                *theirs.entry(g.as_str()).or_default() += 1;
            }
        }
    }
    let n = songs.len() as f32 + 1.0;
    // Genres on at least a quarter of their songs.
    let weights: std::collections::HashMap<String, f32> = theirs
        .into_iter()
        .filter(|(_, c)| *c * 4 >= their_songs)
        .map(|(g, _)| {
            let rarity = (n / (counts.get(g).copied().unwrap_or(0) as f32 + 1.0))
                .ln()
                .max(0.1);
            (g.to_owned(), rarity)
        })
        .collect();
    LikeGenres {
        total: weights.values().sum(),
        weights,
    }
}

/// How well `song` fits `intent`, 0 (not at all) to 1. `punch` is this
/// library's low and high punch.
fn fit(intent: &Intent, song: &SongFacts, punch: (f32, f32)) -> f32 {
    if let Some((min, max)) = intent.tempo {
        if !song.bpm.is_some_and(|b| (min..=max).contains(&b)) {
            return 0.0;
        }
    }
    let (low_punch, high_punch) = punch;
    match intent.energy {
        Some(Level::High)
            if !(song.punch.is_some_and(|p| p >= high_punch)
                || song.bpm.is_some_and(|b| b >= 120.0)) =>
        {
            return 0.0
        }
        Some(Level::Low)
            if !(song.punch.is_some_and(|p| p <= low_punch)
                && song.bpm.is_some_and(|b| b <= 112.0)) =>
        {
            return 0.0
        }
        _ => {}
    }
    let fits = |names: &[String], genres: &[String]| {
        names
            .iter()
            .any(|name| genres.iter().any(|g| genre_fits(g, name)))
    };
    if intent
        .not_genres
        .iter()
        .any(|group| fits(group, &song.genres) || fits(group, &song.own_genres))
    {
        return 0.0;
    }
    let mut score: f32 = 1.0;
    for group in &intent.genres {
        let this = if fits(group, &song.own_genres) {
            1.0
        } else if fits(group, &song.genres) {
            ARTIST_GENRE
        } else if song.genres.is_empty() && fits(group, &song.styles) {
            // The AI's guess only for songs with nothing better: it hears
            // "glam metal" in pop punk and "disco" in Queen.
            HEARD_GENRE
        } else if fits(&intent.near, &song.own_genres) || fits(&intent.near, &song.genres) {
            NEAR_GENRE
        } else {
            0.0
        };
        score = score.min(this);
    }
    if let Some((from, to)) = intent.years {
        score *= era_fit(from, to, song);
    } else if let Some((from, to)) = intent.genre_era {
        score *= era_fit(from, to, song).max(OUT_OF_ERA);
    }
    let mood_fits = match (intent.mood, song.mode) {
        (Some(Mood::Happy), Some(m)) => m > 0.0,
        (Some(Mood::Sad), Some(m)) => m < 0.0,
        _ => true,
    };
    if !mood_fits {
        score *= OTHER_MOOD;
    }
    score
}

/// How well a song's years fit `from..=to`: the year it belongs to, else
/// its own year, else a little for a year or two outside.
fn era_fit(from: i64, to: i64, song: &SongFacts) -> f32 {
    let inside = |y: i64| (from..=to).contains(&y);
    match (song.era, song.year) {
        (Some(era), _) if inside(era) => 1.0,
        (_, Some(year)) if inside(year) => 0.9,
        (Some(y), _) | (None, Some(y)) => {
            let off = if y < from { from - y } else { y - to };
            if off <= NEAR_YEARS {
                NEAR_YEAR
            } else {
                0.0
            }
        }
        (None, None) => 0.0,
    }
}

/// Places in a list arranged with artists spread out; the rest follow by
/// score.
const SPREAD_AHEAD: usize = 200;
/// Each further song by an artist already placed counts this much less.
const REPEAT_FACTOR: f32 = 0.6;
/// No artist comes back within this many songs if anyone else fits.
const ARTIST_GAP: usize = 2;

/// Song ids from `scored` (id, score), best first, but with artists spread
/// out: a further song by an artist already placed counts for less, and an
/// artist doesn't come back right away while others fit. `artist` says
/// whose a song is.
pub fn spread(
    mut scored: Vec<(i64, f32)>,
    artist: impl Fn(i64) -> Option<i64>,
    limit: usize,
) -> Vec<i64> {
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let arrange = limit.min(SPREAD_AHEAD);
    let window = (arrange * 8).min(scored.len());
    let rest = scored.split_off(window);
    let mut pool = scored;
    let mut out: Vec<i64> = Vec::with_capacity(arrange);
    let mut placed: std::collections::HashMap<i64, i32> = std::collections::HashMap::new();
    let mut recent: std::collections::VecDeque<i64> = std::collections::VecDeque::new();
    while out.len() < arrange && !pool.is_empty() {
        let mut best = (0, f32::MIN);
        for (i, (id, score)) in pool.iter().enumerate() {
            let mut weight = *score;
            if let Some(a) = artist(*id) {
                weight *= REPEAT_FACTOR.powi(placed.get(&a).copied().unwrap_or(0));
                if recent.contains(&a) {
                    weight *= 0.01;
                }
            }
            if weight > best.1 {
                best = (i, weight);
            }
        }
        let (id, _) = pool.remove(best.0);
        out.push(id);
        if let Some(a) = artist(id) {
            *placed.entry(a).or_default() += 1;
            recent.push_back(a);
            if recent.len() > ARTIST_GAP {
                recent.pop_front();
            }
        }
    }
    out.extend(pool.into_iter().chain(rest).map(|(id, _)| id));
    out.truncate(limit);
    out
}

/// Does genre `g` (from a song) count as `name` (from the query)? Exact,
/// or `name` as a whole word inside it ("deep house" counts as house), but
/// not a genre that came after or before it ("post grunge" isn't grunge).
pub fn genre_fits(g: &str, name: &str) -> bool {
    if g == name {
        return true;
    }
    let other_era = ["post ", "proto "]
        .iter()
        .any(|p| g.starts_with(p) && !name.starts_with(p));
    !other_era
        && (g.starts_with(&format!("{name} "))
            || g.ends_with(&format!(" {name}"))
            || g.contains(&format!(" {name} ")))
}

/// A cheap, stable hash for ordering.
fn scramble(id: i64) -> u64 {
    (id as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocabulary {
        Vocabulary::new(
            ["future rave", "synth pop", "deep house", "glam metal"],
            [(1, "Zara Larsson"), (2, "KREAM"), (3, "Zara")],
        )
    }

    #[test]
    fn understands_decades_and_genres() {
        let v = vocab();
        let i = parse("80's Hair Metal", &v).unwrap();
        assert_eq!(i.years, Some((1980, 1989)));
        assert_eq!(i.genres.len(), 1);
        assert!(i.genres[0].contains(&"glam metal".to_owned()));
        assert_eq!(i.summary, ["1980s", "hair metal"]);

        assert_eq!(
            parse("early 90s rock", &v).unwrap().years,
            Some((1990, 1993))
        );
        assert_eq!(
            parse("songs from the eighties", &v).unwrap().years,
            Some((1980, 1989))
        );
        assert_eq!(parse("2000s", &v).unwrap().years, Some((2000, 2009)));
        assert_eq!(parse("1995-1999", &v).unwrap().years, Some((1995, 1999)));
        assert_eq!(parse("since 2015", &v).unwrap().years, Some((2015, 2100)));
        // Library genres work as written.
        assert_eq!(
            parse("future rave", &v).unwrap().genres,
            [vec!["future rave".to_owned()]]
        );
    }

    #[test]
    fn understands_tempo_mood_and_artists() {
        let v = vocab();
        let i = parse("upbeat songs like Zara Larsson, nothing too slow", &v).unwrap();
        assert_eq!(i.energy, Some(Level::High));
        assert_eq!(i.like_artists, [1], "the longest name wins");
        assert_eq!(i.tempo, Some((95.0, 250.0)));
        assert_eq!(parse("chill sad house", &v).unwrap().mood, Some(Mood::Sad));
        assert_eq!(parse("chill house", &v).unwrap().energy, Some(Level::Low));
    }

    #[test]
    fn plain_searches_are_not_descriptions() {
        let v = vocab();
        assert!(parse("Lush Life", &v).is_none());
        assert!(parse("Never Forget You", &v).is_none());
        // Theory has "the" in it but isn't a decade or genre.
        assert!(parse("Theory of a Deadman", &v).is_none());
    }

    fn song(id: i64, year: i64, genres: &[&str], popularity: i64, bpm: f32) -> SongFacts {
        let genres: Vec<String> = genres.iter().map(|g| (*g).to_owned()).collect();
        SongFacts {
            id,
            year: Some(year),
            own_genres: genres.clone(),
            genres,
            popularity,
            bpm: Some(bpm),
            punch: Some(0.3),
            mode: Some(0.1),
            artist: Some(id),
            ..SongFacts::default()
        }
    }

    fn ranked(query: &str, songs: &[SongFacts]) -> (Vec<i64>, usize) {
        let intent = parse(query, &vocab()).unwrap();
        rank(&intent, songs, &SoundIndex::default(), None, 10)
    }

    #[test]
    fn ranks_songs_that_fit() {
        let songs = vec![
            song(1, 1987, &["glam metal", "hard rock"], 2, 120.0),
            song(2, 1984, &["heavy metal"], 9, 140.0),
            song(3, 1995, &["glam metal"], 50, 120.0),
            song(4, 1986, &["synth pop"], 50, 118.0),
        ];
        let (ids, total) = ranked("80s hair metal", &songs);
        assert_eq!(total, 2);
        assert_eq!(
            ids,
            [1, 2],
            "glam metal first, heavy metal only near; 1995 and synth pop don't fit"
        );
        assert!(genre_fits("deep house", "house"));
        assert!(!genre_fits("housewife", "house"));
        assert!(!genre_fits("post grunge", "grunge"), "a genre of its own");
        assert!(genre_fits("post grunge", "post grunge"));
        assert!(genre_fits("post punk revival", "post punk"));

        let no_glam = parse("80s metal, no glam", &vocab()).unwrap();
        assert_eq!(no_glam.summary, ["1980s", "metal", "no glam"]);
        let (ids, _) = rank(&no_glam, &songs, &SoundIndex::default(), None, 10);
        assert_eq!(ids, [2], "the glam metal song is ruled out");
    }

    #[test]
    fn the_songs_own_genres_count_most() {
        let mut by_artist = song(1, 1986, &["rock"], 50, 120.0);
        by_artist.genres.push("glam metal".into());
        let mut heard = song(2, 1986, &[], 50, 120.0);
        heard.styles = vec!["glam metal".into()];
        let own = song(3, 1986, &["glam metal"], 0, 120.0);
        // The AI's guess doesn't count against a song's own genres.
        let mut tagged = song(4, 1986, &["pop punk"], 90, 120.0);
        tagged.styles = vec!["glam metal".into()];
        let (ids, total) = ranked("80s glam metal", &[heard, by_artist, own, tagged]);
        assert_eq!(total, 3);
        assert_eq!(
            ids,
            [3, 1, 2],
            "its own tag, then its artist's, then the AI's"
        );
    }

    #[test]
    fn a_greatest_hits_goes_by_when_the_songs_were_made() {
        let mut hits = song(1, 1998, &["glam metal"], 0, 120.0);
        hits.era = Some(1987);
        let late = song(2, 1998, &["glam metal"], 0, 120.0);
        let close = song(3, 1991, &["glam metal"], 0, 120.0);
        let (ids, total) = ranked("80s hair metal", &[hits, late, close]);
        assert_eq!(total, 2);
        assert_eq!(ids, [1, 3], "a year or two out still fits a little");
    }

    #[test]
    fn artists_are_spread_out() {
        let mut songs: Vec<SongFacts> = (1..=4)
            .map(|id| {
                let mut s = song(id, 1986, &["glam metal"], 100, 120.0);
                s.artist = Some(100);
                s
            })
            .collect();
        songs.extend((5..=10).map(|id| song(id, 1986, &["glam metal"], 0, 120.0)));
        let (ids, _) = ranked("80s glam metal", &songs);
        assert_eq!(ids.len(), 10);
        assert_eq!(ids[0], 4, "the band's most popular song still leads");
        let mut next: Vec<i64> = ids[1..7].to_vec();
        next.sort_unstable();
        assert_eq!(
            next,
            [5, 6, 7, 8, 9, 10],
            "everyone else comes before the band's second song: {ids:?}"
        );
    }

    #[test]
    fn classic_rock_means_older_songs() {
        let old = song(1, 1975, &["classic rock"], 0, 120.0);
        let new = song(2, 2023, &["classic rock"], 90, 120.0);
        let (ids, total) = ranked("classic rock", &[new, old]);
        assert_eq!(total, 2, "a new song still fits a little");
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn spreading_keeps_every_song_once() {
        let scored: Vec<(i64, f32)> = (0..1000).map(|id| (id, (id % 17) as f32)).collect();
        let mut ids = spread(scored, |id| Some(id % 3), usize::MAX);
        assert_eq!(ids.len(), 1000);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn merges_what_a_model_read() {
        let v = vocab();
        // Nothing the phrase reader knows: the model's reading, checked.
        let request = "first dance at a wedding, no rap";
        let asked = Asked {
            genres: vec!["Synth Pop".into(), " sad".into(), "sappy ballads".into()],
            not_genres: vec!["rap".into(), "metal".into()],
            tempo: Some("slow".into()),
            like_artists: vec!["Zara Larsson".into()],
            ..Asked::default()
        };
        let i = merge(request, parse(request, &v), &asked, &v).unwrap();
        assert_eq!(i.genres, [vec!["synth pop".to_owned()]]);
        assert_eq!(i.mood, Some(Mood::Sad), "\"sad\" is a mood, not a genre");
        assert_eq!(i.not_genres.len(), 1, "only rap was ruled out by name");
        assert!(i.like_artists.is_empty(), "nobody named Zara Larsson");
        assert_eq!(i.energy, None, "genres narrow it already");
        assert!(i.unknown.is_empty());

        // The phrase reader's parts stay; the model fills the gaps.
        let request = "80s songs my dad likes";
        let asked = Asked {
            genres: vec!["80's glam metal".into()],
            years: Some((1970, 1979)),
            ..Asked::default()
        };
        let i = merge(request, parse(request, &v), &asked, &v).unwrap();
        assert_eq!(i.years, Some((1980, 1989)));
        assert!(i.genres[0].contains(&"glam metal".to_owned()));
        assert_eq!(i.summary, ["1980s", "glam metal"]);

        // "house" here is a place; the model gave no genres.
        let request = "chill music to clean the house to";
        let asked = Asked {
            mood: Some("calm".into()),
            ..Asked::default()
        };
        let i = merge(request, parse(request, &v), &asked, &v).unwrap();
        assert!(i.genres.is_empty());
        assert_eq!(i.summary, ["chill"]);

        // Only a feeling: the tempo guess is all there is.
        let asked = Asked {
            mood: Some("calm".into()),
            ..Asked::default()
        };
        let i = merge("music for cooking dinner", None, &asked, &v).unwrap();
        assert_eq!(i.energy, Some(Level::Low));
        assert_eq!(merge("hmm", None, &Asked::default(), &v), None);
    }
}
