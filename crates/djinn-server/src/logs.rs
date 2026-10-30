//! The latest log lines, kept in memory for Admin > System whether or not
//! DJINN writes a log file, and where that file is.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Lines kept in memory.
const KEPT: usize = 3000;

static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: OnceLock<PathBuf> = OnceLock::new();

/// A log writer that keeps each line in memory. Each event arrives in one
/// write, already formatted.
pub struct Recent;

impl Write for Recent {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        if let Ok(mut lines) = RECENT.lock() {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                if lines.len() == KEPT {
                    lines.pop_front();
                }
                lines.push_back(line.to_owned());
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn set_file(path: &Path) {
    let _ = FILE.set(path.to_path_buf());
}

/// The log file, when DJINN writes one.
pub fn file() -> Option<&'static Path> {
    FILE.get().map(PathBuf::as_path)
}

/// Log levels, least to most serious, as they're written.
const LEVELS: [&str; 5] = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"];

/// A line's level: the word after the time. Lines that carry on a message
/// (no time of their own) have none.
fn level_of(line: &str) -> Option<usize> {
    let word = line.split_whitespace().nth(1)?;
    LEVELS.iter().position(|l| *l == word)
}

/// The latest lines, oldest first: at least as serious as `min` (one of
/// [`LEVELS`], any case) and containing `search` (any case), at most
/// `limit` of them. A line that carries on a message goes with it.
pub fn recent(min: Option<&str>, search: Option<&str>, limit: usize) -> Vec<String> {
    let min = min
        .and_then(|m| LEVELS.iter().position(|l| l.eq_ignore_ascii_case(m)))
        .unwrap_or(0);
    let search = search.map(str::to_lowercase).filter(|s| !s.is_empty());
    let Ok(lines) = RECENT.lock() else {
        return Vec::new();
    };
    let mut level = 0;
    let mut found: Vec<String> = Vec::new();
    for line in lines.iter() {
        level = level_of(line).unwrap_or(level);
        let matches = search
            .as_deref()
            .is_none_or(|s| line.to_lowercase().contains(s));
        if level >= min && matches {
            found.push(line.clone());
        }
    }
    let skip = found.len().saturating_sub(limit);
    found.split_off(skip)
}

/// Everything kept in memory, for a download when there's no log file.
pub fn all() -> String {
    RECENT
        .lock()
        .map(|lines| lines.iter().map(|l| format!("{l}\n")).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_filter_by_level_and_words() {
        let mut w = Recent;
        w.write_all(b"2026-10-30T00:00:01Z  INFO djinn: DJINN listening\n")
            .unwrap();
        w.write_all(b"2026-10-30T00:00:02Z  WARN djinn_cast: cast lost\n  caused by: timeout\n")
            .unwrap();
        w.write_all(b"2026-10-30T00:00:03Z ERROR djinn: track failed\n")
            .unwrap();

        // The buffer is shared with every test that logs; look at ours.
        let ours = |lines: Vec<String>| -> Vec<String> {
            lines
                .into_iter()
                .filter(|l| l.contains("2026-10-30T00:00:0") || l.contains("caused by"))
                .collect()
        };
        assert_eq!(
            ours(recent(Some("warn"), None, 100)),
            [
                "2026-10-30T00:00:02Z  WARN djinn_cast: cast lost",
                "  caused by: timeout",
                "2026-10-30T00:00:03Z ERROR djinn: track failed",
            ]
        );
        assert_eq!(ours(recent(None, Some("LISTENING"), 100)).len(), 1);
        assert_eq!(recent(Some("error"), Some("track failed"), 1).len(), 1);
    }
}
