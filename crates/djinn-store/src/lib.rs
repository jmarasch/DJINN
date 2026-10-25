//! DJINN storage: SQLite access, migrations, and models.

use std::path::Path;
use std::str::FromStr;
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

/// Handle to the database. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Opens (creating if needed) the database file and applies migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(sqlx::Error::Io)?;
        }
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
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Closes all connections, checkpointing the WAL.
    pub async fn close(&self) {
        self.pool.close().await;
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

fn common(options: SqliteConnectOptions) -> SqliteConnectOptions {
    options
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
}

/// Current time as Unix epoch milliseconds, the timestamp format in the schema.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

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
