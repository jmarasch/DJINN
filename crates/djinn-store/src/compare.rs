//! What the library editor's compare screen needs from the database: each
//! song's stored tags, and which songs had a gap filled in by an applied
//! identification.

use std::collections::HashSet;

use sqlx::FromRow;

use crate::{Result, Store};

/// A song's tags as the database holds them.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct CompareRow {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
}

impl Store {
    /// Every song that is in the library now, by path.
    pub async fn compare_rows(&self) -> Result<Vec<CompareRow>> {
        Ok(sqlx::query_as(
            "SELECT id, path, title, artist, album, album_artist, track_no, disc_no, year, genre
             FROM tracks WHERE removed_at IS NULL ORDER BY path",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Paths of songs with an applied identification (it fills what the file
    /// lacks).
    pub async fn identified_paths(&self) -> Result<HashSet<String>> {
        let paths: Vec<String> = sqlx::query_scalar(
            "SELECT t.path FROM track_ids ti JOIN tracks t ON t.id = ti.track_id
             WHERE ti.status = 'applied'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(paths.into_iter().collect())
    }
}
