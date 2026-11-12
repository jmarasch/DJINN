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
}

/// Lyrics to save: found (`source` 'lrc', 'file' or 'lrclib') or not ('none').
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
    /// LRCLIB check, so it isn't asked again early.
    pub async fn save_read_lyrics(
        &self,
        track_id: i64,
        file_mtime: i64,
        found: &NewLyrics<'_>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO lyrics (track_id, synced, plain, instrumental, source, file_mtime, updated_at)
             SELECT id, ?, ?, ?, ?, ?, ? FROM tracks WHERE id = ?
             ON CONFLICT (track_id) DO UPDATE SET
               synced = excluded.synced, plain = excluded.plain,
               instrumental = excluded.instrumental, source = excluded.source,
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
                 AS not_looked_up
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
