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
    /// 0 nothing, 1 an AI made the words by listening (guesses), 2 a
    /// person's words timed by an AI.
    pub ai: i64,
    /// What made them, when they say.
    pub ai_model: Option<String>,
    /// For 2: how well the timing matched the words, in percent.
    pub ai_match: Option<i64>,
    /// The zip they came from, when imported.
    pub import_name: Option<String>,
}

/// What an AI did to imported lyrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiMark<'a> {
    /// 1 made by listening, 2 timed by an AI.
    pub kind: i64,
    pub model: &'a str,
    pub match_pct: Option<i64>,
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
    /// Songs whose lyrics an AI made or timed.
    pub ai: i64,
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
            "SELECT synced, plain, instrumental, source, ai, ai_model, ai_match, import_name FROM lyrics
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

    /// Timed lyrics found for a song that has only words (or none), asked
    /// for from the library editor's Make timed. Returns whether they went in
    /// (not when the song is timed already, or these were blocked).
    pub async fn save_timed_lyrics(&self, track_id: i64, found: &NewLyrics<'_>) -> Result<bool> {
        if found.synced.is_none()
            || self
                .lyrics_blocked(track_id, found.source, found.synced, found.plain)
                .await?
        {
            return Ok(false);
        }
        let now = now_ms();
        let done = sqlx::query(
            "UPDATE lyrics SET synced = ?, plain = ?, instrumental = 0, source = ?,
                    ai = 0, ai_model = NULL, ai_match = NULL, online_checked_at = ?, updated_at = ?
             WHERE track_id = ? AND synced IS NULL",
        )
        .bind(found.synced)
        .bind(found.plain)
        .bind(found.source)
        .bind(now)
        .bind(now)
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
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
               ai = CASE WHEN lyrics.source = 'bulk' AND excluded.source = 'none'
                         THEN lyrics.ai ELSE 0 END,
               ai_model = CASE WHEN lyrics.source = 'bulk' AND excluded.source = 'none'
                               THEN lyrics.ai_model ELSE NULL END,
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
             WHERE (l.source = 'none' OR l.ai = 1) AND t.removed_at IS NULL
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
                            ai = 0, ai_model = NULL, ai_match = NULL, online_checked_at = ?, updated_at = ?
                     WHERE track_id = ? AND (source = 'none' OR ai = 1)",
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
    /// With `replace_other`, imported lyrics that rank better also replace
    /// lyrics from a `.lrc` file, the tags or LRCLIB (timed over plain, say);
    /// the song's file isn't read again for them until a recheck.
    /// Timed lyrics also take the place of imported plain ones; with
    /// `replace`, imported lyrics are replaced by newer ones (but never timed
    /// by plain). Lyrics from files, tags or LRCLIB are never touched, and
    /// lyrics an admin removed aren't brought back.
    pub async fn save_bulk_lyrics(
        &self,
        track_id: i64,
        found: &NewLyrics<'_>,
        replace: bool,
        ai: Option<&AiMark<'_>>,
        import_name: &str,
        replace_other: bool,
    ) -> Result<BulkSave> {
        if self
            .lyrics_blocked(track_id, "bulk", found.synced, found.plain)
            .await?
        {
            return Ok(BulkSave::Blocked);
        }
        // Ranks: a person's timed lyrics 4, a person's words timed by an AI
        // 3, a person's plain words 2, words an AI made by listening 1. A
        // better one replaces a worse one; an equal one only when asked to.
        let rank: i64 = match ai.map(|a| a.kind) {
            Some(2) => 3,
            Some(_) => 1,
            None if found.synced.is_some() => 4,
            None => 2,
        };
        let done = sqlx::query(
            "INSERT INTO lyrics (track_id, synced, plain, instrumental, source, ai, ai_model, ai_match,
                                 import_name, imported_at, file_mtime, updated_at)
             SELECT id, ?1, ?2, ?3, 'bulk', ?4, ?5, ?6, ?7, ?8, NULL, ?8 FROM tracks WHERE id = ?9
             ON CONFLICT (track_id) DO UPDATE SET
               synced = excluded.synced, plain = excluded.plain,
               instrumental = excluded.instrumental, source = 'bulk',
               ai = excluded.ai, ai_model = excluded.ai_model, ai_match = excluded.ai_match,
               import_name = excluded.import_name, imported_at = excluded.imported_at,
               file_mtime = CASE WHEN lyrics.source IN ('lrc', 'file', 'lrclib')
                                 THEN (SELECT mtime FROM tracks WHERE id = lyrics.track_id) END,
               updated_at = excluded.updated_at
             WHERE lyrics.source = 'none'
                OR (?12 AND lyrics.source IN ('lrc', 'file', 'lrclib')
                    AND ?11 > CASE WHEN lyrics.synced IS NOT NULL THEN 4 ELSE 2 END)
                OR (lyrics.source = 'bulk'
                    AND (?11 > CASE WHEN lyrics.ai = 2 THEN 3 WHEN lyrics.ai = 1 THEN 1
                                    WHEN lyrics.synced IS NOT NULL THEN 4 ELSE 2 END
                         OR (?11 = CASE WHEN lyrics.ai = 2 THEN 3 WHEN lyrics.ai = 1 THEN 1
                                        WHEN lyrics.synced IS NOT NULL THEN 4 ELSE 2 END AND ?10)))",
        )
        .bind(found.synced)
        .bind(found.plain)
        .bind(found.instrumental)
        .bind(ai.map_or(0, |a| a.kind))
        .bind(ai.map(|a| a.model))
        .bind(ai.and_then(|a| a.match_pct))
        .bind(import_name)
        .bind(now_ms())
        .bind(track_id)
        .bind(replace)
        .bind(rank)
        .bind(replace_other)
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
                    ai_removed = CASE WHEN ai > 0 THEN 1 ELSE ai_removed END,
                    ai = 0, ai_model = NULL, ai_match = NULL,
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
            "UPDATE lyrics SET file_mtime = NULL, online_checked_at = NULL, ai_removed = 0
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
                    ai = 0, ai_model = NULL, ai_match = NULL, import_name = NULL, imported_at = NULL,
                    file_mtime = NULL, online_checked_at = NULL, updated_at = ?
             WHERE source = 'bulk'",
        )
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Marks a song the AI listener heard no singing in as an instrumental
    /// (only while it has no lyrics).
    pub async fn mark_instrumental(&self, track_id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE lyrics SET instrumental = 1, updated_at = ?
             WHERE track_id = ? AND source = 'none'",
        )
        .bind(now_ms())
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
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
               count(CASE WHEN l.ai > 0 THEN 1 END) AS ai,
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
                    .save_bulk_lyrics(id, &bulk(text), replace, None, "z", false)
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
            store
                .save_bulk_lyrics(ids[0], &plain, false, None, "z", false)
                .await
                .unwrap(),
            BulkSave::Saved
        );
        // Timed takes the place of plain without being asked to replace.
        assert_eq!(
            store
                .save_bulk_lyrics(ids[0], &timed, false, None, "z", false)
                .await
                .unwrap(),
            BulkSave::Saved
        );
        // Plain never takes the place of timed, even when replacing.
        assert_eq!(
            store
                .save_bulk_lyrics(ids[0], &plain, true, None, "z", false)
                .await
                .unwrap(),
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
    async fn removed_ai_lyrics_are_marked_and_not_taken_up_again() {
        let (store, ids) = store_with(&["One", "Two"]).await;
        sqlx::query("INSERT INTO users (username, password_hash, role, created_at) VALUES ('a', 'x', 'admin', 1)")
            .execute(&store.pool)
            .await
            .unwrap();
        let admin: i64 = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let made = NewLyrics {
            synced: Some("[00:01.00]la"),
            plain: Some("la"),
            instrumental: false,
            source: "bulk",
        };
        let mark = AiMark {
            kind: 1,
            model: "m",
            match_pct: None,
        };
        store
            .save_bulk_lyrics(ids[0], &made, false, Some(&mark), "z", false)
            .await
            .unwrap();
        store
            .save_read_lyrics(ids[1], 7, &NewLyrics::NONE)
            .await
            .unwrap();
        store.block_lyrics(ids[0], admin, None).await.unwrap();
        let t = store.track(ids[0]).await.unwrap().unwrap();
        assert_eq!((t.lyrics, t.lyrics_ai, t.lyrics_failed), (0, 0, 1));
        // The AI isn't handed it again; the other song without lyrics is.
        let jobs = store
            .ai_claim("w", true, false, true, 10, 60_000, &[])
            .await
            .unwrap();
        assert!(jobs.iter().all(|j| j.track_id != ids[0]));
        // Allowing its lyrics again clears the mark.
        store.allow_lyrics_again(Some(ids[0])).await.unwrap();
        let t = store.track(ids[0]).await.unwrap().unwrap();
        assert_eq!(t.lyrics_failed, 0);
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
            store
                .save_bulk_lyrics(ids[0], &bulk, false, None, "z", false)
                .await
                .unwrap(),
            BulkSave::Saved
        );
        assert_eq!(
            store.block_lyrics(ids[0], admin, None).await.unwrap(),
            Some("bulk".into())
        );
        assert_eq!(
            store
                .save_bulk_lyrics(ids[0], &bulk, false, None, "z", false)
                .await
                .unwrap(),
            BulkSave::Blocked
        );
        let fixed = NewLyrics {
            plain: Some("Right words"),
            ..words_like(&bulk)
        };
        assert_eq!(
            store
                .save_bulk_lyrics(ids[0], &fixed, false, None, "z", false)
                .await
                .unwrap(),
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
    async fn ai_lyrics_rank_below_a_persons_and_timing_beats_plain() {
        let (store, ids) = store_with(&["One", "Two"]).await;
        let lyr = |synced: Option<&'static str>, plain: &'static str| NewLyrics {
            synced,
            plain: Some(plain),
            instrumental: false,
            source: "bulk",
        };
        let made = AiMark {
            kind: 1,
            model: "faster-whisper/medium",
            match_pct: None,
        };
        let synced_by_ai = AiMark {
            kind: 2,
            model: "faster-whisper/medium",
            match_pct: Some(84),
        };
        let save = |id: i64, l: NewLyrics<'static>, replace: bool, ai: Option<AiMark<'static>>| {
            let store = &store;
            async move {
                store
                    .save_bulk_lyrics(id, &l, replace, ai.as_ref(), "batch.zip", false)
                    .await
                    .unwrap()
            }
        };
        // An AI fills a gap, and says so, with where it came from.
        assert_eq!(
            save(
                ids[0],
                lyr(Some("[00:01.00]guess"), "guess"),
                false,
                Some(made.clone())
            )
            .await,
            BulkSave::Saved
        );
        let got = store.lyrics(ids[0]).await.unwrap().unwrap();
        assert_eq!(
            (got.ai, got.ai_model.as_deref()),
            (1, Some("faster-whisper/medium"))
        );
        assert_eq!(got.import_name.as_deref(), Some("batch.zip"));
        assert_eq!(store.lyrics_counts().await.unwrap().ai, 1);
        // Another AI file doesn't replace it unless told to.
        assert_eq!(
            save(
                ids[0],
                lyr(Some("[00:01.00]guess"), "guess"),
                false,
                Some(made.clone())
            )
            .await,
            BulkSave::Kept
        );
        // A person's plain words replace it, although the AI's are timed.
        assert_eq!(
            save(ids[0], lyr(None, "the real words"), false, None).await,
            BulkSave::Saved
        );
        let got = store.lyrics(ids[0]).await.unwrap().unwrap();
        assert!(got.ai == 0 && got.plain.as_deref() == Some("the real words"));
        // The AI listening never replaces a person's words...
        assert_eq!(
            save(
                ids[0],
                lyr(Some("[00:01.00]guess"), "guess"),
                true,
                Some(made)
            )
            .await,
            BulkSave::Kept
        );
        // ...but the same words timed by an AI do replace plain ones: timing wins.
        assert_eq!(
            save(
                ids[0],
                lyr(Some("[00:01.00]the real words"), "the real words"),
                false,
                Some(synced_by_ai.clone())
            )
            .await,
            BulkSave::Saved
        );
        let got = store.lyrics(ids[0]).await.unwrap().unwrap();
        assert_eq!((got.ai, got.ai_match), (2, Some(84)));
        // A person's timed lyrics beat the AI's timing; plain ones don't.
        assert_eq!(
            save(ids[0], lyr(None, "plain again"), false, None).await,
            BulkSave::Kept
        );
        assert_eq!(
            save(
                ids[0],
                lyr(Some("[00:02.00]by hand"), "by hand"),
                false,
                None
            )
            .await,
            BulkSave::Saved
        );
        assert_eq!(store.lyrics(ids[0]).await.unwrap().unwrap().ai, 0);
        // LRCLIB is still asked about a song with AI-made lyrics (not timed
        // human words), and wins.
        save(
            ids[1],
            lyr(Some("[00:01.00]guess"), "guess"),
            false,
            Some(AiMark {
                kind: 1,
                model: "m",
                match_pct: None,
            }),
        )
        .await;
        assert!(store
            .lyrics_to_look_up(10)
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == ids[1]));
        let found = NewLyrics {
            synced: None,
            plain: Some("from lrclib"),
            instrumental: false,
            source: "lrclib",
        };
        store
            .save_looked_up_lyrics(ids[1], Some(&found))
            .await
            .unwrap();
        let got = store.lyrics(ids[1]).await.unwrap().unwrap();
        assert!((got.source.as_str(), got.ai) == ("lrclib", 0));
        assert_eq!(store.lyrics_counts().await.unwrap().ai, 0);
    }

    #[tokio::test]
    async fn imports_may_replace_lyrics_from_other_sources_when_allowed_and_better() {
        let (store, ids) = store_with(&["One", "Two"]).await;
        let plain_file = NewLyrics {
            synced: None,
            plain: Some("plain from a tag"),
            instrumental: false,
            source: "file",
        };
        let timed_file = NewLyrics {
            synced: Some("[00:01.00]timed from a file"),
            plain: Some("timed from a file"),
            instrumental: false,
            source: "lrc",
        };
        store
            .save_read_lyrics(ids[0], 7, &plain_file)
            .await
            .unwrap();
        store
            .save_read_lyrics(ids[1], 7, &timed_file)
            .await
            .unwrap();
        let timed_import = NewLyrics {
            synced: Some("[00:01.00]timed import"),
            plain: Some("timed import"),
            instrumental: false,
            source: "bulk",
        };
        let sync = AiMark {
            kind: 2,
            model: "m",
            match_pct: Some(90),
        };
        let go = |id: i64, other: bool, ai: Option<AiMark<'static>>| {
            let store = &store;
            let l = NewLyrics {
                ..timed_import_ref(&timed_import)
            };
            async move {
                store
                    .save_bulk_lyrics(id, &l, false, ai.as_ref(), "z.zip", other)
                    .await
                    .unwrap()
            }
        };
        // Not allowed: the tag's lyrics stay.
        assert_eq!(go(ids[0], false, None).await, BulkSave::Kept);
        // Allowed: timed beats the tag's plain words, and the file isn't read over it.
        assert_eq!(go(ids[0], true, None).await, BulkSave::Saved);
        assert_eq!(store.lyrics(ids[0]).await.unwrap().unwrap().source, "bulk");
        assert!(store
            .lyrics_to_read(10)
            .await
            .unwrap()
            .iter()
            .all(|t| t.id != ids[0]));
        // A person's timed lyrics are not replaced by the AI's timing, nor by equal ones.
        assert_eq!(go(ids[1], true, Some(sync.clone())).await, BulkSave::Kept);
        assert_eq!(go(ids[1], true, None).await, BulkSave::Kept);
        assert_eq!(store.lyrics(ids[1]).await.unwrap().unwrap().source, "lrc");
    }

    fn timed_import_ref<'a>(l: &NewLyrics<'a>) -> NewLyrics<'a> {
        NewLyrics {
            synced: l.synced,
            plain: l.plain,
            instrumental: l.instrumental,
            source: l.source,
        }
    }

    #[tokio::test]
    async fn songs_say_whether_an_ai_made_or_timed_their_lyrics() {
        let (store, ids) = store_with(&["One", "Two", "Three"]).await;
        let timed = NewLyrics {
            synced: Some("[00:01.00]la"),
            plain: Some("la"),
            instrumental: false,
            source: "bulk",
        };
        let mark = |kind| AiMark {
            kind,
            model: "m",
            match_pct: None,
        };
        store
            .save_bulk_lyrics(ids[0], &timed, false, Some(&mark(1)), "z", false)
            .await
            .unwrap();
        store
            .save_bulk_lyrics(ids[1], &timed, false, Some(&mark(2)), "z", false)
            .await
            .unwrap();
        store
            .save_bulk_lyrics(ids[2], &timed, false, None, "z", false)
            .await
            .unwrap();
        let mut got = Vec::new();
        for id in &ids {
            let t = store.track(*id).await.unwrap().unwrap();
            got.push((t.lyrics, t.lyrics_ai));
        }
        assert_eq!(got, [(2, 1), (2, 2), (2, 0)]);
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
