//! Browsing the library a page at a time, the A to Z (or year) jumps
//! beside each list, and search.

use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One list in its order, with where each letter or year starts.
#[derive(Debug)]
struct Listed<T> {
    items: Vec<T>,
    jumps: Vec<Jump>,
}

impl<T> Listed<T> {
    fn page(&self, offset: i64, limit: i64) -> &[T] {
        let start = usize::try_from(offset).unwrap_or(0).min(self.items.len());
        let end = start
            .saturating_add(usize::try_from(limit).unwrap_or(0))
            .min(self.items.len());
        &self.items[start..end]
    }

    fn total(&self) -> i64 {
        i64::try_from(self.items.len()).unwrap_or(i64::MAX)
    }
}

#[derive(Debug, Default)]
struct Lists {
    version: i64,
    artists: HashMap<bool, Arc<Listed<ArtistSummary>>>,
    albums: HashMap<(AlbumSort, AlbumFilter), Arc<Listed<Album>>>,
    /// Songs are read a page at a time, so only their order is kept.
    tracks: HashMap<TrackSort, Arc<Listed<i64>>>,
}

/// Library lists worked out once and kept until `browse_version` says the
/// library changed, so pages and jumps don't repeat the same counting and
/// sorting over the whole library. Holding the lock while building means a
/// page and its jumps arriving together only build a list once.
#[derive(Debug, Default)]
pub(crate) struct BrowseMemo(tokio::sync::Mutex<Lists>);

/// A cancelled browse request discards its connection, so an unfinished
/// statement cannot affect the next request using that connection.
struct TimedBrowse {
    conn: sqlx::pool::PoolConnection<Sqlite>,
    clean: bool,
}

impl TimedBrowse {
    async fn acquire(pool: &sqlx::SqlitePool) -> Result<Self> {
        let mut conn = pool.acquire().await?;
        let deadline = Instant::now() + Duration::from_secs(60);
        conn.lock_handle()
            .await?
            .set_progress_handler(10_000, move || Instant::now() < deadline);
        Ok(Self { conn, clean: false })
    }

    async fn finish(mut self) -> Result<()> {
        self.conn.lock_handle().await?.remove_progress_handler();
        self.clean = true;
        Ok(())
    }
}

impl Drop for TimedBrowse {
    fn drop(&mut self) {
        if !self.clean {
            self.conn.close_on_drop();
        }
    }
}

// The counted artist list is shared by pages, totals, and jump positions.
// Keep the normal list free of album classification work. When collaborations
// are hidden, find artists with a regular album (or an albumless song) once,
// rather than running album_kind for every track of every artist.
macro_rules! artist_browse {
    () => {
        concat!(
            " FROM (",
            artist_counts!(),
            ") summary WHERE track_count > 0 AND lower(name) NOT IN
              ('various artists', 'various', 'va', 'v.a.', 'soundtrack', 'soundtracks', 'ost', 'original soundtrack')"
        )
    };
}

macro_rules! primary_artists {
    () => {
        concat!(
            " AND summary.id IN (
                SELECT al.artist_id FROM albums al JOIN artists ar ON ar.id = al.artist_id
                WHERE ",
            album_kind!(),
            " = 'regular' AND EXISTS (
                    SELECT 1 FROM tracks t WHERE t.album_id = al.id AND t.removed_at IS NULL)
                UNION SELECT t.artist_id FROM tracks t
                WHERE t.album_id IS NULL AND t.removed_at IS NULL)"
        )
    };
}

impl Store {
    /// The kept lists, emptied first if the library has changed.
    async fn lists(&self) -> Result<tokio::sync::MutexGuard<'_, Lists>> {
        let mut lists = self.browse.0.lock().await;
        let version = sqlx::query_scalar("SELECT version FROM browse_version")
            .fetch_one(&self.pool)
            .await?;
        if lists.version != version {
            *lists = Lists {
                version,
                ..Lists::default()
            };
        }
        Ok(lists)
    }

    async fn artist_list(&self, show_collaborations: bool) -> Result<Arc<Listed<ArtistSummary>>> {
        let mut lists = self.lists().await?;
        if let Some(found) = lists.artists.get(&show_collaborations) {
            return Ok(Arc::clone(found));
        }
        let sql = if show_collaborations {
            concat!(
                "SELECT id, name, art_hash, track_count, ",
                sort_key!("name"),
                artist_browse!(),
                " ORDER BY ",
                sort_key!("name")
            )
        } else {
            concat!(
                "SELECT id, name, art_hash, track_count, ",
                sort_key!("name"),
                artist_browse!(),
                primary_artists!(),
                " ORDER BY ",
                sort_key!("name")
            )
        };
        type Row = (i64, String, Option<String>, i64, Option<String>);
        let mut browse = TimedBrowse::acquire(&self.pool).await?;
        let rows: std::result::Result<Vec<Row>, _> =
            sqlx::query_as(sql).fetch_all(&mut *browse.conn).await;
        browse.finish().await?;
        let (items, keys) = rows?
            .into_iter()
            .map(|(id, name, art_hash, track_count, key)| {
                let artist = ArtistSummary {
                    id,
                    name,
                    art_hash,
                    track_count,
                };
                (artist, key)
            })
            .unzip();
        let list = Arc::new(Listed {
            items,
            jumps: jumps(keys, false),
        });
        lists.artists.insert(show_collaborations, Arc::clone(&list));
        Ok(list)
    }

    async fn album_list(&self, sort: AlbumSort, filter: AlbumFilter) -> Result<Arc<Listed<Album>>> {
        let mut lists = self.lists().await?;
        if let Some(found) = lists.albums.get(&(sort, filter)) {
            return Ok(Arc::clone(found));
        }
        let sql = match sort {
            AlbumSort::Name => concat!(
                "SELECT ",
                album_columns!(),
                ", ",
                sort_key!("al.title"),
                listed_albums!(),
                " AND (? = 'all' OR ",
                album_kind!(),
                " = ?)",
                " ORDER BY ",
                album_order!(name)
            ),
            AlbumSort::Year => concat!(
                "SELECT ",
                album_columns!(),
                ", CAST(al.year AS TEXT)",
                listed_albums!(),
                " AND (? = 'all' OR ",
                album_kind!(),
                " = ?)",
                " ORDER BY ",
                album_order!(newest)
            ),
            AlbumSort::Oldest => concat!(
                "SELECT ",
                album_columns!(),
                ", CAST(al.year AS TEXT)",
                listed_albums!(),
                " AND (? = 'all' OR ",
                album_kind!(),
                " = ?)",
                " ORDER BY ",
                album_order!(oldest)
            ),
        };
        type Row = (
            i64,
            String,
            i64,
            String,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let rows: Vec<Row> = sqlx::query_as(sql)
            .bind(filter.as_str())
            .bind(filter.as_str())
            .fetch_all(&self.pool)
            .await?;
        let (items, keys) = rows
            .into_iter()
            .map(
                |(id, title, artist_id, artist, year, art_hash, genres, key)| {
                    let album = Album {
                        id,
                        title,
                        artist_id,
                        artist,
                        year,
                        art_hash,
                        genres,
                    };
                    (album, key)
                },
            )
            .unzip();
        let list = Arc::new(Listed {
            items,
            jumps: jumps(keys, sort != AlbumSort::Name),
        });
        lists.albums.insert((sort, filter), Arc::clone(&list));
        Ok(list)
    }

    /// The order of every song. `Popular` follows plays and requests, not
    /// the library, so `tracks_page` works it out each time.
    async fn track_list(&self, sort: TrackSort) -> Result<Arc<Listed<i64>>> {
        let mut lists = self.lists().await?;
        if let Some(found) = lists.tracks.get(&sort) {
            return Ok(Arc::clone(found));
        }
        let sql = match sort {
            TrackSort::Title => concat!(
                "SELECT t.id, t.title FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(title)
            ),
            TrackSort::Artist => concat!(
                "SELECT t.id, ",
                sort_key!("coalesce(t.album_artist, t.artist)"),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(artist)
            ),
            TrackSort::Year => concat!(
                "SELECT t.id, CAST(t.year AS TEXT) FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(newest)
            ),
            TrackSort::Oldest => concat!(
                "SELECT t.id, CAST(t.year AS TEXT) FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(oldest)
            ),
            TrackSort::Added | TrackSort::Popular => concat!(
                "SELECT t.id, NULL FROM tracks t WHERE ",
                available!(),
                " ORDER BY t.added_at DESC, t.id DESC"
            ),
        };
        let rows: Vec<(i64, Option<String>)> = sqlx::query_as(sql).fetch_all(&self.pool).await?;
        let (items, keys) = rows.into_iter().unzip();
        let list = Arc::new(Listed {
            items,
            jumps: jumps(keys, matches!(sort, TrackSort::Year | TrackSort::Oldest)),
        });
        lists.tracks.insert(sort, Arc::clone(&list));
        Ok(list)
    }

    /// Artists with playable tracks, A to Z.
    pub async fn artists_page(&self, offset: i64, limit: i64) -> Result<Page<ArtistSummary>> {
        self.artists_page_filtered(true, offset, limit).await
    }

    pub async fn artists_page_filtered(
        &self,
        show_collaborations: bool,
        offset: i64,
        limit: i64,
    ) -> Result<Page<ArtistSummary>> {
        let list = self.artist_list(show_collaborations).await?;
        Ok(Page {
            items: list.page(offset, limit).to_vec(),
            total: list.total(),
        })
    }

    /// Albums with playable tracks, A to Z or newest first.
    pub async fn albums_page(
        &self,
        sort: AlbumSort,
        offset: i64,
        limit: i64,
    ) -> Result<Page<Album>> {
        self.albums_page_filtered(sort, AlbumFilter::All, offset, limit)
            .await
    }

    pub async fn albums_page_filtered(
        &self,
        sort: AlbumSort,
        filter: AlbumFilter,
        offset: i64,
        limit: i64,
    ) -> Result<Page<Album>> {
        let list = self.album_list(sort, filter).await?;
        Ok(Page {
            items: list.page(offset, limit).to_vec(),
            total: list.total(),
        })
    }

    /// All playable tracks in the given order.
    pub async fn tracks_page(
        &self,
        sort: TrackSort,
        offset: i64,
        limit: i64,
    ) -> Result<Page<Track>> {
        if sort == TrackSort::Popular {
            let items = sqlx::query_as(concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY (SELECT count(*) FROM request_events r WHERE r.track_id = t.id) DESC,
                           (SELECT count(*) FROM play_events p WHERE p.track_id = t.id) DESC,
                           t.title COLLATE NOCASE LIMIT ? OFFSET ?"
            ))
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
            return Ok(Page {
                items,
                total: self.track_count().await?,
            });
        }
        let list = self.track_list(sort).await?;
        Ok(Page {
            items: self.tracks_by_ids(list.page(offset, limit)).await?,
            total: list.total(),
        })
    }

    /// Where each letter starts in the artist list.
    pub async fn artist_jumps(&self) -> Result<Vec<Jump>> {
        self.artist_jumps_filtered(true).await
    }

    pub async fn artist_jumps_filtered(&self, show_collaborations: bool) -> Result<Vec<Jump>> {
        Ok(self.artist_list(show_collaborations).await?.jumps.clone())
    }

    /// Where each letter (or year) starts in the album list.
    pub async fn album_jumps(&self, sort: AlbumSort) -> Result<Vec<Jump>> {
        self.album_jumps_filtered(sort, AlbumFilter::All).await
    }

    pub async fn album_jumps_filtered(
        &self,
        sort: AlbumSort,
        filter: AlbumFilter,
    ) -> Result<Vec<Jump>> {
        Ok(self.album_list(sort, filter).await?.jumps.clone())
    }

    /// Where each letter (or year) starts in the song list; none for the
    /// sorts that aren't by name or year.
    pub async fn track_jumps(&self, sort: TrackSort) -> Result<Vec<Jump>> {
        if matches!(sort, TrackSort::Added | TrackSort::Popular) {
            return Ok(Vec::new());
        }
        Ok(self.track_list(sort).await?.jumps.clone())
    }

    /// Search-as-you-type over tracks, albums, and artists. Every word must
    /// match the start of a word in the indexed fields.
    pub async fn search(&self, query: &str, limit: i64) -> Result<SearchResults> {
        let Some(fts) = fts_query(query) else {
            return Ok(SearchResults::default());
        };
        // Title matches rank above artist/album matches, genre lowest.
        let tracks = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            " FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
             WHERE tracks_fts MATCH ? AND ",
            available!(),
            "
             ORDER BY bm25(tracks_fts, 10.0, 5.0, 4.0, 3.0, 1.0)
             LIMIT ?"
        ))
        .bind(&fts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let albums = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM albums_fts f
             JOIN albums al ON al.id = f.rowid
             JOIN artists ar ON ar.id = al.artist_id
             WHERE albums_fts MATCH ?
               AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id AND ",
            available!(),
            ")
             ORDER BY bm25(albums_fts, 5.0, 1.0)
             LIMIT ?"
        ))
        .bind(&fts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let artists = sqlx::query_as(concat!(
            "SELECT ar.id, ar.name, ar.art_hash FROM artists_fts f JOIN artists ar ON ar.id = f.rowid
             WHERE artists_fts MATCH ?
               AND EXISTS (SELECT 1 FROM tracks t WHERE t.id IN ",
            artist_songs!("ar.id"),
            " AND ",
            available!(),
            ")
             ORDER BY bm25(artists_fts)
             LIMIT ?"
        ))
        .bind(&fts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(SearchResults {
            tracks,
            albums,
            artists,
        })
    }
}

/// Turns user input into an FTS5 prefix query: each word becomes a quoted
/// prefix term, all required. Splits on the same characters the unicode61
/// tokenizer does, so "AC/DC" searches for "ac" and "dc".
pub(super) fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}
