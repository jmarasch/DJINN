//! Artist and album details looked up online (photos, bios, genres,
//! covers), and each artist's most played songs.

use super::*;

impl Store {
    /// Artists with playable tracks whose online info was never looked up
    /// or was last looked up before `older_than` (Unix ms).
    pub async fn artists_for_info_lookup(&self, older_than: i64) -> Result<Vec<InfoLookupArtist>> {
        let artists = sqlx::query_as(concat!(
            "SELECT ar.id, ar.name,
                    EXISTS (SELECT 1 FROM tracks t WHERE t.id IN ",
            artist_songs!("ar.id"),
            " AND t.artist_art IS NOT NULL AND t.removed_at IS NULL) AS has_folder_art,
                    coalesce((SELECT group_concat(title, char(10)) FROM (
                        SELECT t.title FROM track_artists ta JOIN tracks t ON t.id = ta.track_id
                        WHERE ta.artist_id = ar.id AND t.removed_at IS NULL
                        ORDER BY t.id LIMIT 5)), '') AS sample_titles
             FROM artists ar
             WHERE (ar.info_checked_at IS NULL OR ar.info_checked_at < ?)
               AND EXISTS (SELECT 1 FROM tracks t WHERE t.id IN ",
            artist_songs!("ar.id"),
            " AND t.removed_at IS NULL)
             ORDER BY ar.name"
        ))
        .bind(older_than)
        .fetch_all(&self.pool)
        .await?;
        Ok(artists)
    }

    /// Saves what an online lookup found for an artist, keeping earlier
    /// finds for anything it didn't find this time.
    pub async fn save_artist_info(&self, id: i64, info: &ArtistInfo) -> Result<()> {
        sqlx::query(
            "UPDATE artists SET online_art_hash = coalesce(?, online_art_hash),
                                photo_credit = CASE WHEN ? IS NULL THEN photo_credit ELSE ? END,
                                photo_credit_url = CASE WHEN ? IS NULL THEN photo_credit_url
                                                        ELSE ? END,
                                bio = coalesce(?, bio), bio_url = coalesce(?, bio_url),
                                mbid = coalesce(?, mbid), genres = coalesce(?, genres),
                                info_checked_at = CASE WHEN ? THEN ? ELSE info_checked_at END
             WHERE id = ?",
        )
        .bind(&info.photo)
        // A new photo replaces the credit (even with none, e.g. from Deezer).
        .bind(&info.photo)
        .bind(&info.photo_credit)
        .bind(&info.photo)
        .bind(&info.photo_credit_url)
        .bind(&info.bio)
        .bind(&info.bio_url)
        .bind(&info.mbid)
        .bind(&info.genres)
        .bind(info.checked)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Albums with playable tracks not looked up online since `older_than`.
    pub async fn albums_for_info_lookup(&self, older_than: i64) -> Result<Vec<InfoLookupAlbum>> {
        let albums = sqlx::query_as(
            "SELECT al.id, al.title, ar.name AS artist,
                    NOT EXISTS (SELECT 1 FROM tracks t
                                WHERE t.album_id = al.id AND t.art_hash IS NOT NULL)
                        AS needs_cover
             FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE (al.info_checked_at IS NULL OR al.info_checked_at < ?)
               AND EXISTS (SELECT 1 FROM tracks t
                           WHERE t.album_id = al.id AND t.removed_at IS NULL)
             ORDER BY ar.name, al.title",
        )
        .bind(older_than)
        .fetch_all(&self.pool)
        .await?;
        Ok(albums)
    }

    pub async fn save_album_info(
        &self,
        id: i64,
        cover: Option<&str>,
        mbid: Option<&str>,
        genres: Option<&str>,
        checked: bool,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE albums SET online_art_hash = coalesce(?, online_art_hash),
                               mbid = coalesce(?, mbid), genres = coalesce(?, genres),
                               info_checked_at = CASE WHEN ? THEN ? ELSE info_checked_at END
             WHERE id = ?",
        )
        .bind(cover)
        .bind(mbid)
        .bind(genres)
        .bind(checked)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The artist's songs, most requested first, then most played.
    pub async fn artist_top_tracks(&self, artist_id: i64, limit: i64) -> Result<Vec<TopTrack>> {
        let tracks = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            ", (SELECT count(*) FROM request_events r WHERE r.track_id = t.id) AS requests,
               (SELECT count(*) FROM play_events p WHERE p.track_id = t.id) AS plays
             FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
             WHERE ",
            available!(),
            " AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ?) OR al.artist_id = ?)
             ORDER BY requests DESC, plays DESC, t.title COLLATE NOCASE
             LIMIT ?"
        ))
        .bind(artist_id)
        .bind(artist_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(tracks)
    }
}
