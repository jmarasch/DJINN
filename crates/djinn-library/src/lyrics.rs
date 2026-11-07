//! Song lyrics: read from a `.lrc` file beside the song or from its tags,
//! or looked up on LRCLIB (lrclib.net, free and open, no key). Timed lyrics
//! are LRC text: lines like `[01:02.50]words`, which [`parse`] turns into
//! [`Line`]s. The music library is only read, never written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use lofty::file::TaggedFileExt;
use lofty::tag::ItemKey;
use serde::{Deserialize, Serialize};

/// One timed line: when it starts, in milliseconds from the song's start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Line {
    pub ms: u64,
    pub text: String,
}

/// Where lyrics were found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A `.lrc` file beside the song.
    Lrc,
    /// The song's own tags.
    File,
    /// LRCLIB.
    Lrclib,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Lrc => "lrc",
            Source::File => "file",
            Source::Lrclib => "lrclib",
        }
    }
}

/// Lyrics as found: timed (LRC text), plain, or both. An instrumental has
/// neither and says so.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub instrumental: bool,
    pub source: Source,
}

impl Found {
    /// Sorts text into timed or plain. Blank text is nothing.
    fn from_text(text: &str, source: Source) -> Option<Found> {
        let text = text.trim_start_matches('\u{feff}').trim();
        if text.is_empty() {
            return None;
        }
        Some(if is_timed(text) {
            Found {
                plain: Some(plain_of(text)),
                synced: Some(text.to_owned()),
                instrumental: false,
                source,
            }
        } else {
            Found {
                synced: None,
                plain: Some(text.to_owned()),
                instrumental: false,
                source,
            }
        })
    }

    /// The text that's searched: the plain words.
    pub fn search_text(&self) -> Option<&str> {
        self.plain.as_deref().filter(|p| !p.trim().is_empty())
    }
}

/// `[mm:ss]`, `[mm:ss.x]`, `[mm:ss.xx]` or `[mm:ss.xxx]` (also with `:` before
/// the fraction), as milliseconds.
fn timestamp(tag: &str) -> Option<u64> {
    let (minutes, rest) = tag.split_once(':')?;
    let minutes: u64 = minutes.trim().parse().ok()?;
    let (seconds, fraction) = match rest.find(['.', ':']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let seconds: u64 = seconds.trim().parse().ok()?;
    if seconds >= 60 || !fraction.chars().all(|c| c.is_ascii_digit()) || fraction.len() > 3 {
        return None;
    }
    let fraction_ms = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction.parse::<u64>().ok()?,
    };
    Some(minutes * 60_000 + seconds * 1000 + fraction_ms)
}

/// Splits a line's leading `[...]` tags from its words.
fn leading_tags(line: &str) -> (Vec<&str>, &str) {
    let mut tags = Vec::new();
    let mut rest = line.trim_start();
    while let Some(after) = rest.strip_prefix('[') {
        let Some(end) = after.find(']') else { break };
        tags.push(&after[..end]);
        rest = after[end + 1..].trim_start();
    }
    (tags, rest)
}

/// True if the text is LRC: some line starts with a timestamp.
pub fn is_timed(text: &str) -> bool {
    text.lines()
        .any(|line| leading_tags(line).0.iter().any(|t| timestamp(t).is_some()))
}

/// Timed lines in order. A line with several timestamps (a repeated chorus)
/// appears at each; `[offset:+250]` moves every line earlier by 250 ms, as
/// the format says; other tags (`[ar:...]`) are skipped, and so are words
/// inside `<...>` timings of enhanced LRC.
pub fn parse(text: &str) -> Vec<Line> {
    let mut offset: i64 = 0;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let (tags, words) = leading_tags(raw);
        let mut times = Vec::new();
        for tag in tags {
            if let Some(ms) = timestamp(tag) {
                times.push(ms);
            } else if let Some(value) = tag.strip_prefix("offset:") {
                offset = value.trim().parse().unwrap_or(0);
            }
        }
        if times.is_empty() {
            continue;
        }
        let words = strip_word_timings(words);
        for ms in times {
            lines.push(Line {
                ms,
                text: words.clone(),
            });
        }
    }
    for line in &mut lines {
        line.ms = (line.ms as i64 - offset).max(0) as u64;
    }
    lines.sort_by_key(|l| l.ms);
    lines
}

/// Enhanced LRC times each word as `<01:02.50>`; only the words are kept.
fn strip_word_timings(words: &str) -> String {
    let mut out = String::with_capacity(words.len());
    let mut rest = words;
    while let Some(start) = rest.find('<') {
        match rest[start..].find('>') {
            Some(end) if timestamp(&rest[start + 1..start + end]).is_some() => {
                out.push_str(&rest[..start]);
                rest = &rest[start + end + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of LRC text, one line each, without times or tags; each
/// repeated line once, where it first comes.
pub fn plain_of(text: &str) -> String {
    let mut out = Vec::new();
    for raw in text.lines() {
        let (tags, words) = leading_tags(raw);
        let timed = tags.iter().any(|t| timestamp(t).is_some());
        if !timed && !tags.is_empty() {
            continue;
        }
        out.push(strip_word_timings(words));
    }
    out.join("\n").trim().to_owned()
}

/// The `.lrc` file beside a song: same name, `.lrc` or `.LRC`.
pub fn sidecar(song: &Path) -> Option<PathBuf> {
    ["lrc", "LRC"]
        .into_iter()
        .map(|ext| song.with_extension(ext))
        .find(|p| p.is_file())
}

/// Lyrics on disk: the `.lrc` beside the song first, then its tags (timed
/// ones before plain). Reads files only.
pub fn local(song: &Path) -> Option<Found> {
    if let Some(found) = sidecar(song)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| Found::from_text(&String::from_utf8_lossy(&bytes), Source::Lrc))
    {
        return Some(found);
    }
    let file = std::panic::catch_unwind(|| lofty::read_from_path(song))
        .ok()?
        .ok()?;
    let mut texts: Vec<String> = Vec::new();
    for tag in file.tags() {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            if let Some(text) = tag.get_string(key) {
                texts.push(text.to_owned());
            }
        }
    }
    // Timed text wins over plain when a file has both.
    texts.sort_by_key(|t| !is_timed(t));
    texts.iter().find_map(|t| Found::from_text(t, Source::File))
}

/// What an export file holds: the timed LRC when there is one, otherwise
/// the plain words.
pub fn export_text(synced: Option<&str>, plain: Option<&str>) -> Option<String> {
    synced
        .or(plain)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("{t}\n"))
}

/// What to ask LRCLIB for.
pub struct Query<'a> {
    pub artist: &'a str,
    pub title: &'a str,
    pub album: Option<&'a str>,
    pub duration_ms: Option<i64>,
}

/// A lookup that failed (network, or LRCLIB having trouble), as opposed to
/// LRCLIB not knowing the song.
#[derive(Debug, thiserror::Error)]
#[error("LRCLIB lookup failed: {0}")]
pub struct LookupError(String);

#[derive(Deserialize)]
struct LrclibSong {
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

/// Talks to LRCLIB. Ask it one song at a time; it's a free service.
pub struct Lrclib {
    client: reqwest::Client,
}

impl Lrclib {
    pub fn new() -> Result<Lrclib, LookupError> {
        // reqwest is built without a default TLS provider; use ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "DJINN/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/jmarasch/DJINN)"
            ))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| LookupError(e.to_string()))?;
        Ok(Lrclib { client })
    }

    /// The song's lyrics, `None` if LRCLIB doesn't have them. Matching uses
    /// artist, title, album and length (within a couple of seconds), so a
    /// different version of a song isn't timed wrong.
    pub async fn get(&self, query: &Query<'_>) -> Result<Option<Found>, LookupError> {
        let mut params = vec![
            ("artist_name", query.artist.to_owned()),
            ("track_name", query.title.to_owned()),
        ];
        if let Some(album) = query.album.filter(|a| !a.is_empty()) {
            params.push(("album_name", album.to_owned()));
        }
        if let Some(ms) = query.duration_ms.filter(|ms| *ms > 0) {
            params.push(("duration", ((ms + 500) / 1000).to_string()));
        }
        let response = self
            .client
            .get("https://lrclib.net/api/get")
            .query(&params)
            .send()
            .await
            .map_err(|e| LookupError(e.to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(LookupError(format!("status {status}")));
        }
        let song: LrclibSong = response
            .json()
            .await
            .map_err(|e| LookupError(e.to_string()))?;
        if song.instrumental {
            return Ok(Some(Found {
                synced: None,
                plain: None,
                instrumental: true,
                source: Source::Lrclib,
            }));
        }
        let synced = song.synced.filter(|s| is_timed(s));
        let plain = song
            .plain
            .filter(|p| !p.trim().is_empty())
            .or_else(|| synced.as_deref().map(plain_of));
        if synced.is_none() && plain.is_none() {
            return Ok(None);
        }
        Ok(Some(Found {
            synced,
            plain,
            instrumental: false,
            source: Source::Lrclib,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_take_every_common_form() {
        assert_eq!(timestamp("01:02"), Some(62_000));
        assert_eq!(timestamp("01:02.5"), Some(62_500));
        assert_eq!(timestamp("01:02.50"), Some(62_500));
        assert_eq!(timestamp("01:02.505"), Some(62_505));
        assert_eq!(timestamp("01:02:50"), Some(62_500));
        assert_eq!(timestamp("12:00.00"), Some(720_000));
        assert_eq!(timestamp("ar:Queen"), None);
        assert_eq!(timestamp("01:75"), None);
    }

    #[test]
    fn lines_come_in_order_with_repeats_and_offset() {
        let lrc = "[ar:Band]\n[ti:Song]\n[offset:+500]\n\
                   [00:10.00]First line\n\
                   [00:20.00][00:40.00]Chorus\n\
                   [00:30.00]  Second   verse \n\
                   no time here\n";
        let lines = parse(lrc);
        let got: Vec<(u64, &str)> = lines.iter().map(|l| (l.ms, l.text.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (9_500, "First line"),
                (19_500, "Chorus"),
                (29_500, "Second verse"),
                (39_500, "Chorus"),
            ]
        );
    }

    #[test]
    fn a_negative_offset_moves_lines_later_and_none_go_below_zero() {
        let lines = parse("[offset:-1000]\n[00:00.50]Hi\n");
        assert_eq!(lines[0].ms, 1_500);
        let lines = parse("[offset:2000]\n[00:01.00]Hi\n");
        assert_eq!(lines[0].ms, 0);
    }

    #[test]
    fn enhanced_word_timings_are_dropped() {
        let lines = parse("[00:01.00]<00:01.00>Hello <00:01.50>there <b>bold</b>\n");
        assert_eq!(lines[0].text, "Hello there <b>bold</b>");
    }

    #[test]
    fn plain_text_is_the_words_without_tags() {
        let lrc = "[ar:Band]\n[00:01.00]One\n[00:02.00][00:05.00]Two\n\n[00:03.00]Three";
        assert_eq!(plain_of(lrc), "One\nTwo\n\nThree");
        assert!(is_timed(lrc));
        assert!(!is_timed("Just words\nNo times [here]"));
    }

    #[test]
    fn text_sorts_into_timed_or_plain() {
        let timed = Found::from_text("\u{feff}[00:01.00]Hi\n", Source::Lrc).unwrap();
        assert_eq!(timed.synced.as_deref(), Some("[00:01.00]Hi"));
        assert_eq!(timed.plain.as_deref(), Some("Hi"));
        let plain = Found::from_text("Hi\nthere", Source::File).unwrap();
        assert_eq!(
            (plain.synced, plain.plain.as_deref()),
            (None, Some("Hi\nthere"))
        );
        assert!(Found::from_text("  \n ", Source::File).is_none());
    }

    #[test]
    fn a_sidecar_lrc_is_found_beside_the_song() {
        let dir = std::env::temp_dir().join(format!("djinn-lyrics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("01 Song.flac");
        std::fs::write(&song, b"not really audio").unwrap();
        assert!(local(&song).is_none());
        std::fs::write(dir.join("01 Song.lrc"), "[00:01.00]Hello\n").unwrap();
        let found = local(&song).unwrap();
        assert_eq!(found.source, Source::Lrc);
        assert_eq!(found.plain.as_deref(), Some("Hello"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn export_prefers_timed_text() {
        assert_eq!(
            export_text(Some("[00:01.00]Hi"), Some("Hi")).as_deref(),
            Some("[00:01.00]Hi\n")
        );
        assert_eq!(export_text(None, Some("Hi ")).as_deref(), Some("Hi\n"));
        assert_eq!(export_text(None, Some("  ")), None);
    }
}
