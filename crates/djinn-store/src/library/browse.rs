//! Browsing the library a page at a time, the A to Z (or year) jumps
//! beside each list, and search.

use super::*;

impl Store {
    /// Artists with playable tracks, A to Z.
    pub async fn artists_page(&self, offset: i64, limit: i64) -> Result<Page<ArtistSummary>> {
        let items = sqlx::query_as(concat!(
            "SELECT id, name, art_hash, track_count FROM (",
            artist_counts!(),
            ") WHERE track_count > 0 ORDER BY ",
            sort_key!("name"),
            " LIMIT ? OFFSET ?"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let total = sqlx::query_scalar(concat!(
            "SELECT count(*) FROM (",
            artist_counts!(),
            ") WHERE track_count > 0"
        ))
        .fetch_one(&self.pool)
        .await?;
        Ok(Page { items, total })
    }

    /// Albums with playable tracks, A to Z or newest first.
    pub async fn albums_page(
        &self,
        sort: AlbumSort,
        offset: i64,
        limit: i64,
    ) -> Result<Page<Album>> {
        let sql = match sort {
            AlbumSort::Name => concat!(
                "SELECT ",
                album_columns!(),
                listed_albums!(),
                " ORDER BY ",
                album_order!(name),
                " LIMIT ? OFFSET ?"
            ),
            AlbumSort::Year => concat!(
                "SELECT ",
                album_columns!(),
                listed_albums!(),
                " ORDER BY ",
                album_order!(newest),
                " LIMIT ? OFFSET ?"
            ),
            AlbumSort::Oldest => concat!(
                "SELECT ",
                album_columns!(),
                listed_albums!(),
                " ORDER BY ",
                album_order!(oldest),
                " LIMIT ? OFFSET ?"
            ),
        };
        let items = sqlx::query_as(sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        let total = sqlx::query_scalar(concat!("SELECT count(*)", listed_albums!()))
            .fetch_one(&self.pool)
            .await?;
        Ok(Page { items, total })
    }

    /// All playable tracks in the given order.
    pub async fn tracks_page(
        &self,
        sort: TrackSort,
        offset: i64,
        limit: i64,
    ) -> Result<Page<Track>> {
        let sql = match sort {
            TrackSort::Title => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(title),
                " LIMIT ? OFFSET ?"
            ),
            TrackSort::Artist => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(artist),
                " LIMIT ? OFFSET ?"
            ),
            TrackSort::Year => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(newest),
                " LIMIT ? OFFSET ?"
            ),
            TrackSort::Oldest => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(oldest),
                " LIMIT ? OFFSET ?"
            ),
            TrackSort::Added => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY t.added_at DESC, t.id DESC LIMIT ? OFFSET ?"
            ),
            TrackSort::Popular => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY (SELECT count(*) FROM request_events r WHERE r.track_id = t.id) DESC,
                           (SELECT count(*) FROM play_events p WHERE p.track_id = t.id) DESC,
                           t.title COLLATE NOCASE LIMIT ? OFFSET ?"
            ),
        };
        let items = sqlx::query_as(sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        Ok(Page {
            items,
            total: self.track_count().await?,
        })
    }

    /// Where each letter starts in the artist list.
    pub async fn artist_jumps(&self) -> Result<Vec<Jump>> {
        let keys = sqlx::query_scalar(concat!(
            "SELECT ",
            sort_key!("name"),
            " FROM (",
            artist_counts!(),
            ") WHERE track_count > 0 ORDER BY ",
            sort_key!("name")
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(jumps(keys, false))
    }

    /// Where each letter (or year) starts in the album list.
    pub async fn album_jumps(&self, sort: AlbumSort) -> Result<Vec<Jump>> {
        let sql = match sort {
            AlbumSort::Name => concat!(
                "SELECT ",
                sort_key!("al.title"),
                listed_albums!(),
                " ORDER BY ",
                album_order!(name)
            ),
            AlbumSort::Year => concat!(
                "SELECT CAST(al.year AS TEXT)",
                listed_albums!(),
                " ORDER BY ",
                album_order!(newest)
            ),
            AlbumSort::Oldest => concat!(
                "SELECT CAST(al.year AS TEXT)",
                listed_albums!(),
                " ORDER BY ",
                album_order!(oldest)
            ),
        };
        let keys = sqlx::query_scalar(sql).fetch_all(&self.pool).await?;
        Ok(jumps(keys, sort != AlbumSort::Name))
    }

    /// Where each letter (or year) starts in the song list; none for the
    /// sorts that aren't by name or year.
    pub async fn track_jumps(&self, sort: TrackSort) -> Result<Vec<Jump>> {
        let sql = match sort {
            TrackSort::Title => concat!(
                "SELECT t.title FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(title)
            ),
            TrackSort::Artist => concat!(
                "SELECT ",
                sort_key!("coalesce(t.album_artist, t.artist)"),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(artist)
            ),
            TrackSort::Year => concat!(
                "SELECT CAST(t.year AS TEXT) FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(newest)
            ),
            TrackSort::Oldest => concat!(
                "SELECT CAST(t.year AS TEXT) FROM tracks t WHERE ",
                available!(),
                " ORDER BY ",
                track_order!(oldest)
            ),
            TrackSort::Added | TrackSort::Popular => return Ok(Vec::new()),
        };
        let keys = sqlx::query_scalar(sql).fetch_all(&self.pool).await?;
        Ok(jumps(
            keys,
            matches!(sort, TrackSort::Year | TrackSort::Oldest),
        ))
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
               AND EXISTS (
                 SELECT 1 FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
                 WHERE ",
            available!(),
            " AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ar.id) OR al.artist_id = ar.id))
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
