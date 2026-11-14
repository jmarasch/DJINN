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
    /// Listeners said its lyrics are wrong or bad.
    ReportedLyrics,
    NoArtist,
    NoAlbum,
    AlbumNoCover,
    AlbumNoYear,
    AlbumNoGenre,
    /// The same album filed twice (a copy in another folder, or under
    /// another name).
    DuplicateAlbums,
    /// The same file twice in one album.
    IdenticalFiles,
    /// A track or disc number of 0 or less, or a length of 0.
    BogusValues,
    /// Under 10 seconds or over 3 hours long.
    OddLength,
    /// A bit rate under 96 kbps.
    LowBitrate,
    /// "(Official Video)", a track number or a web address in the title, ...
    TitleJunk,
    /// The file's name doesn't contain the song's title.
    NameMismatch,
    /// Two names for one artist ("The Beatles" and "Beatles").
    ArtistVariants,
    /// An album's folder holds it under two artists' names.
    SplitAcrossArtists,
    /// Songs of one album with different covers.
    CoverDiffers,
    /// Missing or repeated track numbers.
    TrackGaps,
    /// No lyrics found for the song.
    NoLyrics,
    /// No ReplayGain tags and no measurement, in a library that uses it.
    NoLoudness,
    /// Songs that went missing from the library in the last 30 days.
    Vanished,
}

impl ProblemKind {
    pub const ALL: [Self; 27] = [
        Self::MissingFolder,
        Self::Unreadable,
        Self::BadName,
        Self::Unsupported,
        Self::Tags,
        Self::WontPlay,
        Self::Reported,
        Self::ReportedLyrics,
        Self::NoArtist,
        Self::NoAlbum,
        Self::AlbumNoCover,
        Self::AlbumNoYear,
        Self::AlbumNoGenre,
        Self::DuplicateAlbums,
        Self::IdenticalFiles,
        Self::BogusValues,
        Self::OddLength,
        Self::LowBitrate,
        Self::TitleJunk,
        Self::NameMismatch,
        Self::ArtistVariants,
        Self::SplitAcrossArtists,
        Self::CoverDiffers,
        Self::TrackGaps,
        Self::NoLyrics,
        Self::NoLoudness,
        Self::Vanished,
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
            Self::ReportedLyrics => "reported_lyrics",
            Self::NoArtist => "no_artist",
            Self::NoAlbum => "no_album",
            Self::AlbumNoCover => "album_no_cover",
            Self::AlbumNoYear => "album_no_year",
            Self::AlbumNoGenre => "album_no_genre",
            Self::DuplicateAlbums => "duplicate_albums",
            Self::IdenticalFiles => "identical_files",
            Self::BogusValues => "bogus_values",
            Self::OddLength => "odd_length",
            Self::LowBitrate => "low_bitrate",
            Self::TitleJunk => "title_junk",
            Self::NameMismatch => "name_mismatch",
            Self::ArtistVariants => "artist_variants",
            Self::SplitAcrossArtists => "split_across_artists",
            Self::CoverDiffers => "cover_differs",
            Self::TrackGaps => "track_gaps",
            Self::NoLyrics => "no_lyrics",
            Self::NoLoudness => "no_loudness",
            Self::Vanished => "vanished",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// Files that never made it into the library, or songs that are there
    /// but broken: the ones worth a badge. Tag gaps are only listed.
    pub fn is_trouble(self) -> bool {
        matches!(
            self,
            Self::MissingFolder
                | Self::Unreadable
                | Self::BadName
                | Self::Unsupported
                | Self::Tags
                | Self::WontPlay
                | Self::Reported
                | Self::ReportedLyrics
                | Self::DuplicateAlbums
                | Self::IdenticalFiles
        )
    }

    /// One row per album rather than per song.
    pub fn per_album(self) -> bool {
        matches!(
            self,
            Self::AlbumNoCover
                | Self::AlbumNoYear
                | Self::AlbumNoGenre
                | Self::DuplicateAlbums
                | Self::SplitAcrossArtists
                | Self::CoverDiffers
                | Self::TrackGaps
        )
    }
}

/// Two albums that are the same album: how many songs they share and hold.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct AlbumTwin {
    /// The smaller album id.
    pub a: i64,
    pub b: i64,
    pub shared: i64,
    pub a_songs: i64,
    pub b_songs: i64,
}

/// An artist's name with capitals, "The", "&", spaces and punctuation taken
/// out, so spellings of one name meet.
fn artist_key(name: &str) -> String {
    let lower = name.to_lowercase().replace('&', "and");
    let lower = lower.strip_prefix("the ").unwrap_or(&lower);
    lower.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Songs that went missing after this time (Unix ms) are listed as gone.
fn vanished_since() -> i64 {
    now_ms() - 30 * 24 * 60 * 60 * 1000
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
        "FROM song_reports r JOIN tracks t ON t.id = r.track_id AND t.removed_at IS NULL
         WHERE substr(r.reason, 1, 7) != 'lyrics_'"
    };
}
macro_rules! reported_lyrics_from {
    () => {
        "FROM song_reports r JOIN tracks t ON t.id = r.track_id AND t.removed_at IS NULL
         WHERE substr(r.reason, 1, 7) = 'lyrics_'"
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
/// Songs listed only once the same file (size, length and title) sits twice
/// in one album.
macro_rules! identical_from {
    () => {
        "FROM tracks t JOIN (
             SELECT album_id, size, duration_ms, lower(title) AS lt FROM tracks
             WHERE removed_at IS NULL GROUP BY album_id, size, duration_ms, lt HAVING count(*) > 1
         ) d ON d.album_id IS t.album_id AND d.size = t.size AND d.duration_ms IS t.duration_ms
             AND d.lt = lower(t.title)
         WHERE t.removed_at IS NULL"
    };
}
macro_rules! bogus_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NULL
           AND (t.track_no <= 0 OR t.disc_no <= 0 OR t.duration_ms <= 0)"
    };
}
macro_rules! bogus_detail {
    () => {
        "CASE WHEN t.track_no <= 0 THEN 'Track number ' || t.track_no
              WHEN t.disc_no <= 0 THEN 'Disc number ' || t.disc_no
              ELSE 'Length of ' || t.duration_ms || ' ms' END"
    };
}
macro_rules! odd_length_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NULL AND t.duration_ms > 0
           AND (t.duration_ms < 10000 OR t.duration_ms > 10800000)"
    };
}
macro_rules! odd_length_detail {
    () => {
        "CASE WHEN t.duration_ms < 10000 THEN 'Only ' || (t.duration_ms / 1000) || ' seconds long'
              ELSE (t.duration_ms / 60000) || ' minutes long' END"
    };
}
macro_rules! low_bitrate_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NULL AND t.duration_ms >= 10000
           AND t.size * 8 / t.duration_ms < 96"
    };
}
macro_rules! low_bitrate_detail {
    () => {
        "(t.size * 8 / t.duration_ms) || ' kbps'"
    };
}
/// Titles with leftovers: a video tag, a web address, a track number in
/// front, spaces around, or all capitals.
macro_rules! junk_where {
    () => {
        "(lower(t.title) LIKE '%(official%' OR lower(t.title) LIKE '%[official%'
          OR lower(t.title) LIKE '%(lyric%' OR lower(t.title) LIKE '%[lyric%'
          OR lower(t.title) LIKE '%(audio)%' OR lower(t.title) LIKE '%(video)%'
          OR lower(t.title) LIKE '%www.%' OR lower(t.title) LIKE '%.com%'
          OR t.title GLOB '[0-9][0-9]. *' OR t.title GLOB '[0-9][0-9] - *'
          OR t.title GLOB '[0-9][0-9]-*' OR t.title GLOB '[0-9][0-9]_*'
          OR t.title <> trim(t.title)
          OR (length(t.title) > 8 AND instr(t.title, ' ') > 0
              AND t.title = upper(t.title) AND t.title <> lower(t.title)))"
    };
}
macro_rules! title_junk_from {
    () => {
        concat!(
            "FROM tracks t WHERE t.removed_at IS NULL AND ",
            junk_where!()
        )
    };
}
macro_rules! title_junk_detail {
    () => {
        "CASE WHEN t.title <> trim(t.title) THEN 'Spaces around the title'
              WHEN t.title GLOB '[0-9][0-9][.-_]*' THEN 'A track number in the title'
              WHEN lower(t.title) LIKE '%www.%' OR lower(t.title) LIKE '%.com%' THEN 'A web address in the title'
              WHEN t.title = upper(t.title) THEN 'All capitals'
              ELSE 'A video or lyrics tag in the title' END"
    };
}
/// An expression with what file names change (spaces, dashes, colons, slashes,
/// question marks, quotes, dots, ...) taken out, so "11:00am" meets "11-00am".
macro_rules! plain {
    ($e:literal) => {
        concat!(
            "replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(",
            "replace(replace(replace(replace(replace(replace(replace(", $e,
            ", ' ', ''), '-', ''), '_', ''), ':', ''), '/', ''), '?', ''), '*', ''), '\"', ''), ",
            "'<', ''), '>', ''), '|', ''), '''', ''), '.', ''), ',', ''), '!', ''), '’', ''), '&', '')"
        )
    };
}
macro_rules! name_mismatch_from {
    () => {
        concat!(
            "FROM tracks t WHERE t.removed_at IS NULL AND length(trim(t.title)) > 1
               AND instr(",
            plain!("lower(t.path)"),
            ", ",
            plain!("lower(t.title)"),
            ") = 0"
        )
    };
}
macro_rules! no_lyrics_from {
    () => {
        "FROM tracks t LEFT JOIN lyrics l ON l.track_id = t.id
         WHERE t.removed_at IS NULL AND (l.track_id IS NULL OR l.source = 'none')"
    };
}
macro_rules! no_loudness_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NULL
           AND t.rg_track_gain IS NULL AND t.loudness_lufs IS NULL
           AND (SELECT 1 FROM tracks o
                WHERE o.rg_track_gain IS NOT NULL OR o.loudness_lufs IS NOT NULL LIMIT 1) IS NOT NULL"
    };
}
macro_rules! vanished_from {
    () => {
        "FROM tracks t WHERE t.removed_at IS NOT NULL AND t.removed_at > ?
           AND NOT EXISTS (SELECT 1 FROM tracks a WHERE a.removed_at IS NULL
                           AND a.size = t.size AND lower(a.title) = lower(t.title))"
    };
}
/// A folder (path without the file name), same for `/` and `\` paths.
macro_rules! dirs_cte {
    () => {
        "WITH p AS (SELECT t.album_id, replace(t.path, '\\', '/') AS q FROM tracks t
                    WHERE t.removed_at IS NULL AND t.album_id IS NOT NULL),
              dirs AS (SELECT DISTINCT album_id, rtrim(q, replace(q, '/', '')) AS dir FROM p) "
    };
}
macro_rules! split_where {
    () => {
        " AND EXISTS (SELECT 1 FROM dirs d1 JOIN dirs d2 ON d2.dir = d1.dir AND d2.album_id <> d1.album_id
                      JOIN albums b ON b.id = d2.album_id
                      WHERE d1.album_id = a.id AND b.title = a.title AND b.artist_id <> a.artist_id)"
    };
}
macro_rules! cover_differs_where {
    () => {
        " AND (SELECT count(DISTINCT t.art_hash) FROM tracks t
               WHERE t.album_id = a.id AND t.removed_at IS NULL AND t.art_hash IS NOT NULL) > 1"
    };
}
/// Per disc: numbers starting at 1 with holes, or one number used twice.
macro_rules! track_gaps_where {
    () => {
        " AND EXISTS (SELECT 1 FROM (
              SELECT album_id, min(track_no) AS lo, max(track_no) AS hi,
                     count(DISTINCT track_no) AS uniq, count(*) AS n
              FROM tracks WHERE removed_at IS NULL AND track_no > 0
              GROUP BY album_id, coalesce(disc_no, 1)) g
              WHERE g.album_id = a.id AND ((g.lo = 1 AND g.hi > g.uniq) OR g.n > g.uniq))"
    };
}
macro_rules! song_columns {
    ($detail:expr, $noticed:literal) => {
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
pub const REPORT_REASONS: [&str; 10] = [
    "wrong_info",
    "wrong_cover",
    "sounds_bad",
    "wrong_song",
    "other",
    // About the song's lyrics.
    "lyrics_wrong_song",
    "lyrics_out_of_sync",
    "lyrics_garbled",
    "lyrics_missing",
    "lyrics_other",
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

    /// Clears the reports about a song's lyrics only.
    pub async fn clear_lyrics_reports(&self, track_id: i64) -> Result<u64> {
        let done = sqlx::query(
            "DELETE FROM song_reports WHERE track_id = ? AND substr(reason, 1, 7) = 'lyrics_'",
        )
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
            K::ReportedLyrics => sqlx::query_scalar(concat!(
                "SELECT COUNT(DISTINCT t.id) ",
                reported_lyrics_from!()
            )),
            K::DuplicateAlbums => {
                return Ok(i64::try_from(self.album_twins().await?.len()).unwrap_or(i64::MAX))
            }
            K::IdenticalFiles => sqlx::query_scalar(concat!("SELECT COUNT(*) ", identical_from!())),
            K::BogusValues => sqlx::query_scalar(concat!("SELECT COUNT(*) ", bogus_from!())),
            K::OddLength => sqlx::query_scalar(concat!("SELECT COUNT(*) ", odd_length_from!())),
            K::LowBitrate => sqlx::query_scalar(concat!("SELECT COUNT(*) ", low_bitrate_from!())),
            K::TitleJunk => sqlx::query_scalar(concat!("SELECT COUNT(*) ", title_junk_from!())),
            K::NameMismatch => {
                sqlx::query_scalar(concat!("SELECT COUNT(*) ", name_mismatch_from!()))
            }
            K::NoLyrics => sqlx::query_scalar(concat!("SELECT COUNT(*) ", no_lyrics_from!())),
            K::NoLoudness => sqlx::query_scalar(concat!("SELECT COUNT(*) ", no_loudness_from!())),
            K::Vanished => sqlx::query_scalar(concat!("SELECT COUNT(*) ", vanished_from!()))
                .bind(vanished_since()),
            K::ArtistVariants => {
                return Ok(i64::try_from(self.artist_variants().await?.len()).unwrap_or(i64::MAX))
            }
            K::SplitAcrossArtists => sqlx::query_scalar(concat!(
                dirs_cte!(),
                "SELECT COUNT(*) ",
                album_from!(),
                split_where!()
            )),
            K::CoverDiffers => sqlx::query_scalar(concat!(
                "SELECT COUNT(*) ",
                album_from!(),
                cover_differs_where!()
            )),
            K::TrackGaps => sqlx::query_scalar(concat!(
                "SELECT COUNT(*) ",
                album_from!(),
                track_gaps_where!()
            )),
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

    /// Names that differ only by capitals, a leading "The", punctuation or
    /// spaces ("The Beatles" and "Beatles", "AC/DC" and "ACDC"): the groups
    /// of names, for artists that have songs.
    async fn artist_variants(&self) -> Result<Vec<Vec<String>>> {
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT ar.name FROM artists ar
             JOIN tracks t ON t.artist_id = ar.id AND t.removed_at IS NULL
             ORDER BY ar.name",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut groups: HashMap<String, Vec<String>> = HashMap::new();
        for name in names {
            let key = artist_key(&name);
            if !key.is_empty() {
                groups.entry(key).or_default().push(name);
            }
        }
        let mut found: Vec<Vec<String>> = groups.into_values().filter(|g| g.len() > 1).collect();
        found.sort();
        Ok(found)
    }

    async fn artist_variant_rows(&self, offset: i64, limit: i64) -> Result<Vec<ProblemItem>> {
        let groups = self.artist_variants().await?;
        let start = usize::try_from(offset.max(0))
            .unwrap_or(0)
            .min(groups.len());
        let end = start
            .saturating_add(usize::try_from(limit.max(0)).unwrap_or(0))
            .min(groups.len());
        Ok(groups[start..end]
            .iter()
            .map(|names| ProblemItem {
                detail: names.join("  /  "),
                artist: names.first().cloned(),
                ..ProblemItem::default()
            })
            .collect())
    }

    /// One row per pair of albums that are the same album: the first of
    /// them, and where the other is.
    async fn duplicate_album_rows(&self, offset: i64, limit: i64) -> Result<Vec<ProblemItem>> {
        #[derive(FromRow)]
        struct Row {
            title: String,
            artist: String,
            path: Option<String>,
        }
        let twins = self.album_twins().await?;
        let start = usize::try_from(offset.max(0)).unwrap_or(0).min(twins.len());
        let end = start
            .saturating_add(usize::try_from(limit.max(0)).unwrap_or(0))
            .min(twins.len());
        let mut items = Vec::with_capacity(end - start);
        for twin in &twins[start..end] {
            let mut rows = Vec::with_capacity(2);
            for id in [twin.a, twin.b] {
                let row: Option<Row> = sqlx::query_as(
                    "SELECT a.title, ar.name AS artist,
                            (SELECT t.path FROM tracks t WHERE t.album_id = a.id
                             AND t.removed_at IS NULL ORDER BY t.disc_no, t.track_no LIMIT 1) AS path
                     FROM albums a JOIN artists ar ON ar.id = a.artist_id WHERE a.id = ?",
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
                rows.push(row);
            }
            let (Some(first), Some(second)) = (rows.remove(0), rows.remove(0)) else {
                continue;
            };
            let folder = |path: &Option<String>| {
                path.as_deref()
                    .and_then(|p| p.rsplit_once(['/', '\\']).map(|(dir, _)| dir.to_owned()))
                    .unwrap_or_default()
            };
            items.push(ProblemItem {
                path: first.path.clone(),
                detail: format!(
                    "Also filed as \"{}\" in {}: {} of {} songs match",
                    second.title,
                    folder(&second.path),
                    twin.shared,
                    twin.a_songs.max(twin.b_songs)
                ),
                track_id: None,
                album_id: Some(twin.a),
                title: None,
                artist: Some(first.artist),
                album: Some(first.title),
                noticed_at: None,
            });
        }
        Ok(items)
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
            K::ReportedLyrics => sqlx::query_as(concat!(
                "SELECT t.path,
                        group_concat(r.reason || char(31) || coalesce(r.note, ''), char(30)) AS detail,
                        t.id AS track_id, t.album_id, t.title, t.artist, t.album,
                        max(r.created_at) AS noticed_at ",
                reported_lyrics_from!(),
                " GROUP BY t.id ORDER BY noticed_at DESC LIMIT ? OFFSET ?"
            )),
            K::DuplicateAlbums => return self.duplicate_album_rows(offset, limit).await,
            K::ArtistVariants => return self.artist_variant_rows(offset, limit).await,
            K::IdenticalFiles => sqlx::query_as(concat!(
                song_columns!("'The same file twice in this album'", "NULL"),
                identical_from!(),
                " ORDER BY t.album_id, lower(t.title), t.path LIMIT ? OFFSET ?"
            )),
            K::BogusValues => sqlx::query_as(concat!(
                song_columns!(bogus_detail!(), "NULL"),
                bogus_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::OddLength => sqlx::query_as(concat!(
                song_columns!(odd_length_detail!(), "NULL"),
                odd_length_from!(),
                " ORDER BY t.duration_ms LIMIT ? OFFSET ?"
            )),
            K::LowBitrate => sqlx::query_as(concat!(
                song_columns!(low_bitrate_detail!(), "NULL"),
                low_bitrate_from!(),
                " ORDER BY t.size * 8 / t.duration_ms, t.path LIMIT ? OFFSET ?"
            )),
            K::TitleJunk => sqlx::query_as(concat!(
                song_columns!(title_junk_detail!(), "NULL"),
                title_junk_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::NameMismatch => sqlx::query_as(concat!(
                song_columns!("'The file name doesn''t contain the title'", "NULL"),
                name_mismatch_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::NoLyrics => sqlx::query_as(concat!(
                song_columns!("''", "NULL"),
                no_lyrics_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::NoLoudness => sqlx::query_as(concat!(
                song_columns!("''", "NULL"),
                no_loudness_from!(),
                " ORDER BY t.path LIMIT ? OFFSET ?"
            )),
            K::Vanished => sqlx::query_as(concat!(
                song_columns!("'Gone from the library'", "t.removed_at"),
                vanished_from!(),
                " ORDER BY t.removed_at DESC LIMIT ? OFFSET ?"
            ))
            .bind(vanished_since()),
            K::SplitAcrossArtists => sqlx::query_as(concat!(
                dirs_cte!(),
                album_columns!(),
                album_from!(),
                split_where!(),
                " ORDER BY ar.name, a.title LIMIT ? OFFSET ?"
            )),
            K::CoverDiffers => sqlx::query_as(concat!(
                album_columns!(),
                album_from!(),
                cover_differs_where!(),
                " ORDER BY ar.name, a.title LIMIT ? OFFSET ?"
            )),
            K::TrackGaps => sqlx::query_as(concat!(
                album_columns!(),
                album_from!(),
                track_gaps_where!(),
                " ORDER BY ar.name, a.title LIMIT ? OFFSET ?"
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
    async fn lyrics_reports_are_their_own_kind() {
        let store = Store::open_in_memory().await.unwrap();
        store.save_scanned(&[song("/m/x.mp3", "x")]).await.unwrap();
        let id = store.problems(ProblemKind::NoArtist, 0, 1).await.unwrap();
        assert!(id.is_empty());
        let track: i64 = sqlx::query_scalar("SELECT id FROM tracks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO users (username, password_hash, role, created_at) VALUES ('a', 'x', 'admin', 1)")
            .execute(&store.pool)
            .await
            .unwrap();
        let user: i64 = sqlx::query_scalar("SELECT id FROM users")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert!(store
            .report_song(track, user, "lyrics_out_of_sync", Some("late"))
            .await
            .unwrap());
        let counts = store.problem_counts().await.unwrap();
        assert_eq!(count(&counts, ProblemKind::ReportedLyrics), 1);
        assert_eq!(count(&counts, ProblemKind::Reported), 0);
        assert_eq!(store.clear_lyrics_reports(track).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn the_same_album_filed_twice_is_a_problem_but_a_deluxe_edition_or_a_shared_song_is_not()
    {
        let store = Store::open_in_memory().await.unwrap();
        let track = |path: &str, album: &str, title: &str, n: i64| ScannedTrack {
            track_no: Some(n),
            album: Some(album.into()),
            ..song(path, title)
        };
        let mut songs = Vec::new();
        // Twelve songs filed twice under two folders and names.
        for n in 1..=12 {
            songs.push(track(
                &format!("/m/a/{n}.flac"),
                "Abbey Road",
                &format!("Song {n}"),
                n,
            ));
            songs.push(track(
                &format!("/m/b/{n}.flac"),
                "Abbey Road (Remastered)",
                &format!("Song {n}"),
                n,
            ));
        }
        // A deluxe edition with extra songs, and a greatest hits with a few of them.
        for n in 1..=20 {
            songs.push(track(
                &format!("/m/c/{n}.flac"),
                "Abbey Road Deluxe",
                &format!("Song {n}"),
                n,
            ));
        }
        for n in 1..=3 {
            songs.push(track(
                &format!("/m/d/{n}.flac"),
                "Hits",
                &format!("Song {n}"),
                n,
            ));
        }
        store.save_scanned(&songs).await.unwrap();
        let counts = store.problem_counts().await.unwrap();
        assert_eq!(count(&counts, ProblemKind::DuplicateAlbums), 1);
        let rows = store
            .problems(ProblemKind::DuplicateAlbums, 0, 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].detail.contains("12 of 12 songs match"),
            "{}",
            rows[0].detail
        );
        assert!(rows[0].detail.contains("/m/"), "{}", rows[0].detail);
    }

    #[tokio::test]
    async fn every_new_check_finds_its_case_and_leaves_good_songs_alone() {
        use ProblemKind as K;
        let store = Store::open_in_memory().await.unwrap();
        let good = |path: &str, title: &str| ScannedTrack {
            size: 5_000_000,
            duration_ms: Some(200_000),
            ..song(path, title)
        };
        let mut songs = vec![good("/m/ok/Fine Song.flac", "Fine Song")];
        // Bogus values, odd length, a low bit rate.
        songs.push(ScannedTrack {
            track_no: Some(0),
            ..good("/m/odd/Zero.flac", "Zero")
        });
        songs.push(ScannedTrack {
            duration_ms: Some(5_000),
            ..good("/m/odd/Short.flac", "Short")
        });
        songs.push(ScannedTrack {
            duration_ms: Some(14_400_000),
            size: 900_000_000,
            ..good("/m/odd/Long.flac", "Long")
        });
        songs.push(ScannedTrack {
            size: 400_000,
            ..good("/m/odd/Thin.mp3", "Thin")
        });
        // Junk in titles, and a file name that doesn't say the title.
        songs.push(good("/m/junk/a.flac", "01 - Numbered"));
        songs.push(good("/m/junk/b.flac", "Tagged (Official Video)"));
        songs.push(good("/m/junk/c.flac", "SHOUTING TITLE"));
        songs.push(good("/m/junk/track07.mp3", "Real Name"));
        // Two spellings of one artist.
        let by = |path: &str, artist: &str| ScannedTrack {
            artist: Some(artist.into()),
            album: Some(format!("{artist} album")),
            ..good(path, "Song")
        };
        songs.extend([
            by("/m/b1/x.flac", "The Beatles"),
            by("/m/b2/x.flac", "Beatles"),
            by("/m/b3/x.flac", "AC/DC"),
            by("/m/b4/x.flac", "ACDC"),
        ]);
        // An album with a hole in its numbers and covers that differ.
        for (n, art) in [(1, "a"), (2, "a"), (4, "b")] {
            songs.push(ScannedTrack {
                track_no: Some(n),
                album: Some("Holey".into()),
                embedded_art: Some(art.into()),
                ..good(&format!("/m/holey/{n}.flac"), &format!("Hole {n}"))
            });
        }
        // The same album in one folder under two artists.
        for (n, artist) in [(1, "Left"), (2, "Right")] {
            songs.push(ScannedTrack {
                album: Some("Split".into()),
                album_artist: Some(artist.into()),
                track_no: Some(n),
                ..good(&format!("/m/split/{n}.flac"), &format!("Split {n}"))
            });
        }
        // The same file twice in one album.
        for dir in ["x", "y"] {
            songs.push(ScannedTrack {
                album: Some("Twice".into()),
                ..good(&format!("/m/{dir}/Echo.flac"), "Echo")
            });
        }
        store.save_scanned(&songs).await.unwrap();
        // Loudness known for one song only; one song gone.
        sqlx::query("UPDATE tracks SET loudness_lufs = -14 WHERE title = 'Fine Song'")
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tracks SET removed_at = ? WHERE title = 'Zero'")
            .bind(now_ms())
            .execute(&store.pool)
            .await
            .unwrap();
        let counts = store.problem_counts().await.unwrap();
        let n = |k| count(&counts, k);
        // Zero is gone, so it isn't a bogus value any more but is listed as vanished.
        assert_eq!(n(K::BogusValues), 0);
        assert_eq!(n(K::Vanished), 1);
        assert_eq!(n(K::OddLength), 2);
        assert_eq!(n(K::LowBitrate), 1);
        assert_eq!(n(K::TitleJunk), 3);
        assert!(n(K::NameMismatch) >= 1);
        assert_eq!(n(K::ArtistVariants), 2);
        assert_eq!(n(K::TrackGaps), 1);
        assert_eq!(n(K::CoverDiffers), 1);
        assert_eq!(n(K::SplitAcrossArtists), 2);
        assert_eq!(n(K::IdenticalFiles), 2);
        assert!(n(K::NoLyrics) > 10);
        assert!(n(K::NoLoudness) > 10);
        // Each list answers without an SQL error, with the detail where there is one.
        for (kind, _) in &counts {
            store.problems(*kind, 0, 5).await.unwrap();
        }
        let long = store.problems(K::OddLength, 0, 5).await.unwrap();
        assert!(long.iter().any(|i| i.detail.contains("seconds long")));
        let variants = store.problems(K::ArtistVariants, 0, 5).await.unwrap();
        assert!(variants.iter().any(|i| i.detail.contains("The Beatles")));
        // Good songs aren't in any of the new lists.
        for kind in [K::BogusValues, K::OddLength, K::LowBitrate, K::TitleJunk] {
            let rows = store.problems(kind, 0, 50).await.unwrap();
            assert!(
                rows.iter().all(|r| r.title.as_deref() != Some("Fine Song")),
                "{kind:?}"
            );
        }
    }

    /// Run by hand: `cargo test -p djinn-store --release big_library -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "timing on a big library"]
    async fn problem_counts_on_a_big_library() {
        let store = Store::open_in_memory().await.unwrap();
        let mut songs = Vec::new();
        for album in 0..3000 {
            for n in 1..=12 {
                songs.push(ScannedTrack {
                    path: format!("/m/{}/{album}/{n:02} Song {n}.flac", album % 50),
                    title: format!("Song {}", (album * 12 + n) % 9000),
                    artist: Some(format!("Artist {}", album % 800)),
                    album: Some(format!("Album {album}")),
                    track_no: Some(n),
                    size: 5_000_000,
                    duration_ms: Some(200_000),
                    ..Default::default()
                });
            }
        }
        for chunk in songs.chunks(2000) {
            store.save_scanned(chunk).await.unwrap();
        }
        let started = std::time::Instant::now();
        let counts = store.problem_counts().await.unwrap();
        println!("all counts: {:?}", started.elapsed());
        for kind in ProblemKind::ALL {
            let at = std::time::Instant::now();
            store.problems(kind, 0, 100).await.unwrap();
            println!("{:?}: {:?} ({})", kind, at.elapsed(), count(&counts, kind));
        }
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
