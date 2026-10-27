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
    /// Normalized genre names.
    pub genres: Vec<String>,
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
    (
        "hair metal",
        &[
            "glam metal",
            "hair metal",
            "glam rock",
            "heavy metal",
            "hard rock",
        ],
    ),
    ("glam metal", &["glam metal", "hair metal", "glam rock"]),
    ("glam", &["glam metal", "glam rock", "glam punk"]),
    ("metal", &["metal"]),
    (
        "classic rock",
        &[
            "classic rock",
            "hard rock",
            "rock",
            "blues rock",
            "album rock",
        ],
    ),
    ("rock", &["rock"]),
    ("punk", &["punk"]),
    ("emo", &["emo"]),
    ("grunge", &["grunge"]),
    (
        "goth",
        &["gothic rock", "goth", "darkwave", "deathrock", "industrial"],
    ),
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
    (
        "synthwave",
        &[
            "synthwave",
            "synth pop",
            "new wave",
            "electropop",
            "retrowave",
        ],
    ),
    (
        "synth",
        &["synth pop", "synthwave", "electropop", "new wave"],
    ),
    ("new wave", &["new wave", "synth pop", "post punk"]),
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
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    if !group.is_empty() || asked.genres.is_empty() {
        // The phrase reader's genres were words out of context.
        let dropped: Vec<String> = intent.genres.drain(..).flatten().collect();
        intent.summary.retain(|label| {
            genre_group(label, vocab).is_none_or(|g| g.iter().all(|n| !dropped.contains(n)))
        });
    }
    if !group.is_empty() {
        intent.genres.push(group);
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

/// Songs that fit, best first, and how many matched in all.
pub fn rank(
    intent: &Intent,
    songs: &[SongFacts],
    sound: &SoundIndex,
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
    let (low_punch, high_punch) = (percentile(0.4), percentile(0.6));

    let fits = |song: &SongFacts| -> bool {
        if let Some((from, to)) = intent.years {
            if !song.year.is_some_and(|y| (from..=to).contains(&y)) {
                return false;
            }
        }
        if let Some((min, max)) = intent.tempo {
            if !song.bpm.is_some_and(|b| (min..=max).contains(&b)) {
                return false;
            }
        }
        match intent.energy {
            Some(Level::High)
                if !(song.punch.is_some_and(|p| p >= high_punch)
                    || song.bpm.is_some_and(|b| b >= 120.0)) =>
            {
                return false
            }
            Some(Level::Low)
                if !(song.punch.is_some_and(|p| p <= low_punch)
                    && song.bpm.is_some_and(|b| b <= 112.0)) =>
            {
                return false
            }
            _ => {}
        }
        let in_group = |group: &Vec<String>| {
            group
                .iter()
                .any(|name| song.genres.iter().any(|g| genre_fits(g, name)))
        };
        intent.genres.iter().all(in_group) && !intent.not_genres.iter().any(in_group)
    };
    let mut matches: Vec<&SongFacts> = songs.iter().filter(|s| fits(s)).collect();
    let total = matches.len();

    // Sound like the named artists: their closest songs first.
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
        matches.retain(|s| order.contains_key(&s.id));
        matches.sort_by_key(|s| order.get(&s.id).copied().unwrap_or(usize::MAX));
        return (
            matches.iter().take(limit).map(|s| s.id).collect(),
            total.min(order.len()),
        );
    }

    // Otherwise: mood fit, then popularity, then a stable shuffle so the
    // list doesn't read as an alphabetical dump.
    let mood_score = |s: &SongFacts| -> i32 {
        match (intent.mood, s.mode) {
            (Some(Mood::Happy), Some(m)) if m > 0.0 => 1,
            (Some(Mood::Sad), Some(m)) if m < 0.0 => 1,
            _ => 0,
        }
    };
    matches.sort_by(|a, b| {
        mood_score(b)
            .cmp(&mood_score(a))
            .then(b.popularity.cmp(&a.popularity))
            .then(scramble(a.id).cmp(&scramble(b.id)))
    });
    (matches.iter().take(limit).map(|s| s.id).collect(), total)
}

/// Does genre `g` (from a song) count as `name` (from the query)? Exact,
/// or `name` as a whole word inside it ("deep house" counts as house).
fn genre_fits(g: &str, name: &str) -> bool {
    g == name
        || g.starts_with(&format!("{name} "))
        || g.ends_with(&format!(" {name}"))
        || g.contains(&format!(" {name} "))
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

    #[test]
    fn ranks_songs_that_fit() {
        let song = |id, year, genres: &[&str], popularity, bpm: f32| SongFacts {
            id,
            year: Some(year),
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            popularity,
            bpm: Some(bpm),
            punch: Some(0.3),
            mode: Some(0.1),
        };
        let songs = vec![
            song(1, 1987, &["glam metal", "hard rock"], 2, 120.0),
            song(2, 1984, &["heavy metal"], 9, 140.0),
            song(3, 1995, &["glam metal"], 50, 120.0),
            song(4, 1986, &["synth pop"], 50, 118.0),
        ];
        let intent = parse("80s hair metal", &vocab()).unwrap();
        let (ids, total) = rank(&intent, &songs, &SoundIndex::default(), 10);
        assert_eq!(total, 2);
        assert_eq!(ids, [2, 1], "popular first; 1995 and synth pop don't fit");
        assert!(genre_fits("deep house", "house"));
        assert!(!genre_fits("housewife", "house"));

        let no_glam = parse("80s metal, no glam", &vocab()).unwrap();
        assert_eq!(no_glam.summary, ["1980s", "metal", "no glam"]);
        let (ids, _) = rank(&no_glam, &songs, &SoundIndex::default(), 10);
        assert_eq!(ids, [2], "the glam metal song is ruled out");
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
