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
    let body: String = text
        .lines()
        .filter(|l| !is_header_line(l))
        .collect::<Vec<_>>()
        .join("\n");
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
