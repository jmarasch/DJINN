//! Tag corrections made in the app. Files are never changed: the scanner
//! applies these over a file's tags each time it reads the file.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// One song's corrections. `None` keeps the file's value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, FromRow)]
pub struct TagEdit {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
}

/// A change to songs' corrections. For each field, `None` leaves it as it
/// is, `Some(None)` goes back to the file's value, and `Some(Some(v))`
/// sets it.
#[derive(Clone, Debug, Default)]
pub struct TagChange {
    pub title: Option<Option<String>>,
    pub artist: Option<Option<String>>,
    pub album: Option<Option<String>>,
    pub album_artist: Option<Option<String>>,
    pub year: Option<Option<i64>>,
    pub genre: Option<Option<String>>,
}

impl TagChange {
    fn apply(&self, edit: &mut TagEdit) {
        fn set<T: Clone>(field: &mut Option<T>, change: &Option<Option<T>>) {
            if let Some(value) = change {
                field.clone_from(value);
            }
        }
        set(&mut edit.title, &self.title);
        set(&mut edit.artist, &self.artist);
        set(&mut edit.album, &self.album);
        set(&mut edit.album_artist, &self.album_artist);
        set(&mut edit.year, &self.year);
        set(&mut edit.genre, &self.genre);
    }
}

/// A song an artist edit touches: one they're credited on, or on one of
/// their albums.
#[derive(Clone, Debug, FromRow)]
pub struct ArtistSong {
    pub id: i64,
    /// The artist credit and album artist as the song reads now.
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    /// They lead the song rather than only being featured.
    pub leads: bool,
}

#[derive(FromRow)]
struct PathEdit {
    path: String,
    #[sqlx(flatten)]
    edit: TagEdit,
}

impl Store {
    /// Every song's corrections, by file path, for the scanner.
    pub async fn tag_edits_by_path(&self) -> Result<HashMap<String, TagEdit>> {
        let rows: Vec<PathEdit> = sqlx::query_as(
            "SELECT t.path, e.title, e.artist, e.album, e.album_artist, e.year, e.genre \
             FROM tag_edits e JOIN tracks t ON t.id = e.track_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.path, r.edit)).collect())
    }

    /// A song's corrections; all `None` when it has none.
    pub async fn tag_edit(&self, track_id: i64) -> Result<TagEdit> {
        let edit = sqlx::query_as(
            "SELECT title, artist, album, album_artist, year, genre FROM tag_edits WHERE track_id = ?",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(edit.unwrap_or_default())
    }

    /// The songs an artist is on, or whose album is theirs.
    pub async fn artist_songs(&self, artist_id: i64) -> Result<Vec<ArtistSong>> {
        let songs = sqlx::query_as(
            "SELECT t.id, t.artist, t.album_artist, \
                    EXISTS (SELECT 1 FROM track_artists ta \
                            WHERE ta.track_id = t.id AND ta.artist_id = ?1 AND NOT ta.featured) AS leads \
             FROM tracks t LEFT JOIN albums al ON al.id = t.album_id \
             WHERE t.removed_at IS NULL \
               AND (al.artist_id = ?1 OR EXISTS (SELECT 1 FROM track_artists ta \
                                                 WHERE ta.track_id = t.id AND ta.artist_id = ?1))",
        )
        .bind(artist_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(songs)
    }

    /// Renames an artist in place, keeping their photo, bio and looked-up
    /// details, unless another artist has the name already. Their songs'
    /// corrections should follow, or the next read of the files undoes it.
    pub async fn rename_artist(&self, id: i64, name: &str) -> Result<()> {
        sqlx::query(
            "UPDATE artists SET name = ?1 WHERE id = ?2 \
               AND NOT EXISTS (SELECT 1 FROM artists WHERE name = ?1 AND id != ?2)",
        )
        .bind(name)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The artist with this name (any case), if there is one.
    pub async fn artist_named(&self, name: &str) -> Result<Option<i64>> {
        let id = sqlx::query_scalar("SELECT id FROM artists WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(id)
    }

    /// Applies `change` to each song's corrections. Returns the songs' file
    /// paths, to read again so the change shows.
    pub async fn edit_tags(&self, track_ids: &[i64], change: &TagChange) -> Result<Vec<String>> {
        let mut tx = self.pool.begin().await?;
        let now = now_ms();
        let mut paths = Vec::with_capacity(track_ids.len());
        for &id in track_ids {
            let path: Option<String> = sqlx::query_scalar("SELECT path FROM tracks WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some(path) = path else { continue };
            let mut edit: TagEdit = sqlx::query_as(
                "SELECT title, artist, album, album_artist, year, genre FROM tag_edits WHERE track_id = ?",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or_default();
            change.apply(&mut edit);
            if edit == TagEdit::default() {
                sqlx::query("DELETE FROM tag_edits WHERE track_id = ?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query(
                    "INSERT INTO tag_edits (track_id, title, artist, album, album_artist, year, genre, edited_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                     ON CONFLICT (track_id) DO UPDATE SET title = excluded.title, \
                        artist = excluded.artist, album = excluded.album, \
                        album_artist = excluded.album_artist, year = excluded.year, \
                        genre = excluded.genre, edited_at = excluded.edited_at",
                )
                .bind(id)
                .bind(&edit.title)
                .bind(&edit.artist)
                .bind(&edit.album)
                .bind(&edit.album_artist)
                .bind(edit.year)
                .bind(&edit.genre)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
            paths.push(path);
        }
        tx.commit().await?;
        Ok(paths)
    }

    /// "Sounds like" genres taken off songs, as (track id, genre).
    pub async fn removed_styles(&self) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query_as("SELECT track_id, genre FROM removed_styles")
            .fetch_all(self.pool())
            .await?;
        Ok(rows)
    }

    /// Takes a "Sounds like" genre off a song, or (`removed` false) puts
    /// it back. False if there's no such song.
    pub async fn set_style_removed(
        &self,
        track_id: i64,
        genre: &str,
        removed: bool,
    ) -> Result<bool> {
        if removed {
            let added = sqlx::query(
                "INSERT INTO removed_styles (track_id, genre) \
                 SELECT id, ? FROM tracks WHERE id = ? ON CONFLICT DO NOTHING",
            )
            .bind(genre)
            .bind(track_id)
            .execute(self.pool())
            .await?;
            if added.rows_affected() > 0 {
                return Ok(true);
            }
        } else {
            sqlx::query("DELETE FROM removed_styles WHERE track_id = ? AND genre = ?")
                .bind(track_id)
                .bind(genre)
                .execute(self.pool())
                .await?;
        }
        let exists = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ?)")
            .bind(track_id)
            .fetch_one(self.pool())
            .await?;
        Ok(exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScannedTrack;

    async fn store_with_song() -> (Store, i64) {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[ScannedTrack {
                path: "/music/a.flac".into(),
                title: "Untitled".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = store.library_track_ids().await.unwrap()[0];
        (store, id)
    }

    #[tokio::test]
    async fn edits_set_keep_and_revert_fields() {
        let (store, id) = store_with_song().await;
        let paths = store
            .edit_tags(
                &[id, 999],
                &TagChange {
                    title: Some(Some("Lush Life".into())),
                    year: Some(Some(2015)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(paths, ["/music/a.flac"], "unknown songs are skipped");

        // Only the year goes back to the file's; the title stays.
        store
            .edit_tags(
                &[id],
                &TagChange {
                    year: Some(None),
                    genre: None,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let edit = store.tag_edit(id).await.unwrap();
        assert_eq!(edit.title.as_deref(), Some("Lush Life"));
        assert_eq!(edit.year, None);
        let by_path = store.tag_edits_by_path().await.unwrap();
        assert_eq!(by_path["/music/a.flac"], edit);

        // Nothing left to correct: the row goes.
        store
            .edit_tags(
                &[id],
                &TagChange {
                    title: Some(None),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(store.tag_edits_by_path().await.unwrap().is_empty());
        assert_eq!(store.tag_edit(id).await.unwrap(), TagEdit::default());
    }

    #[tokio::test]
    async fn styles_come_off_and_back() {
        let (store, id) = store_with_song().await;
        assert!(store.set_style_removed(id, "trap", true).await.unwrap());
        assert!(
            store.set_style_removed(id, "trap", true).await.unwrap(),
            "twice is fine"
        );
        assert!(store.set_style_removed(id, "lo fi", true).await.unwrap());
        assert!(
            !store.set_style_removed(999, "trap", true).await.unwrap(),
            "no such song"
        );
        let mut removed = store.removed_styles().await.unwrap();
        removed.sort();
        assert_eq!(removed, [(id, "lo fi".to_owned()), (id, "trap".to_owned())]);

        assert!(store.set_style_removed(id, "trap", false).await.unwrap());
        assert_eq!(
            store.removed_styles().await.unwrap(),
            [(id, "lo fi".to_owned())]
        );
    }
}
