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
    /// It has folders of its own (an album folder has none).
    pub has_folders: bool,
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
    pub duration_ms: Option<i64>,
    /// Its lyrics: 0 none, 1 words, 2 timed.
    pub lyrics: i64,
    /// AI lyrics work asked for or under way: "listen" or "sync", then
    /// " asked" or " working" (an import's: " after adding").
    pub ai_job: Option<String>,
}

/// The columns of an [`EditorSong`], over `tracks t` and `albums a`.
macro_rules! editor_columns {
    () => {
        "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_artist, t.track_no,
                t.disc_no, t.year, t.genre, t.album_id,
                coalesce(t.art_hash, a.art_hash, a.online_art_hash) AS art_hash, t.duration_ms,
                coalesce((SELECT CASE WHEN l.synced IS NOT NULL THEN 2
                                      WHEN l.plain IS NOT NULL THEN 1 ELSE 0 END
                          FROM lyrics l WHERE l.track_id = t.id), 0) AS lyrics,
                coalesce((SELECT j.kind || ' working' FROM ai_jobs j
                          WHERE j.track_id = t.id AND j.status IN ('working', 'prepared')),
                         (SELECT q.kind || ' asked' FROM ai_asked q WHERE q.track_id = t.id)) AS ai_job"
    };
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
        let rows: Vec<(Option<String>, i64, bool)> = sqlx::query_as(
            "SELECT CASE WHEN instr(rest, ?1) > 0 THEN substr(rest, 1, instr(rest, ?1) - 1) END AS seg,
                    count(*),
                    max(instr(substr(rest, instr(rest, ?1) + 1), ?1) > 0)
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
        for (seg, count, has_folders) in rows {
            match seg {
                Some(name) => folders.push(FolderChild {
                    name,
                    songs: count,
                    has_folders,
                }),
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
        let rows = sqlx::query_as(concat!(
            editor_columns!(),
            " FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
             WHERE t.removed_at IS NULL AND substr(t.path, 1, ?1) = ?2
             ORDER BY t.path LIMIT ?3 OFFSET ?4"
        ))
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
            editor_columns!(),
            " FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
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

    /// An album's MusicBrainz id (its release group), once looked up.
    pub async fn album_mbid(&self, album_id: i64) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT mbid FROM albums WHERE id = ?")
            .bind(album_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten())
    }

    /// An album's songs: id, title and length.
    pub async fn album_song_titles(
        &self,
        album_id: i64,
    ) -> Result<Vec<(i64, String, Option<i64>)>> {
        Ok(sqlx::query_as(
            "SELECT id, title, duration_ms FROM tracks
             WHERE album_id = ? AND removed_at IS NULL ORDER BY disc_no, track_no, path",
        )
        .bind(album_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// These songs as the editor's grid shows them, by path.
    pub async fn editor_songs_by_id(&self, ids: &[i64]) -> Result<Vec<EditorSong>> {
        let list = format!(
            "[{}]",
            ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
        );
        Ok(sqlx::query_as(concat!(
            editor_columns!(),
            " FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
             WHERE t.removed_at IS NULL AND t.id IN (SELECT value FROM json_each(?1))
             ORDER BY t.path"
        ))
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
        // Album folders have no folders of their own; the others do.
        assert!(root.folders.iter().all(|f| f.has_folders));
        assert!(artist.folders.iter().all(|f| !f.has_folders));
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
