//! Song lyrics: what was found for each song, where, and the search over
//! their words. Reading files and LRCLIB live in `djinn-library::lyrics`.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// A song's stored lyrics.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct StoredLyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub instrumental: bool,
    pub source: String,
}

/// A song whose file should be read for lyrics.
#[derive(Clone, Debug, FromRow)]
pub struct LyricsToRead {
    pub id: i64,
    pub path: String,
    pub mtime: i64,
}

/// A song to ask LRCLIB about.
#[derive(Clone, Debug, FromRow)]
pub struct LyricsToLookUp {
    pub id: i64,
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
}

/// How the library stands, for Admin.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, FromRow)]
pub struct LyricsCounts {
    pub songs: i64,
    pub synced: i64,
    /// Words without timing.
    pub plain: i64,
    pub instrumental: i64,
    /// Files not read for lyrics yet.
    pub unread: i64,
    /// Songs without lyrics LRCLIB hasn't been asked about.
    pub not_looked_up: i64,
    /// Songs whose lyrics came from an imported zip.
    pub imported: i64,
    /// Songs with lyrics an admin removed as bad.
    pub blocked: i64,
}

/// What saving imported lyrics did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkSave {
    Saved,
    /// The song has lyrics already (or imported ones, kept).
    Kept,
    /// An admin removed exactly these lyrics before.
    Blocked,
}

/// A fingerprint of lyrics' text, to tell whether a source gives the same
/// words again (FNV-1a over the trimmed text, with its length).
pub fn text_hash(synced: Option<&str>, plain: Option<&str>) -> String {
    let text = synced.or(plain).unwrap_or("").trim();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}{:x}", text.len())
}

/// What a song has now, for an import deciding whether to fill it.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct LyricsState {
    pub track_id: i64,
    pub source: String,
    pub online_checked_at: Option<i64>,
}

/// Lyrics to save: found (`source` 'lrc', 'file', 'lrclib' or 'bulk') or not ('none').
pub struct NewLyrics<'a> {
    pub synced: Option<&'a str>,
    pub plain: Option<&'a str>,
    pub instrumental: bool,
    pub source: &'a str,
}

impl NewLyrics<'_> {
    pub const NONE: NewLyrics<'static> = NewLyrics {
        synced: None,
        plain: None,
        instrumental: false,
        source: "none",
    };
}

/// How long until LRCLIB is asked again about a song it didn't know.
pub const LYRICS_RECHECK_MS: i64 = 90 * 24 * 60 * 60 * 1000;

impl Store {
    pub async fn lyrics(&self, track_id: i64) -> Result<Option<StoredLyrics>> {
        Ok(sqlx::query_as(
            "SELECT synced, plain, instrumental, source FROM lyrics
             WHERE track_id = ? AND source != 'none'",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Songs never read for lyrics, or whose file changed since; songs with
    /// lyrics from LRCLIB keep them.
    pub async fn lyrics_to_read(&self, limit: i64) -> Result<Vec<LyricsToRead>> {
        Ok(sqlx::query_as(
            "SELECT t.id, t.path, t.mtime FROM tracks t
             LEFT JOIN lyrics l ON l.track_id = t.id
             WHERE t.removed_at IS NULL
               AND (l.track_id IS NULL OR (l.source != 'lrclib' AND l.file_mtime IS NOT t.mtime))
             ORDER BY t.id LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// What reading a song's file found. Nothing found keeps an earlier
    /// LRCLIB check, so it isn't asked again early, and keeps imported
    /// lyrics; anything found beside the song wins over them.
    pub async fn save_read_lyrics(
        &self,
        track_id: i64,
        file_mtime: i64,
        found: &NewLyrics<'_>,
    ) -> Result<()> {
        // Lyrics an admin removed stay removed while the file says the same.
        let found = if found.source != "none"
            && self
                .lyrics_blocked(track_id, found.source, found.synced, found.plain)
                .await?
        {
            &NewLyrics::NONE
        } else {
            found
        };
        sqlx::query(
            "INSERT INTO lyrics (track_id, synced, plain, instrumental, source, file_mtime, updated_at)
             SELECT id, ?, ?, ?, ?, ?, ? FROM tracks WHERE id = ?
             ON CONFLICT (track_id) DO UPDATE SET
               synced = CASE WHEN lyrics.source = 'bulk' AND excluded.source = 'none'
                             THEN lyrics.synced ELSE excluded.synced END,
               plain = CASE WHEN lyrics.source = 'bulk' AND excluded.source = 'none'
                            THEN lyrics.plain ELSE excluded.plain END,
               instrumental = CASE WHEN lyrics.source = 'bulk' AND excluded.source = 'none'
                                   THEN lyrics.instrumental ELSE excluded.instrumental END,
               source = CASE WHEN lyrics.source = 'bulk' AND excluded.source = 'none'
                             THEN lyrics.source ELSE excluded.source END,
               file_mtime = excluded.file_mtime, updated_at = excluded.updated_at",
        )
        .bind(found.synced)
        .bind(found.plain)
        .bind(found.instrumental)
        .bind(found.source)
        .bind(file_mtime)
        .bind(now_ms())
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Songs without lyrics that LRCLIB wasn't asked about lately, with an
    /// artist and title to ask with.
    pub async fn lyrics_to_look_up(&self, limit: i64) -> Result<Vec<LyricsToLookUp>> {
        Ok(sqlx::query_as(
            "SELECT t.id, t.artist, t.title, t.album, t.duration_ms FROM lyrics l
             JOIN tracks t ON t.id = l.track_id
             WHERE l.source = 'none' AND t.removed_at IS NULL
               AND t.artist IS NOT NULL AND t.artist != '' AND t.title != ''
               AND (l.online_checked_at IS NULL OR l.online_checked_at < ?)
             ORDER BY t.id LIMIT ?",
        )
        .bind(now_ms() - LYRICS_RECHECK_MS)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// What LRCLIB said: lyrics, or (`None`) that it doesn't know the song.
    pub async fn save_looked_up_lyrics(
        &self,
        track_id: i64,
        found: Option<&NewLyrics<'_>>,
    ) -> Result<()> {
        let now = now_ms();
        let blocked = match found {
            Some(f) => {
                self.lyrics_blocked(track_id, f.source, f.synced, f.plain)
                    .await?
            }
            None => false,
        };
        let found = found.filter(|_| !blocked);
        match found {
            Some(found) => {
                sqlx::query(
                    "UPDATE lyrics SET synced = ?, plain = ?, instrumental = ?, source = ?,
                            online_checked_at = ?, updated_at = ?
                     WHERE track_id = ? AND source = 'none'",
                )
                .bind(found.synced)
                .bind(found.plain)
                .bind(found.instrumental)
                .bind(found.source)
                .bind(now)
                .bind(now)
                .bind(track_id)
                .execute(&self.pool)
                .await?;
            }
            None => {
                sqlx::query("UPDATE lyrics SET online_checked_at = ? WHERE track_id = ?")
                    .bind(now)
                    .bind(track_id)
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Reads every file again and asks LRCLIB again about songs without
    /// lyrics. Lyrics found online are kept.
    pub async fn recheck_lyrics(&self) -> Result<()> {
        sqlx::query(
            "UPDATE lyrics SET file_mtime = NULL,
                    online_checked_at = CASE WHEN source = 'none' THEN NULL ELSE online_checked_at END
             WHERE source != 'lrclib'",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Where each of these songs' lyrics stand (songs never read have no row).
    pub async fn lyrics_states(&self, ids: &[i64]) -> Result<Vec<LyricsState>> {
        Ok(sqlx::query_as(
            "SELECT track_id, source, online_checked_at FROM lyrics
             WHERE track_id IN (SELECT value FROM json_each(?))",
        )
        .bind(crate::library::json_ids(ids))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Saves imported lyrics for a song with none (no row, or source 'none').
    /// Timed lyrics also take the place of imported plain ones; with
    /// `replace`, imported lyrics are replaced by newer ones (but never timed
    /// by plain). Lyrics from files, tags or LRCLIB are never touched, and
    /// lyrics an admin removed aren't brought back.
    pub async fn save_bulk_lyrics(
        &self,
        track_id: i64,
        found: &NewLyrics<'_>,
        replace: bool,
    ) -> Result<BulkSave> {
        if self
            .lyrics_blocked(track_id, "bulk", found.synced, found.plain)
            .await?
        {
            return Ok(BulkSave::Blocked);
        }
        let done = sqlx::query(
            "INSERT INTO lyrics (track_id, synced, plain, instrumental, source, file_mtime, updated_at)
             SELECT id, ?, ?, ?, 'bulk', NULL, ? FROM tracks WHERE id = ?
             ON CONFLICT (track_id) DO UPDATE SET
               synced = excluded.synced, plain = excluded.plain,
               instrumental = excluded.instrumental, source = 'bulk',
               file_mtime = NULL, updated_at = excluded.updated_at
             WHERE lyrics.source = 'none'
                OR (lyrics.source = 'bulk'
                    AND ((excluded.synced IS NOT NULL AND lyrics.synced IS NULL)
                         OR (? AND (excluded.synced IS NOT NULL OR lyrics.synced IS NULL))))",
        )
        .bind(found.synced)
        .bind(found.plain)
        .bind(found.instrumental)
        .bind(now_ms())
        .bind(track_id)
        .bind(replace)
        .execute(&self.pool)
        .await?;
        Ok(if done.rows_affected() > 0 {
            BulkSave::Saved
        } else {
            BulkSave::Kept
        })
    }

    /// True when an admin removed lyrics from this source with this text.
    pub async fn lyrics_blocked(
        &self,
        track_id: i64,
        source: &str,
        synced: Option<&str>,
        plain: Option<&str>,
    ) -> Result<bool> {
        let hash: Option<String> = sqlx::query_scalar(
            "SELECT text_hash FROM lyrics_blocks WHERE track_id = ? AND source = ?",
        )
        .bind(track_id)
        .bind(source)
        .fetch_optional(&self.pool)
        .await?;
        Ok(hash.is_some_and(|h| h == text_hash(synced, plain)))
    }

    /// Takes a song's lyrics away as wrong or bad and remembers where they
    /// came from, so that source's same text isn't taken again. The source
    /// removed, or None when the song has none.
    pub async fn block_lyrics(
        &self,
        track_id: i64,
        admin_id: i64,
        reason: Option<&str>,
    ) -> Result<Option<String>> {
        let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT source, synced, plain FROM lyrics WHERE track_id = ? AND source != 'none'",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((source, synced, plain)) = row else {
            return Ok(None);
        };
        let now = now_ms();
        let mut tx = self.write().await?;
        sqlx::query(
            "INSERT OR REPLACE INTO lyrics_blocks
               (track_id, source, text_hash, reason, blocked_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(track_id)
        .bind(&source)
        .bind(text_hash(synced.as_deref(), plain.as_deref()))
        .bind(reason)
        .bind(admin_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE lyrics SET synced = NULL, plain = NULL, instrumental = 0, source = 'none',
                    online_checked_at = ?, updated_at = ?
             WHERE track_id = ?",
        )
        .bind(now)
        .bind(now)
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(source))
    }

    /// Lets removed lyrics come back (every song's, or one song's): the
    /// songs are read and asked about again. How many blocks were lifted.
    pub async fn allow_lyrics_again(&self, track_id: Option<i64>) -> Result<u64> {
        let mut tx = self.write().await?;
        sqlx::query(
            "UPDATE lyrics SET file_mtime = NULL, online_checked_at = NULL
             WHERE source = 'none'
               AND track_id IN (SELECT track_id FROM lyrics_blocks WHERE ?1 IS NULL OR track_id = ?1)",
        )
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
        let done = sqlx::query("DELETE FROM lyrics_blocks WHERE ?1 IS NULL OR track_id = ?1")
            .bind(track_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(done.rows_affected())
    }

    /// Takes back every imported lyric; those songs can be looked up again.
    /// How many there were.
    pub async fn remove_bulk_lyrics(&self) -> Result<u64> {
        let done = sqlx::query(
            "UPDATE lyrics SET synced = NULL, plain = NULL, instrumental = 0, source = 'none',
                    file_mtime = NULL, online_checked_at = NULL, updated_at = ?
             WHERE source = 'bulk'",
        )
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    pub async fn lyrics_counts(&self) -> Result<LyricsCounts> {
        Ok(sqlx::query_as(
            "SELECT
               (SELECT count(*) FROM tracks WHERE removed_at IS NULL) AS songs,
               count(CASE WHEN l.synced IS NOT NULL THEN 1 END) AS synced,
               count(CASE WHEN l.synced IS NULL AND l.plain IS NOT NULL THEN 1 END) AS plain,
               count(CASE WHEN l.instrumental THEN 1 END) AS instrumental,
               (SELECT count(*) FROM tracks t LEFT JOIN lyrics x ON x.track_id = t.id
                WHERE t.removed_at IS NULL
                  AND (x.track_id IS NULL OR (x.source != 'lrclib' AND x.file_mtime IS NOT t.mtime))) AS unread,
               count(CASE WHEN l.source = 'none'
                           AND (l.online_checked_at IS NULL OR l.online_checked_at < ?) THEN 1 END)
                 AS not_looked_up,
               count(CASE WHEN l.source = 'bulk' THEN 1 END) AS imported,
               (SELECT count(*) FROM lyrics_blocks) AS blocked
             FROM lyrics l JOIN tracks t ON t.id = l.track_id WHERE t.removed_at IS NULL",
        )
        .bind(now_ms() - LYRICS_RECHECK_MS)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Songs whose words contain every word typed (the last may be the start
    /// of a word), best matches first, with their words to pick a line from.
    pub async fn search_lyrics(&self, query: &str, limit: i64) -> Result<Vec<(i64, String)>> {
        let words: Vec<&str> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let Some((last, rest)) = words.split_last() else {
            return Ok(Vec::new());
        };
        let mut terms: Vec<String> = rest.iter().map(|w| format!("\"{w}\"")).collect();
        terms.push(format!("\"{last}\"*"));
        Ok(sqlx::query_as(
            "SELECT f.rowid, f.plain FROM lyrics_fts f
             JOIN tracks t ON t.id = f.rowid AND t.removed_at IS NULL
             WHERE lyrics_fts MATCH ? ORDER BY bm25(lyrics_fts) LIMIT ?",
        )
        .bind(terms.join(" "))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Every song's lyrics with its file, for exporting `.lrc` files.
    pub async fn lyrics_for_export(&self) -> Result<Vec<(String, Option<String>, Option<String>)>> {
        Ok(sqlx::query_as(
            "SELECT t.path, l.synced, l.plain FROM lyrics l JOIN tracks t ON t.id = l.track_id
             WHERE t.removed_at IS NULL AND (l.synced IS NOT NULL OR l.plain IS NOT NULL)
             ORDER BY t.path",
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store_with(titles: &[&str]) -> (Store, Vec<i64>) {
        let store = Store::open_in_memory().await.unwrap();
        let scanned: Vec<crate::ScannedTrack> = titles
            .iter()
            .map(|t| crate::ScannedTrack {
                path: format!("/m/{t}.flac"),
                size: 1,
                mtime: 7,
                title: (*t).to_owned(),
                artist: Some("Band".into()),
                ..Default::default()
            })
            .collect();
        store.save_scanned(&scanned).await.unwrap();
        let mut ids = Vec::new();
        for t in titles {
            ids.push(
                sqlx::query_scalar::<_, i64>("SELECT id FROM tracks WHERE title = ?")
                    .bind(t)
                    .fetch_one(&store.pool)
                    .await
                    .unwrap(),
            );
        }
        (store, ids)
    }

    #[tokio::test]
    async fn files_are_read_once_and_again_when_they_change() {
        let (store, ids) = store_with(&["One", "Two"]).await;
        assert_eq!(store.lyrics_to_read(10).await.unwrap().len(), 2);
        let words = NewLyrics {
            synced: Some("[00:01.00]Hello world"),
            plain: Some("Hello world"),
            instrumental: false,
            source: "lrc",
        };
        store.save_read_lyrics(ids[0], 7, &words).await.unwrap();
        store
            .save_read_lyrics(ids[1], 7, &NewLyrics::NONE)
            .await
            .unwrap();
        assert!(store.lyrics_to_read(10).await.unwrap().is_empty());
        assert_eq!(store.lyrics(ids[0]).await.unwrap().unwrap().source, "lrc");
        assert_eq!(store.lyrics(ids[1]).await.unwrap(), None);
        sqlx::query("UPDATE tracks SET mtime = 8 WHERE id = ?")
            .bind(ids[0])
            .execute(&store.pool)
            .await
            .unwrap();
        let again = store.lyrics_to_read(10).await.unwrap();
        assert_eq!(again.iter().map(|t| t.id).collect::<Vec<_>>(), vec![ids[0]]);
        let counts = store.lyrics_counts().await.unwrap();
        assert_eq!(
            (
                counts.songs,
                counts.synced,
                counts.unread,
                counts.not_looked_up
            ),
            (2, 1, 1, 1)
        );
    }

    #[tokio::test]
    async fn lrclib_fills_gaps_and_isnt_asked_again_soon() {
        let (store, ids) = store_with(&["One", "Two"]).await;
        for id in &ids {
            store
                .save_read_lyrics(*id, 7, &NewLyrics::NONE)
                .await
                .unwrap();
        }
        assert_eq!(store.lyrics_to_look_up(10).await.unwrap().len(), 2);
        let found = NewLyrics {
            synced: None,
            plain: Some("Sing along"),
            instrumental: false,
            source: "lrclib",
        };
        store
            .save_looked_up_lyrics(ids[0], Some(&found))
            .await
            .unwrap();
        store.save_looked_up_lyrics(ids[1], None).await.unwrap();
        assert!(store.lyrics_to_look_up(10).await.unwrap().is_empty());
        // Online lyrics stay when the file is read again.
        store.recheck_lyrics().await.unwrap();
        let reread: Vec<i64> = store
            .lyrics_to_read(10)
            .await
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(reread, vec![ids[1]]);
        assert_eq!(store.lyrics_to_look_up(10).await.unwrap().len(), 1);
        assert_eq!(
            store
                .lyrics(ids[0])
                .await
                .unwrap()
                .unwrap()
                .plain
                .as_deref(),
            Some("Sing along")
        );
    }

    #[tokio::test]
    async fn imported_lyrics_fill_gaps_only_and_give_way_to_files() {
        let (store, ids) = store_with(&["One", "Two", "Three"]).await;
        let own = NewLyrics {
            synced: None,
            plain: Some("From a tag"),
            instrumental: false,
            source: "file",
        };
        store.save_read_lyrics(ids[0], 7, &own).await.unwrap();
        store
            .save_read_lyrics(ids[1], 7, &NewLyrics::NONE)
            .await
            .unwrap();
        let bulk = |text: &'static str| NewLyrics {
            synced: None,
            plain: Some(text),
            instrumental: false,
            source: "bulk",
        };
        // A song with lyrics keeps them; an empty one and an unread one are filled.
        let save = |id: i64, text: &'static str, replace: bool| {
            let store = &store;
            async move {
                store
                    .save_bulk_lyrics(id, &bulk(text), replace)
                    .await
                    .unwrap()
                    == BulkSave::Saved
            }
        };
        assert!(!save(ids[0], "Imported", false).await);
        assert!(save(ids[1], "Imported two", false).await);
        assert!(save(ids[2], "Imported three", false).await);
        assert_eq!(
            store
                .lyrics(ids[0])
                .await
                .unwrap()
                .unwrap()
                .plain
                .as_deref(),
            Some("From a tag")
        );
        // Imported lyrics are kept unless asked to replace them.
        assert!(!save(ids[1], "Newer", false).await);
        assert!(save(ids[1], "Newer", true).await);
        assert_eq!(store.lyrics_counts().await.unwrap().imported, 2);
        assert_eq!(
            store.search_lyrics("newer thing", 5).await.unwrap().len(),
            0
        );
        assert_eq!(
            store
                .search_lyrics("imported three", 5)
                .await
                .unwrap()
                .len(),
            1
        );
        // Reading the files finds nothing: the imported words stay, and a
        // file's own lyrics win when there are some.
        store
            .save_read_lyrics(ids[2], 7, &NewLyrics::NONE)
            .await
            .unwrap();
        assert_eq!(store.lyrics(ids[2]).await.unwrap().unwrap().source, "bulk");
        store.save_read_lyrics(ids[2], 7, &own).await.unwrap();
        assert_eq!(store.lyrics(ids[2]).await.unwrap().unwrap().source, "file");
        // Taking the rest back leaves other lyrics alone and frees the song.
        assert_eq!(store.remove_bulk_lyrics().await.unwrap(), 1);
        assert_eq!(store.lyrics(ids[1]).await.unwrap(), None);
        assert!(store.lyrics(ids[0]).await.unwrap().is_some());
        assert!(store.search_lyrics("newer", 5).await.unwrap().is_empty());
        assert_eq!(store.lyrics_to_look_up(10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn timed_imports_beat_plain_ones() {
        let (store, ids) = store_with(&["One"]).await;
        let plain = NewLyrics {
            synced: None,
            plain: Some("Words"),
            instrumental: false,
            source: "bulk",
        };
        let timed = NewLyrics {
            synced: Some("[00:01.00]Words"),
            plain: Some("Words"),
            instrumental: false,
            source: "bulk",
        };
        assert_eq!(
            store.save_bulk_lyrics(ids[0], &plain, false).await.unwrap(),
            BulkSave::Saved
        );
        // Timed takes the place of plain without being asked to replace.
        assert_eq!(
            store.save_bulk_lyrics(ids[0], &timed, false).await.unwrap(),
            BulkSave::Saved
        );
        // Plain never takes the place of timed, even when replacing.
        assert_eq!(
            store.save_bulk_lyrics(ids[0], &plain, true).await.unwrap(),
            BulkSave::Kept
        );
        assert!(store
            .lyrics(ids[0])
            .await
            .unwrap()
            .unwrap()
            .synced
            .is_some());
    }

    #[tokio::test]
    async fn removed_lyrics_stay_removed_until_they_change() {
        let (store, ids) = store_with(&["One"]).await;
        sqlx::query("INSERT INTO users (username, password_hash, role, created_at) VALUES ('a', 'x', 'admin', 1)")
            .execute(&store.pool)
            .await
            .ok();
        let admin: i64 = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
            .fetch_one(&store.pool)
            .await
            .unwrap_or(1);
        let words = NewLyrics {
            synced: None,
            plain: Some("Wrong song's words"),
            instrumental: false,
            source: "lrclib",
        };
        store
            .save_read_lyrics(ids[0], 7, &NewLyrics::NONE)
            .await
            .unwrap();
        store
            .save_looked_up_lyrics(ids[0], Some(&words))
            .await
            .unwrap();
        assert_eq!(
            store
                .block_lyrics(ids[0], admin, Some("wrong song"))
                .await
                .unwrap(),
            Some("lrclib".to_owned())
        );
        assert_eq!(store.lyrics(ids[0]).await.unwrap(), None);
        assert!(store
            .search_lyrics("wrong song", 5)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(store.lyrics_counts().await.unwrap().blocked, 1);
        // The same text from that source is refused; different text is not.
        store
            .save_looked_up_lyrics(ids[0], Some(&words))
            .await
            .unwrap();
        assert_eq!(store.lyrics(ids[0]).await.unwrap(), None);
        // Imports and files are other sources.
        let bulk = NewLyrics {
            source: "bulk",
            ..words_like(&words)
        };
        assert_eq!(
            store.save_bulk_lyrics(ids[0], &bulk, false).await.unwrap(),
            BulkSave::Saved
        );
        assert_eq!(
            store.block_lyrics(ids[0], admin, None).await.unwrap(),
            Some("bulk".into())
        );
        assert_eq!(
            store.save_bulk_lyrics(ids[0], &bulk, false).await.unwrap(),
            BulkSave::Blocked
        );
        let fixed = NewLyrics {
            plain: Some("Right words"),
            ..words_like(&bulk)
        };
        assert_eq!(
            store.save_bulk_lyrics(ids[0], &fixed, false).await.unwrap(),
            BulkSave::Saved
        );
        assert_eq!(store.allow_lyrics_again(None).await.unwrap(), 2);
        assert_eq!(store.lyrics_counts().await.unwrap().blocked, 0);
    }

    fn words_like<'a>(l: &NewLyrics<'a>) -> NewLyrics<'a> {
        NewLyrics {
            synced: l.synced,
            plain: l.plain,
            instrumental: l.instrumental,
            source: l.source,
        }
    }

    #[tokio::test]
    async fn words_are_searchable_and_follow_changes() {
        let (store, ids) = store_with(&["One", "Two"]).await;
        let save = |id: i64, plain: &'static str| {
            let store = &store;
            async move {
                let words = NewLyrics {
                    synced: None,
                    plain: Some(plain),
                    instrumental: false,
                    source: "file",
                };
                store.save_read_lyrics(id, 7, &words).await.unwrap();
            }
        };
        save(ids[0], "We will rock you\nBuddy you're a boy").await;
        save(ids[1], "Is this the real life").await;
        let hits = store.search_lyrics("rock yo", 10).await.unwrap();
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![ids[0]]);
        assert!(store
            .search_lyrics("real rock", 10)
            .await
            .unwrap()
            .is_empty());
        save(ids[0], "Something else now").await;
        assert!(store
            .search_lyrics("rock you", 10)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(store.search_lyrics("something", 10).await.unwrap().len(), 1);
        assert_eq!(store.lyrics_for_export().await.unwrap().len(), 2);
    }
}
