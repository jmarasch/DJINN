//! What's wrong with the library, for Admin > Library > Problems: files a
//! check couldn't add (`scan_problems`), songs whose tags couldn't be read
//! or that won't play, and gaps in the tags (worked out on read).

use std::collections::HashMap;

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// A kind of problem. The order of [`ProblemKind::ALL`] is the order the
/// page lists them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// A library folder that isn't there (unplugged, offline, renamed).
    MissingFolder,
    /// A folder or file DJINN wasn't allowed to open.
    Unreadable,
    /// A name that isn't valid text, so DJINN can't keep its path.
    BadName,
    /// An audio file in a format DJINN doesn't play.
    Unsupported,
    /// Added under its file name: the tags couldn't be read.
    Tags,
    /// Added, but it failed when played or checked.
    WontPlay,
    /// Listeners said it needs fixing.
    Reported,
    NoArtist,
    NoAlbum,
    AlbumNoCover,
    AlbumNoYear,
    AlbumNoGenre,
}

impl ProblemKind {
    pub const ALL: [Self; 12] = [
        Self::MissingFolder,
        Self::Unreadable,
        Self::BadName,
        Self::Unsupported,
        Self::Tags,
        Self::WontPlay,
        Self::Reported,
        Self::NoArtist,
        Self::NoAlbum,
        Self::AlbumNoCover,
        Self::AlbumNoYear,
        Self::AlbumNoGenre,
    ];

    /// Kinds a library check finds while walking the folders. Each check
    /// replaces all of them.
    const WALKED: [Self; 4] = [
        Self::MissingFolder,
        Self::Unreadable,
        Self::BadName,
        Self::Unsupported,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingFolder => "missing_folder",
            Self::Unreadable => "unreadable",
            Self::BadName => "bad_name",
            Self::Unsupported => "unsupported",
            Self::Tags => "tags",
            Self::WontPlay => "wont_play",
            Self::Reported => "reported",
            Self::NoArtist => "no_artist",
            Self::NoAlbum => "no_album",
            Self::AlbumNoCover => "album_no_cover",
            Self::AlbumNoYear => "album_no_year",
            Self::AlbumNoGenre => "album_no_genre",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// Files that never made it into the library, or songs that are there
    /// but broken: the ones worth a badge. Tag gaps are only listed.
    pub fn is_trouble(self) -> bool {
        !matches!(
            self,
            Self::NoArtist
                | Self::NoAlbum
                | Self::AlbumNoCover
                | Self::AlbumNoYear
                | Self::AlbumNoGenre
        )
    }

    /// One row per album rather than per song.
    pub fn per_album(self) -> bool {
        matches!(
            self,
            Self::AlbumNoCover | Self::AlbumNoYear | Self::AlbumNoGenre
        )
    }
}

/// Something a library check found while walking the folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanProblem {
    pub path: String,
    pub kind: ProblemKind,
    pub detail: String,
}

/// One row on the Problems page. Songs have `track_id`; album rows have
/// `album_id` and the path of one of their songs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, FromRow)]
pub struct ProblemItem {
    pub path: Option<String>,
    pub detail: String,
    pub track_id: Option<i64>,
    pub album_id: Option<i64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub noticed_at: Option<i64>,
}

macro_rules! walked_from {
    () => {
        "FROM scan_problems sp WHERE sp.kind = ?"
    };
}
macro_rules! tags_from {
    () => {
        "FROM scan_problems sp JOIN tracks t ON t.path = sp.path AND t.removed_at IS NULL
         WHERE sp.kind = 'tags'"
    };
}
macro_rules! wont_play_from {
    () => {
        "FROM track_problems tp JOIN tracks t ON t.id = tp.track_id AND t.removed_at IS NULL"
    };
}
macro_rules! reported_from {
    () => {
        "FROM song_reports r JOIN tracks t ON t.id = r.track_id AND t.removed_at IS NULL"
    };
}
macro_rules! no_artist_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NULL AND (t.artist IS NULL OR trim(t.artist) = '')"
    };
}
macro_rules! no_album_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NULL AND (t.album IS NULL OR trim(t.album) = '')"
    };
}
macro_rules! album_from {
    () => {
        "FROM albums a JOIN artists ar ON ar.id = a.artist_id
         WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id AND t.removed_at IS NULL)"
    };
}
macro_rules! album_no_cover {
    () => {
        " AND a.art_hash IS NULL AND a.online_art_hash IS NULL"
    };
}
macro_rules! album_no_year {
    () => {
        " AND a.year IS NULL"
    };
}
macro_rules! album_no_genre {
    () => {
        " AND (a.genres IS NULL OR a.genres = '' OR a.genres = '[]')
         AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id
             AND t.removed_at IS NULL AND trim(coalesce(t.genre, '')) <> '')"
    };
}
macro_rules! song_columns {
    ($detail:literal, $noticed:literal) => {
        concat!(
            "SELECT t.path, ",
            $detail,
            " AS detail, t.id AS track_id, t.album_id, t.title, t.artist, t.album, ",
            $noticed,
            " AS noticed_at "
        )
    };
}
macro_rules! album_columns {
    () => {
        "SELECT (SELECT t.path FROM tracks t WHERE t.album_id = a.id AND t.removed_at IS NULL
                 ORDER BY t.disc_no, t.track_no LIMIT 1) AS path,
                '' AS detail, NULL AS track_id, a.id AS album_id, NULL AS title,
                ar.name AS artist, a.title AS album, NULL AS noticed_at "
    };
}

/// Why a listener says a song needs fixing.
pub const REPORT_REASONS: [&str; 5] = [
    "wrong_info",
    "wrong_cover",
    "sounds_bad",
    "wrong_song",
    "other",
];

impl Store {
    /// Records a listener's report on a song, replacing their last one.
    /// False if the song isn't in the library.
    pub async fn report_song(
        &self,
        track_id: i64,
        user_id: i64,
        reason: &str,
        note: Option<&str>,
    ) -> Result<bool> {
        let done = sqlx::query(
            "INSERT OR REPLACE INTO song_reports (track_id, user_id, reason, note, created_at)
             SELECT id, ?, ?, ?, ? FROM tracks WHERE id = ? AND removed_at IS NULL",
        )
        .bind(user_id)
        .bind(reason)
        .bind(note)
        .bind(now_ms())
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Songs added to the library and songs gone from it since `since`
    /// (Unix ms).
    pub async fn library_changes_since(&self, since: i64) -> Result<(i64, i64)> {
        Ok(sqlx::query_as(
            "SELECT count(*) FILTER (WHERE added_at >= ?1 AND removed_at IS NULL),
                    count(*) FILTER (WHERE removed_at >= ?1)
             FROM tracks",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Clears every report on a song (it's been fixed). How many there were.
    pub async fn clear_song_reports(&self, track_id: i64) -> Result<u64> {
        let done = sqlx::query("DELETE FROM song_reports WHERE track_id = ?")
            .bind(track_id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// Replaces what the last check found while walking the folders. A
    /// problem seen before keeps the time it was first noticed. Also drops
    /// unreadable-tag notes of songs no longer in the library.
    pub async fn replace_walk_problems(&self, problems: &[ScanProblem]) -> Result<()> {
        let mut tx = self.write().await?;
        let seen: Vec<(String, i64)> = sqlx::query_as(
            "SELECT path, noticed_at FROM scan_problems
             WHERE kind IN ('missing_folder', 'unreadable', 'bad_name', 'unsupported')",
        )
        .fetch_all(&mut *tx)
        .await?;
        let seen: HashMap<String, i64> = seen.into_iter().collect();
        for kind in ProblemKind::WALKED {
            sqlx::query("DELETE FROM scan_problems WHERE kind = ?")
                .bind(kind.as_str())
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "DELETE FROM scan_problems WHERE kind = 'tags' AND NOT EXISTS
             (SELECT 1 FROM tracks t WHERE t.path = scan_problems.path AND t.removed_at IS NULL)",
        )
        .execute(&mut *tx)
        .await?;
        let now = now_ms();
        for problem in problems {
            sqlx::query(
                "INSERT OR REPLACE INTO scan_problems (path, kind, detail, noticed_at)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(&problem.path)
            .bind(problem.kind.as_str())
            .bind(&problem.detail)
            .bind(seen.get(&problem.path).copied().unwrap_or(now))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// How many of each kind there are now, in page order.
    pub async fn problem_counts(&self) -> Result<Vec<(ProblemKind, i64)>> {
        let mut counts = Vec::with_capacity(ProblemKind::ALL.len());
        for kind in ProblemKind::ALL {
            counts.push((kind, self.problem_count(kind).await?));
        }
        Ok(counts)
    }

    async fn problem_count(&self, kind: ProblemKind) -> Result<i64> {
        use ProblemKind as K;
        let query = match kind {
            K::MissingFolder | K::Unreadable | K::BadName | K::Unsupported => {
                sqlx::query_scalar(concat!("SELECT COUNT(*) ", walked_from!())).bind(kind.as_str())
            }
            K::Tags => sqlx::query_scalar(concat!("SELECT COUNT(*) ", tags_from!())),
            K::WontPlay => sqlx::query_scalar(concat!("SELECT COUNT(*) ", wont_play_from!())),
            K::Reported => {
                sqlx::query_scalar(concat!("SELECT COUNT(DISTINCT t.id) ", reported_from!()))
            }
            K::NoArtist => sqlx::query_scalar(concat!("SELECT COUNT(*) ", no_artist_from!())),
            K::NoAlbum => sqlx::query_scalar(concat!("SELECT COUNT(*) ", no_album_from!())),
            K::AlbumNoCover => sqlx::query_scalar(concat!(
                "SELECT COUNT(*) ",
                album_from!(),
                album_no_cover!()
            )),
            K::AlbumNoYear => {
                sqlx::query_scalar(concat!("SELECT COUNT(*) ", album_from!(), album_no_year!()))
            }
            K::AlbumNoGenre => sqlx::query_scalar(concat!(
                "SELECT COUNT(*) ",
                album_from!(),
                album_no_genre!()
            )),
        };
        Ok(query.fetch_one(&self.pool).await?)
    }

    /// A page of one kind, ordered by path (albums by artist and title).
    pub async fn problems(
        &self,
        kind: ProblemKind,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<ProblemItem>> {
        use ProblemKind as K;
        let query = match kind {
            K::MissingFolder | K::Unreadable | K::BadName | K::Unsupported => {
                sqlx::query_as(concat!(
                    "SELECT sp.path, sp.detail, NULL AS track_id, NULL AS album_id, NULL AS title,
                     NULL AS artist, NULL AS album, sp.noticed_at ",
                    walked_from!(),
                    " ORDER BY sp.path LIMIT ? OFFSET ?"
                ))
                .bind(kind.as_str())
            }
            K::Tags => sqlx::query_as(concat!(
                song_columns!("sp.detail", "sp.noticed_at"),
                tags_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::WontPlay => sqlx::query_as(concat!(
                song_columns!("tp.error", "tp.noticed_at"),
                wont_play_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            // Each report as reason, unit separator, note; record separators
            // between reports.
            K::Reported => sqlx::query_as(concat!(
                "SELECT t.path,
                        group_concat(r.reason || char(31) || coalesce(r.note, ''), char(30)) AS detail,
                        t.id AS track_id, t.album_id, t.title, t.artist, t.album,
                        max(r.created_at) AS noticed_at ",
                reported_from!(),
                " GROUP BY t.id ORDER BY noticed_at DESC LIMIT ? OFFSET ?"
            )),
            K::NoArtist => sqlx::query_as(concat!(
                song_columns!("''", "NULL"),
                no_artist_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::NoAlbum => sqlx::query_as(concat!(
                song_columns!("''", "NULL"),
                no_album_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::AlbumNoCover => sqlx::query_as(concat!(
                album_columns!(),
                album_from!(),
                album_no_cover!(),
                " ORDER BY ar.name, a.title LIMIT ? OFFSET ?"
            )),
            K::AlbumNoYear => sqlx::query_as(concat!(
                album_columns!(),
                album_from!(),
                album_no_year!(),
                " ORDER BY ar.name, a.title LIMIT ? OFFSET ?"
            )),
            K::AlbumNoGenre => sqlx::query_as(concat!(
                album_columns!(),
                album_from!(),
                album_no_genre!(),
                " ORDER BY ar.name, a.title LIMIT ? OFFSET ?"
            )),
        };
        Ok(query
            .bind(limit.max(0))
            .bind(offset.max(0))
            .fetch_all(&self.pool)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScannedTrack;

    fn song(path: &str, title: &str) -> ScannedTrack {
        ScannedTrack {
            path: path.into(),
            title: title.into(),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            year: Some(1999),
            genre: Some("Rock".into()),
            embedded_art: Some("abc".into()),
            ..Default::default()
        }
    }

    fn count(counts: &[(ProblemKind, i64)], kind: ProblemKind) -> i64 {
        counts
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(0, |(_, n)| *n)
    }

    #[tokio::test]
    async fn walked_problems_are_replaced_and_keep_when_first_seen() {
        let store = Store::open_in_memory().await.unwrap();
        let wma = ScanProblem {
            path: "/m/a.wma".into(),
            kind: ProblemKind::Unsupported,
            detail: "wma".into(),
        };
        store
            .replace_walk_problems(std::slice::from_ref(&wma))
            .await
            .unwrap();
        let first = store
            .problems(ProblemKind::Unsupported, 0, 10)
            .await
            .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].path.as_deref(), Some("/m/a.wma"));

        let opus = ScanProblem {
            path: "/m/b.opus".into(),
            kind: ProblemKind::Unsupported,
            detail: "opus".into(),
        };
        store.replace_walk_problems(&[wma, opus]).await.unwrap();
        let second = store
            .problems(ProblemKind::Unsupported, 0, 10)
            .await
            .unwrap();
        assert_eq!(second.len(), 2);
        assert_eq!(second[0].noticed_at, first[0].noticed_at);

        store.replace_walk_problems(&[]).await.unwrap();
        let counts = store.problem_counts().await.unwrap();
        assert_eq!(count(&counts, ProblemKind::Unsupported), 0);
    }

    #[tokio::test]
    async fn unreadable_tags_follow_the_song() {
        let store = Store::open_in_memory().await.unwrap();
        let mut broken = song("/m/x.mp3", "x");
        broken.tag_problem = Some("bad frame".into());
        store.save_scanned(&[broken]).await.unwrap();
        let items = store.problems(ProblemKind::Tags, 0, 10).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail, "bad frame");
        assert!(items[0].track_id.is_some());

        // Read again with good tags: the note goes.
        store.save_scanned(&[song("/m/x.mp3", "X")]).await.unwrap();
        let counts = store.problem_counts().await.unwrap();
        assert_eq!(count(&counts, ProblemKind::Tags), 0);
    }

    #[tokio::test]
    async fn tag_gaps_are_counted_per_song_and_per_album() {
        let store = Store::open_in_memory().await.unwrap();
        let mut bare = song("/m/bare/1.flac", "Bare");
        bare.artist = None;
        bare.album = Some("Bare".into());
        bare.year = None;
        bare.genre = None;
        bare.embedded_art = None;
        let mut lone = song("/m/lone.flac", "Lone");
        lone.album = None;
        store
            .save_scanned(&[song("/m/ok.flac", "Ok"), bare, lone])
            .await
            .unwrap();
        store.refresh_artwork().await.unwrap();
        let counts = store.problem_counts().await.unwrap();
        assert_eq!(count(&counts, ProblemKind::NoArtist), 1);
        assert_eq!(count(&counts, ProblemKind::NoAlbum), 1);
        assert_eq!(count(&counts, ProblemKind::AlbumNoCover), 1);
        assert_eq!(count(&counts, ProblemKind::AlbumNoYear), 1);
        assert_eq!(count(&counts, ProblemKind::AlbumNoGenre), 1);

        let albums = store
            .problems(ProblemKind::AlbumNoCover, 0, 10)
            .await
            .unwrap();
        assert_eq!(albums[0].album.as_deref(), Some("Bare"));
        assert_eq!(albums[0].path.as_deref(), Some("/m/bare/1.flac"));
    }

    #[tokio::test]
    async fn reports_group_by_song_and_clear() {
        let store = Store::open_in_memory().await.unwrap();
        store.save_scanned(&[song("/m/r.mp3", "R")]).await.unwrap();
        let id = store.library_track_ids().await.unwrap()[0];
        let ann = store
            .create_user("ann", "h", crate::Role::User)
            .await
            .unwrap();
        let bo = store
            .create_user("bo", "h", crate::Role::Guest)
            .await
            .unwrap();
        assert!(store
            .report_song(id, ann.id, "wrong_cover", None)
            .await
            .unwrap());
        assert!(store
            .report_song(id, bo.id, "sounds_bad", Some("skips"))
            .await
            .unwrap());
        // Reporting again replaces your report.
        assert!(store
            .report_song(id, ann.id, "wrong_info", None)
            .await
            .unwrap());
        assert!(!store
            .report_song(9999, ann.id, "other", None)
            .await
            .unwrap());

        let items = store.problems(ProblemKind::Reported, 0, 10).await.unwrap();
        assert_eq!(items.len(), 1);
        let mut reports: Vec<&str> = items[0].detail.split('\u{1e}').collect();
        reports.sort_unstable();
        assert_eq!(reports, ["sounds_bad\u{1f}skips", "wrong_info\u{1f}"]);

        assert_eq!(store.clear_song_reports(id).await.unwrap(), 2);
        let counts = store.problem_counts().await.unwrap();
        assert_eq!(count(&counts, ProblemKind::Reported), 0);
    }

    #[test]
    fn kinds_round_trip() {
        for kind in ProblemKind::ALL {
            assert_eq!(ProblemKind::parse(kind.as_str()), Some(kind));
        }
    }
}
