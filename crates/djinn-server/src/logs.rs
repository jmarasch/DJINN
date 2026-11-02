//! The latest log lines, kept in memory for Admin > System whether or not
//! DJINN writes a log file, and where that file is.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use chrono::{Duration as Days, Local, NaiveDate};
use djinn_store::Store;
use tracing::{Level, Metadata};

pub const LEVEL_SETTING: &str = "log_level";
pub const DAYS_SETTING: &str = "log_keep_days";
pub const DEFAULT_DAYS: u16 = 7;
pub const MAX_DAYS: u16 = 365;

// ERROR=1, WARN=2, TRACE=5. The default records warnings and errors.
static RECORD_LEVEL: AtomicU8 = AtomicU8::new(2);

/// Lines kept in memory.
const KEPT: usize = 3000;

static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: OnceLock<Mutex<DailyWriter>> = OnceLock::new();

pub fn valid_level(level: &str) -> bool {
    matches!(level, "trace" | "warn" | "error")
}

pub fn record_level() -> &'static str {
    match RECORD_LEVEL.load(Ordering::Relaxed) {
        5 => "trace",
        1 => "error",
        _ => "warn",
    }
}

pub fn set_record_level(level: &str) {
    let value = match level {
        "trace" => 5,
        "error" => 1,
        _ => 2,
    };
    RECORD_LEVEL.store(value, Ordering::Relaxed);
}

pub async fn apply(store: &Store) -> anyhow::Result<()> {
    let level = store.get_setting(LEVEL_SETTING).await?.unwrap_or_default();
    set_record_level(&level);
    let days = store
        .get_setting(DAYS_SETTING)
        .await?
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_DAYS)
        .clamp(1, MAX_DAYS);
    set_keep_days(days)?;
    Ok(())
}

/// Filter used by every output layer, so the setting affects the file,
/// console, and the in-memory viewer together.
pub fn enabled(meta: &Metadata<'_>) -> bool {
    let severity = match *meta.level() {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    };
    severity <= RECORD_LEVEL.load(Ordering::Relaxed)
}

struct DailyWriter {
    base: PathBuf,
    day: NaiveDate,
    file: File,
    keep_days: u16,
}

impl DailyWriter {
    fn new(base: PathBuf, keep_days: u16) -> io::Result<Self> {
        let day = Local::now().date_naive();
        if let Some(parent) = base.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(daily_path(&base, day))?;
        Ok(Self {
            base,
            day,
            file,
            keep_days,
        })
    }

    fn path(&self) -> PathBuf {
        daily_path(&self.base, self.day)
    }

    fn write_on(&mut self, day: NaiveDate, buf: &[u8]) -> io::Result<usize> {
        if day != self.day {
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(daily_path(&self.base, day))?;
            self.day = day;
            self.prune()?;
        }
        self.file.write(buf)
    }

    fn prune(&self) -> io::Result<()> {
        let parent = self
            .base
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let cutoff = self.day - Days::days(i64::from(self.keep_days.saturating_sub(1)));
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type()?.is_file() {
                continue;
            }
            if path == self.base || path.file_name() == self.base.file_name() {
                // The pre-rotation log is kept until its last write ages out.
                let old = entry
                    .metadata()?
                    .modified()
                    .ok()
                    .map(|t| chrono::DateTime::<Local>::from(t).date_naive());
                if old.is_some_and(|date| date < cutoff) {
                    std::fs::remove_file(path)?;
                }
            } else if dated_log_day(&self.base, &path).is_some_and(|date| date < cutoff) {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

impl Write for DailyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_on(Local::now().date_naive(), buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn daily_path(base: &Path, day: NaiveDate) -> PathBuf {
    let stem = base.file_stem().unwrap_or_default().to_string_lossy();
    let ext = base
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    base.with_file_name(format!("{stem}-{day}{ext}"))
}

fn dated_log_day(base: &Path, path: &Path) -> Option<NaiveDate> {
    let stem = base.file_stem()?.to_str()?;
    let ext = base
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let name = path.file_name()?.to_str()?;
    let date = name.strip_prefix(&format!("{stem}-"))?.strip_suffix(&ext)?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

pub fn open_file(base: &Path) -> io::Result<()> {
    let writer = DailyWriter::new(base.to_path_buf(), DEFAULT_DAYS)?;
    let _ = FILE.set(Mutex::new(writer));
    Ok(())
}

pub fn keep_days() -> u16 {
    FILE.get()
        .and_then(|w| w.lock().ok().map(|w| w.keep_days))
        .unwrap_or(DEFAULT_DAYS)
}

pub fn set_keep_days(days: u16) -> io::Result<()> {
    if let Some(writer) = FILE.get() {
        let mut writer = writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        writer.keep_days = days;
        writer.prune()?;
    }
    Ok(())
}

/// Ensures a new day's file exists even when the chosen level has had no
/// events, and expires old files on a quiet server.
pub fn rotate_today() -> io::Result<()> {
    if let Some(writer) = FILE.get() {
        let mut writer = writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        writer.write_on(Local::now().date_naive(), b"")?;
        writer.prune()?;
    }
    Ok(())
}

pub struct FileWriter(Option<MutexGuard<'static, DailyWriter>>);

pub fn writer() -> FileWriter {
    FileWriter(
        FILE.get()
            .map(|w| w.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
    )
}

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.0.as_mut() {
            Some(w) => w.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.0.as_mut() {
            Some(w) => w.flush(),
            None => Ok(()),
        }
    }
}

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

/// The log file, when DJINN writes one.
pub fn file() -> Option<PathBuf> {
    FILE.get().and_then(|w| w.lock().ok().map(|w| w.path()))
}

pub fn available_days() -> io::Result<Vec<String>> {
    let Some(base) = FILE
        .get()
        .and_then(|w| w.lock().ok().map(|w| w.base.clone()))
    else {
        return Ok(Vec::new());
    };
    let parent = base
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut days = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(day) = dated_log_day(&base, &entry.path()) {
                days.push(day.to_string());
            }
        }
    }
    days.sort_unstable_by(|a, b| b.cmp(a));
    Ok(days)
}

pub fn file_for_day(day: &str) -> Option<PathBuf> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    if date.to_string() != day {
        return None;
    }
    let base = FILE.get()?.lock().ok()?.base.clone();
    let path = daily_path(&base, date);
    path.is_file().then_some(path)
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
    fn recording_level_changes_on_the_same_callsite() {
        use tracing_subscriber::filter::dynamic_filter_fn;
        use tracing_subscriber::prelude::*;

        fn event() {
            tracing::info!("dynamic-log-level-test-marker");
        }
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(|| Recent)
                .with_filter(dynamic_filter_fn(|meta, _| enabled(meta))),
        );
        let before = record_level().to_owned();
        tracing::subscriber::with_default(subscriber, || {
            set_record_level("error");
            event();
            set_record_level("trace");
            event();
        });
        set_record_level(&before);
        let lines = recent(None, Some("dynamic-log-level-test-marker"), 10);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn dated_files_rotate_and_expire() {
        let dir = std::env::temp_dir().join(format!("djinn-log-test-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("djinn.log");
        let today = Local::now().date_naive();
        let old = today - Days::days(8);
        let prior = today - Days::days(1);
        std::fs::write(daily_path(&base, old), b"old").unwrap();
        let mut writer = DailyWriter::new(base.clone(), 7).unwrap();
        // Startup must wait for the database retention setting before it
        // removes files; the admin may have chosen more than seven days.
        assert!(daily_path(&base, old).exists());
        writer.write_on(prior, b"prior\n").unwrap();
        writer.write_on(today, b"today\n").unwrap();
        assert!(!daily_path(&base, old).exists());
        assert_eq!(std::fs::read(daily_path(&base, prior)).unwrap(), b"prior\n");
        assert_eq!(std::fs::read(daily_path(&base, today)).unwrap(), b"today\n");
        drop(writer);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
