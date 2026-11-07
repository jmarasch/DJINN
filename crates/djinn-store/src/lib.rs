//! DJINN storage: SQLite access, migrations, and models.

mod duplicates;
mod identify;
mod library;
mod loudness;
mod lyrics;
mod maintenance;
mod nodes;
mod outputs;
mod party;
mod personal;
mod playlists;
mod problems;
mod queue;
mod roles;
mod rooms;
mod searches;
mod security;
mod stats;
mod submissions;
mod subsonic;
mod tag_edits;
mod users;

pub use duplicates::*;
pub use identify::*;
pub use library::*;
pub use loudness::*;
pub use lyrics::*;
pub use nodes::*;
pub use outputs::*;
pub use party::*;
pub use personal::*;
pub use playlists::*;
pub use problems::*;
pub use queue::*;
pub use roles::*;
pub use rooms::*;
pub use searches::*;
pub use security::*;
pub use stats::*;
pub use submissions::*;
pub use subsonic::*;
pub use tag_edits::*;
pub use users::*;

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

pub use sqlx;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Set by a migration that leaves space worth giving back.
const COMPACT_DATABASE: &str = "compact_database";

/// Handle to the database. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Store {
    pool: SqlitePool,
    /// The library lists, kept while they stay current.
    browse: Arc<library::BrowseMemo>,
}

impl Store {
    /// Opens (creating if needed) the database file and applies migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(sqlx::Error::Io)?;
        }
        keep_private(path).map_err(sqlx::Error::Io)?;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(common(options))
            .await?;
        Self::init(pool).await
    }

    /// Opens a separate pool for the jukebox after the main store has run
    /// migrations. Browse requests cannot occupy these connections.
    pub async fn open_playback(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(common(options))
            .await?;
        Ok(Self {
            pool,
            browse: Arc::default(),
        })
    }

    /// A private in-memory database, for tests.
    pub async fn open_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?;
        // Each in-memory connection is its own database, so keep exactly one.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(common(options))
            .await?;
        Self::init(pool).await
    }

    async fn init(pool: SqlitePool) -> Result<Self> {
        MIGRATOR.run(&pool).await?;
        let store = Self {
            pool,
            browse: Arc::default(),
        };
        store.compact_if_due().await?;
        Ok(store)
    }

    /// Gives back space when a migration asked for it. If that fails (not
    /// enough disk for the copy it makes), everything still works and the
    /// next start tries again.
    async fn compact_if_due(&self) -> Result<()> {
        if self.get_setting(COMPACT_DATABASE).await?.is_none() {
            return Ok(());
        }
        if sqlx::query("VACUUM").execute(&self.pool).await.is_ok() {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(COMPACT_DATABASE)
                .execute(&self.pool)
                .await?;
            // The copy went through the write-ahead log; don't keep it twice.
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Closes all connections, checkpointing the WAL.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Writes a consistent copy of the whole database to `to`, which must
    /// not exist yet. Safe while DJINN runs. (An in-memory database writes
    /// nothing.)
    pub async fn snapshot(&self, to: &Path) -> Result<()> {
        sqlx::query("VACUUM INTO ?")
            .bind(to.to_string_lossy().into_owned())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Points songs under the folder `from` at `to` instead, after the
    /// library moved (a backup restored on another computer). Separators
    /// change to `sep` when the old path used the other kind.
    pub async fn move_library(&self, from: &str, to: &str, sep: char) -> Result<u64> {
        let old_sep = if from.contains('\\') { '\\' } else { '/' };
        let moved = sqlx::query(
            "UPDATE tracks SET path = ?1 || replace(substr(path, length(?2) + 1), ?3, ?4) \
             WHERE substr(path, 1, length(?2)) = ?2 AND substr(path, length(?2) + 1, 1) IN ('/', '\\')",
        )
        .bind(to)
        .bind(from)
        .bind(old_sep.to_string())
        .bind(sep.to_string())
        .execute(&self.pool)
        .await?;
        Ok(moved.rows_affected())
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let value = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(value)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl Store {
    /// A transaction that takes the write lock at once (BEGIN IMMEDIATE).
    /// A plain transaction that reads first and writes later can't wait for
    /// another writer: SQLite fails it straight away with "database is
    /// locked". Taking the lock up front waits its turn instead.
    pub(crate) async fn write(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }
}

fn common(options: SqliteConnectOptions) -> SqliteConnectOptions {
    options
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(15))
}

/// Current time as Unix epoch milliseconds, the timestamp format in the schema.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod playback_pool_tests {
    use super::*;

    /// A library check and sound analysis writing at once: each waits for
    /// the other instead of failing with "database is locked".
    #[tokio::test]
    async fn a_library_check_and_analysis_can_write_at_once() {
        let dir = std::env::temp_dir().join(format!(
            "djinn-write-race-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("djinn.db")).await.unwrap();
        let song = |i: usize| crate::ScannedTrack {
            path: format!("/m/{i}.flac"),
            title: format!("Song {i}"),
            artist: Some("Band".into()),
            album: Some("Album".into()),
            ..Default::default()
        };
        store
            .save_scanned(&(0..200).map(song).collect::<Vec<_>>())
            .await
            .unwrap();
        let ids = store.library_track_ids().await.unwrap();
        let analysis = {
            let store = store.clone();
            tokio::spawn(async move {
                for round in 0..4 {
                    for &id in &ids {
                        store
                            .save_analysis(id, 1, round, Some((-10.0, 0.9)), None, Some(&[1, 2]))
                            .await
                            .unwrap();
                    }
                }
            })
        };
        for _ in 0..10 {
            store
                .save_scanned(&(0..200).map(song).collect::<Vec<_>>())
                .await
                .unwrap();
        }
        analysis.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn playback_pool_works_when_browse_pool_is_full() {
        let dir = std::env::temp_dir().join(format!(
            "djinn-playback-pool-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("djinn.db");
        let browse = Store::open(&path).await.unwrap();
        let playback = Store::open_playback(&path).await.unwrap();

        let mut held = Vec::new();
        for _ in 0..8 {
            held.push(browse.pool().acquire().await.unwrap());
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            playback.set_setting("playback_pool_test", "ready").await?;
            assert_eq!(
                playback.get_setting("playback_pool_test").await?,
                Some("ready".to_owned())
            );
            Ok::<(), StoreError>(())
        })
        .await
        .unwrap()
        .unwrap();

        drop(held);
        assert_eq!(
            browse.get_setting("playback_pool_test").await.unwrap(),
            Some("ready".to_owned())
        );
        playback.close().await;
        browse.close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Only this account can read the database: it holds password hashes, two-step
/// secrets, app passwords and API keys. SQLite gives its side files the same
/// permissions. On Windows the folder DJINN installs to is private already.
#[cfg(unix)]
fn keep_private(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    let name = path.as_os_str();
    for suffix in ["", "-wal", "-shm"] {
        let mut file = name.to_owned();
        file.push(suffix);
        match std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn keep_private(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn party_and_go_playlists_become_one_system() {
        const MERGE: i64 = 32;
        let pool = {
            let options = SqliteConnectOptions::from_str("sqlite::memory:").unwrap();
            SqlitePoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
                .connect_with(common(options))
                .await
                .unwrap()
        };
        let before = sqlx::migrate::Migrator {
            migrations: MIGRATOR
                .migrations
                .iter()
                .filter(|m| m.version < MERGE)
                .cloned()
                .collect::<Vec<_>>()
                .into(),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        before.run(&pool).await.unwrap();
        // GO playlists 1 and 7, party playlists 1 and 2 (the one playing).
        // Party playlist 1 has a song the library lacks.
        for sql in [
            "INSERT INTO users (id, username, password_hash, role, created_at)
             VALUES (1, 'root', 'h', 'admin', 0), (2, 'sam', 'h', 'user', 0)",
            "INSERT INTO tracks (id, path, size, mtime, title, added_at)
             VALUES (10, '/a', 0, 0, 'A', 0), (11, '/b', 0, 0, 'B', 0)",
            "INSERT INTO go_playlists (id, user_id, name, shared, created_at, updated_at)
             VALUES (1, 2, 'Private', 0, 5, 6), (7, 2, 'Open', 1, 5, 6)",
            "INSERT INTO go_playlist_tracks (playlist_id, position, track_id)
             VALUES (1, 0, 11), (1, 1, 10), (1, 2, 11)",
            "INSERT INTO playlists (id, owner_id, name, shuffle, created_at, updated_at)
             VALUES (1, 1, 'Dance', 0, 5, 6), (2, 1, 'Chill', 1, 5, 6)",
            "INSERT INTO playlist_wanted (id, playlist_id, title) VALUES (3, 1, 'Gap')",
            "INSERT INTO playlist_entries (id, playlist_id, kind, ref_id, position, added_at)
             VALUES (40, 1, 'track', 10, 1, 0), (41, 1, 'wanted', 3, 2, 0)",
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('active_playlist', '2')",
            "INSERT OR REPLACE INTO roles (id, name, builtin, created_at) VALUES (9, 'Old', 0, 0)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }

        MIGRATOR.run(&pool).await.unwrap();
        let store = Store {
            pool,
            browse: Arc::default(),
        };

        // GO's keep their ids, with the repeat at its first place.
        let private = store.playlist(1).await.unwrap().unwrap();
        assert_eq!(private.playlist.name, "Private");
        assert!(!private.playlist.shared);
        assert_eq!(private.playlist.owner_id, 2);
        let songs: Vec<_> = private.tracks.iter().map(|t| t.id).collect();
        assert_eq!(songs, [11, 10]);
        assert!(store.playlist(7).await.unwrap().unwrap().playlist.shared);

        // Party ones move above them, shared, whole, and still playing.
        let dance = store.playlist(8).await.unwrap().unwrap();
        assert_eq!(dance.playlist.name, "Dance");
        assert!(dance.playlist.shared);
        assert!(!dance.playlist.shuffle);
        assert_eq!(dance.entries.len(), 2);
        assert_eq!(dance.entries[1].name.as_deref(), Some("Gap"));
        assert_eq!(
            store
                .get_setting("active_playlist")
                .await
                .unwrap()
                .as_deref(),
            Some("9")
        );
        assert_eq!(
            store.playlist(9).await.unwrap().unwrap().playlist.name,
            "Chill"
        );

        // Sam sees their own and what's open, and a new role can't take over.
        let sam: Vec<_> = store
            .playlists_for(2)
            .await
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(sam, [7, 1, 9, 8]);
        let root: Vec<_> = store
            .playlists_for(1)
            .await
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(root, [9, 8, 7], "not Sam's private one");
        assert!(!store.role(9).await.unwrap().unwrap().permissions.auto_mix);
    }

    #[tokio::test]
    async fn migrations_apply() {
        let store = Store::open_in_memory().await.unwrap();
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .fetch_all(store.pool())
                .await
                .unwrap();
        for table in [
            "tracks",
            "albums",
            "artists",
            "play_events",
            "request_queue",
        ] {
            assert!(tables.iter().any(|t| t == table), "missing table {table}");
        }
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let store = Store::open_in_memory().await.unwrap();
        assert_eq!(store.get_setting("history_window").await.unwrap(), None);
        store.set_setting("history_window", "25").await.unwrap();
        store.set_setting("history_window", "30").await.unwrap();
        assert_eq!(
            store
                .get_setting("history_window")
                .await
                .unwrap()
                .as_deref(),
            Some("30")
        );
    }

    #[tokio::test]
    async fn fts_triggers_index_tracks_artists_and_albums() {
        let store = Store::open_in_memory().await.unwrap();
        let pool = store.pool();
        let artist: i64 =
            sqlx::query_scalar("INSERT INTO artists (name) VALUES ('Björk') RETURNING id")
                .fetch_one(pool)
                .await
                .unwrap();
        sqlx::query("INSERT INTO albums (title, artist_id) VALUES ('Homogenic', ?)")
            .bind(artist)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tracks (path, size, mtime, title, artist, album, added_at)
             VALUES ('/m/joga.flac', 1, 1, 'Jóga', 'Björk', 'Homogenic', 0)",
        )
        .execute(pool)
        .await
        .unwrap();

        // Prefix match, diacritics folded.
        let hits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tracks_fts WHERE tracks_fts MATCH 'jog*'")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(hits, 1);
        let hits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM artists_fts WHERE artists_fts MATCH 'bjo*'")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(hits, 1);
        let hits: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM albums_fts WHERE albums_fts MATCH 'artist:bjork'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(hits, 1);

        sqlx::query("UPDATE tracks SET title = 'Bachelorette'")
            .execute(pool)
            .await
            .unwrap();
        let hits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tracks_fts WHERE tracks_fts MATCH 'jog*'")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(hits, 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn only_this_account_can_read_the_database() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "djinn-private-db-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("djinn.db");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let store = Store::open(&path).await.unwrap();
        store.set_setting("a", "b").await.unwrap();
        assert_eq!(mode(&path), 0o600, "new");
        assert_eq!(mode(&dir.join("djinn.db-wal")), 0o600, "its side file");
        store.close().await;

        // One from before, as a default umask left it.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let store = Store::open(&path).await.unwrap();
        assert_eq!(mode(&path), 0o600, "existing");
        store.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn file_database_uses_wal() {
        let dir = std::env::temp_dir().join(format!("djinn-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let store = Store::open(&path).await.unwrap();
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        store.pool().close().await;
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
