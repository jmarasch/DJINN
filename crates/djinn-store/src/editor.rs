//! What the library editor's folder tree and grid read: the folders under a
//! folder (worked out from song paths, so it shows the real layout on disk),
//! and the songs under one, with their stored tags.

use serde::Serialize;
use sqlx::FromRow;

use crate::{Result, Store};

/// A folder one level down, with how many songs are in it (any depth).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct FolderChild {
    pub name: String,
    pub songs: i64,
}

/// What a folder holds directly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FolderListing {
    pub folders: Vec<FolderChild>,
    /// Songs directly in the folder, not in a folder under it.
    pub songs_here: i64,
}

/// A song as the editor's grid shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct EditorSong {
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
    pub album_id: Option<i64>,
    pub art_hash: Option<String>,
}

/// The separator a path uses: `\` on Windows, `/` elsewhere.
fn separator(path: &str) -> &'static str {
    if path.contains('\\') && !path.contains('/') {
        "\\"
    } else {
        "/"
    }
}

impl Store {
    /// The folders directly under `folder`, and how many songs sit directly in it.
    pub async fn folder_listing(&self, folder: &str) -> Result<FolderListing> {
        let sep = separator(folder);
        let prefix = format!("{}{sep}", folder.trim_end_matches(['/', '\\']));
        let cut = i64::try_from(prefix.chars().count()).unwrap_or(0);
        let rows: Vec<(Option<String>, i64)> = sqlx::query_as(
            "SELECT CASE WHEN instr(rest, ?1) > 0 THEN substr(rest, 1, instr(rest, ?1) - 1) END AS seg,
                    count(*)
             FROM (SELECT substr(path, ?3 + 1) AS rest FROM tracks
                   WHERE removed_at IS NULL AND substr(path, 1, ?3) = ?2)
             GROUP BY seg ORDER BY lower(seg)",
        )
        .bind(sep)
        .bind(&prefix)
        .bind(cut)
        .fetch_all(&self.pool)
        .await?;
        let mut folders = Vec::new();
        let mut songs_here = 0;
        for (seg, count) in rows {
            match seg {
                Some(name) => folders.push(FolderChild { name, songs: count }),
                None => songs_here = count,
            }
        }
        Ok(FolderListing {
            folders,
            songs_here,
        })
    }

    /// Songs at any depth under `folder`, by path; one page, and the total.
    pub async fn editor_songs(
        &self,
        folder: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<EditorSong>, i64)> {
        let sep = separator(folder);
        let prefix = format!("{}{sep}", folder.trim_end_matches(['/', '\\']));
        let cut = i64::try_from(prefix.chars().count()).unwrap_or(0);
        let rows = sqlx::query_as(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_no,
                    t.disc_no, t.year, t.genre, t.album_id,
                    coalesce(t.art_hash, a.art_hash, a.online_art_hash) AS art_hash
             FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
             WHERE t.removed_at IS NULL AND substr(t.path, 1, ?1) = ?2
             ORDER BY t.path LIMIT ?3 OFFSET ?4",
        )
        .bind(cut)
        .bind(&prefix)
        .bind(limit.clamp(1, 1000))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        let total = sqlx::query_scalar(
            "SELECT count(*) FROM tracks WHERE removed_at IS NULL AND substr(path, 1, ?1) = ?2",
        )
        .bind(cut)
        .bind(&prefix)
        .fetch_one(&self.pool)
        .await?;
        Ok((rows, total))
    }
}

/// At most this many words are looked for at once.
pub const FIND_TERMS: usize = 5;

macro_rules! finds {
    ($n:literal) => {
        concat!(
            "instr(lower(coalesce(t.artist, '') || ' ' || coalesce(t.album_artist, '') || ' ' || t.title || ' ' || coalesce(t.album, '')), lower(?",
            $n,
            ")) > 0"
        )
    };
}

impl Store {
    /// Songs whose artist, album artist, title or album holds any of
    /// `terms` (any case), by artist, album and track; at most `limit`.
    pub async fn editor_find(&self, terms: &[String], limit: i64) -> Result<Vec<EditorSong>> {
        let term = |i: usize| {
            terms
                .get(i)
                .filter(|t| !t.trim().is_empty())
                .map(|t| t.trim().to_owned())
        };
        if (0..FIND_TERMS).all(|i| term(i).is_none()) {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as(concat!(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_no,
                    t.disc_no, t.year, t.genre, t.album_id,
                    coalesce(t.art_hash, a.art_hash, a.online_art_hash) AS art_hash
             FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
             WHERE t.removed_at IS NULL AND (",
            finds!("1"), " OR ", finds!("2"), " OR ", finds!("3"), " OR ", finds!("4"), " OR ", finds!("5"),
            ") ORDER BY lower(coalesce(t.album_artist, t.artist, '')), lower(coalesce(t.album, '')),
                       t.disc_no, t.track_no, t.path
             LIMIT ?6"
        ))
        .bind(term(0))
        .bind(term(1))
        .bind(term(2))
        .bind(term(3))
        .bind(term(4))
        .bind(limit.clamp(1, 2000))
        .fetch_all(&self.pool)
        .await?)
    }

    /// These songs as the editor's grid shows them, by path.
    pub async fn editor_songs_by_id(&self, ids: &[i64]) -> Result<Vec<EditorSong>> {
        let list = format!(
            "[{}]",
            ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
        );
        Ok(sqlx::query_as(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_no,
                    t.disc_no, t.year, t.genre, t.album_id,
                    coalesce(t.art_hash, a.art_hash, a.online_art_hash) AS art_hash
             FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
             WHERE t.removed_at IS NULL AND t.id IN (SELECT value FROM json_each(?1))
             ORDER BY t.path",
        )
        .bind(list)
        .fetch_all(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{ScannedTrack, Store};

    #[tokio::test]
    async fn folders_and_songs_follow_the_paths() {
        let store = Store::open_in_memory().await.unwrap();
        let song = |path: &str| ScannedTrack {
            path: path.into(),
            title: path.rsplit('/').next().unwrap_or("").into(),
            ..Default::default()
        };
        store
            .save_scanned(&[
                song("/pool/A/Artist One/Album x/1.mp3"),
                song("/pool/A/Artist One/Album x/2.mp3"),
                song("/pool/A/Artist One/Album y/1.mp3"),
                song("/pool/B/Artist Two/Album z/1.mp3"),
                song("/pool/loose.mp3"),
                song("/poolside/other.mp3"),
            ])
            .await
            .unwrap();
        let root = store.folder_listing("/pool").await.unwrap();
        assert_eq!(root.songs_here, 1);
        let names: Vec<(&str, i64)> = root
            .folders
            .iter()
            .map(|f| (f.name.as_str(), f.songs))
            .collect();
        assert_eq!(names, [("A", 3), ("B", 1)]);
        let artist = store.folder_listing("/pool/A/Artist One").await.unwrap();
        assert_eq!(artist.folders.len(), 2);
        assert_eq!(artist.songs_here, 0);
        // A folder with a similar name isn't inside it.
        let (songs, total) = store.editor_songs("/pool", 0, 100).await.unwrap();
        assert_eq!((songs.len() as i64, total), (5, 5));
        let (page, _) = store.editor_songs("/pool/A", 1, 1).await.unwrap();
        assert_eq!(page[0].path, "/pool/A/Artist One/Album x/2.mp3");
        // Found by any of the words, in any case.
        let found = store
            .editor_find(&["ALBUM X".into(), "other".into()], 100)
            .await
            .unwrap();
        assert_eq!(
            found.len(),
            1,
            "only the title other.mp3; no album tags here"
        );
        let found = store
            .editor_find(&["loose".into(), "2.MP3".into()], 100)
            .await
            .unwrap();
        assert_eq!(found.len(), 2);
        assert!(store
            .editor_find(&[" ".into()], 100)
            .await
            .unwrap()
            .is_empty());
    }
}
