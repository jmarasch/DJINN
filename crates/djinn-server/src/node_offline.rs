//! A node's cache and what it plays when Main can't be reached.
//!
//! While joined, the node keeps copies of the songs coming up in Main's room
//! (`<db dir>/node-cache/<id>.<ext>`), trimmed oldest first past a size
//! limit; kicking wipes them all. If Main goes away, the node plays the
//! cached songs that were coming up, in order, on its own player, then
//! either stops or (if Main's setting says so) keeps playing random cached
//! songs until Main is back.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Weak};
use std::time::SystemTime;

use djinn_engine::{
    Command, Engine, EngineConfig, Event, OutputConfig, OutputDevice, Track, TrackId,
};

use crate::nodes::{AudioSettings, Upcoming};

/// The cache's size limit.
pub const CACHE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Songs a random pick won't repeat.
const NO_REPEAT: usize = 20;

pub struct Cache {
    dir: PathBuf,
    limit: u64,
}

impl Cache {
    pub fn new(dir: PathBuf, limit: u64) -> Cache {
        Cache { dir, limit }
    }

    pub fn path(&self, song: &Upcoming) -> PathBuf {
        self.dir.join(format!("{}.{}", song.id, song.ext))
    }

    pub fn has(&self, song: &Upcoming) -> bool {
        self.path(song).is_file()
    }

    /// Cached songs (finished downloads only).
    pub fn files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e != "part"))
            .collect()
    }

    /// Marks a song as wanted, so trimming keeps it longest.
    pub fn touch(&self, song: &Upcoming) {
        if let Ok(file) = std::fs::File::options().write(true).open(self.path(song)) {
            let _ = file.set_modified(SystemTime::now());
        }
    }

    /// Deletes the oldest songs (by last wanted) until the cache fits,
    /// never the ones in `keep`.
    pub fn trim(&self, keep: &[PathBuf]) {
        let mut files: Vec<(PathBuf, u64, SystemTime)> = self
            .files()
            .into_iter()
            .filter_map(|p| {
                let meta = std::fs::metadata(&p).ok()?;
                Some((p, meta.len(), meta.modified().ok()?))
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.2);
        for (path, size, _) in files {
            if total <= self.limit {
                break;
            }
            if keep.contains(&path) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    /// Where a download lands until it's complete.
    pub fn partial(&self, song: &Upcoming) -> PathBuf {
        self.dir.join(format!("{}.{}.part", song.id, song.ext))
    }
}

/// Plays cached songs on this box while Main is away.
pub struct OfflinePlayer {
    /// Playing while held; dropping it stops the music.
    _engine: Arc<Engine>,
}

impl OfflinePlayer {
    /// Starts with `first` (the cached songs that were coming up), then, if
    /// `keep_going`, random cached songs from `cache` until stopped.
    pub fn start(
        rate: u32,
        audio: Option<&AudioSettings>,
        volume: f32,
        first: Vec<PathBuf>,
        keep_going: bool,
        cache: Vec<PathBuf>,
    ) -> anyhow::Result<OfflinePlayer> {
        let device = audio
            .and_then(|a| a.device.clone())
            .map_or(OutputDevice::Default, OutputDevice::Id);
        let (tx, rx) = mpsc::channel();
        let engine = Arc::new(Engine::start(
            EngineConfig {
                master_rate: rate,
                outputs: vec![OutputConfig {
                    device,
                    delay_ms: 0,
                    ..OutputConfig::system_default(1)
                }],
            },
            move |event| {
                let _ = tx.send(event);
            },
        )?);
        let trim = audio.map_or(1.0, |a| a.trim as f32);
        engine.send(Command::SetVolume((volume * trim).clamp(0.0, 1.0)))?;
        if let Some(audio) = audio {
            engine.send(Command::SetEqualizer(audio.equalizer.engine_config()))?;
        }
        let mut picker = Picker::new(first, keep_going, cache);
        engine.send(Command::SetNext(picker.next()))?;
        engine.send(Command::Play)?;
        let weak: Weak<Engine> = Arc::downgrade(&engine);
        std::thread::Builder::new()
            .name("djinn-node-offline".to_owned())
            .spawn(move || {
                // Ends when the engine (and with it the sender) is dropped.
                while let Ok(event) = rx.recv() {
                    if let Event::NextNeeded { .. } = event {
                        let Some(engine) = weak.upgrade() else { break };
                        let _ = engine.send(Command::SetNext(picker.next()));
                    }
                }
            })?;
        Ok(OfflinePlayer { _engine: engine })
    }
}

/// Chooses what plays offline: the songs that were coming up, then random
/// cached ones (not the last few) if allowed.
struct Picker {
    first: VecDeque<PathBuf>,
    keep_going: bool,
    cache: Vec<PathBuf>,
    recent: VecDeque<PathBuf>,
    next_id: i64,
}

impl Picker {
    fn new(first: Vec<PathBuf>, keep_going: bool, cache: Vec<PathBuf>) -> Picker {
        Picker {
            first: first.into(),
            keep_going,
            cache,
            recent: VecDeque::new(),
            next_id: 1,
        }
    }

    fn pick(&mut self) -> Option<PathBuf> {
        if let Some(path) = self.first.pop_front() {
            return Some(path);
        }
        if !self.keep_going || self.cache.is_empty() {
            return None;
        }
        let fresh: Vec<&PathBuf> = self
            .cache
            .iter()
            .filter(|p| !self.recent.contains(p))
            .collect();
        let pool = if fresh.is_empty() {
            self.cache.iter().collect()
        } else {
            fresh
        };
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).ok()?;
        let index = (u64::from_le_bytes(bytes) % pool.len() as u64) as usize;
        Some(pool[index].clone())
    }

    fn next(&mut self) -> Option<Track> {
        let path = self.pick()?;
        self.recent.push_back(path.clone());
        while self.recent.len() > NO_REPEAT {
            self.recent.pop_front();
        }
        self.next_id += 1;
        Some(Track {
            id: TrackId(self.next_id),
            path,
            gapless: false,
            gain: 1.0,
            start_ms: 0,
        })
    }
}

/// The cached songs that were coming up after the one playing, in order.
pub fn coming_up(cache: &Cache, upcoming: &[Upcoming]) -> Vec<PathBuf> {
    upcoming
        .iter()
        .skip(1)
        .filter(|song| cache.has(song))
        .map(|song| cache.path(song))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: i64) -> Upcoming {
        Upcoming {
            id,
            ext: "mp3".into(),
        }
    }

    #[test]
    fn the_cache_trims_the_oldest_but_keeps_whats_coming_up() {
        let dir = std::env::temp_dir().join(format!("djinn-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = Cache::new(dir.clone(), 25);
        for id in 1..=3 {
            std::fs::write(cache.path(&song(id)), [0u8; 10]).unwrap();
            let old = SystemTime::now() - std::time::Duration::from_secs(100 - id as u64 * 10);
            std::fs::File::options()
                .write(true)
                .open(cache.path(&song(id)))
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        std::fs::write(cache.partial(&song(4)), [0u8; 10]).unwrap();
        assert_eq!(cache.files().len(), 3, "downloads in progress don't count");
        // 30 bytes, limit 25: the oldest (1) goes, unless it's kept.
        cache.trim(&[cache.path(&song(1))]);
        assert!(cache.has(&song(1)) && !cache.has(&song(2)) && cache.has(&song(3)));
        assert_eq!(
            coming_up(&cache, &[song(9), song(3), song(2), song(1)]),
            vec![cache.path(&song(3)), cache.path(&song(1))]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_plays_whats_coming_then_stops_or_keeps_going() {
        let a = PathBuf::from("/c/1.mp3");
        let b = PathBuf::from("/c/2.mp3");
        let mut stops = Picker::new(vec![a.clone()], false, vec![a.clone(), b.clone()]);
        assert_eq!(stops.next().map(|t| t.path), Some(a.clone()));
        assert!(stops.next().is_none());

        let mut keeps = Picker::new(vec![a.clone()], true, vec![a.clone(), b.clone()]);
        assert_eq!(keeps.next().map(|t| t.path), Some(a));
        // The only song not just played.
        assert_eq!(keeps.next().map(|t| t.path), Some(b));
        assert!(keeps.next().is_some(), "carries on");
    }
}
