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
                                commons_category = coalesce(?, commons_category),
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
        .bind(&info.commons_category)
        .bind(info.checked)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Where an artist's slideshow pictures come from; `None` for no such
    /// artist.
    pub async fn picture_source(&self, artist_id: i64) -> Result<Option<PictureSource>> {
        let source = sqlx::query_as(
            "SELECT name, mbid, commons_category, pictures_checked_at IS NOT NULL AS checked
             FROM artists WHERE id = ?",
        )
        .bind(artist_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(source)
    }

    /// An artist's slideshow pictures, in order.
    pub async fn artist_pictures(&self, artist_id: i64) -> Result<Vec<ArtistPicture>> {
        let pictures = sqlx::query_as(
            "SELECT art_hash, credit, credit_url, source, remote_url FROM artist_pictures
             WHERE artist_id = ? ORDER BY position",
        )
        .bind(artist_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(pictures)
    }

    /// Replaces an artist's slideshow pictures and marks them fetched.
    pub async fn save_artist_pictures(
        &self,
        artist_id: i64,
        pictures: &[ArtistPicture],
    ) -> Result<()> {
        let retained = sqlx::query_as::<_, ArtistPicture>(
            "SELECT art_hash, credit, credit_url, source, remote_url FROM artist_pictures
             WHERE artist_id = ? AND source IN ('manual', 'unsplash') ORDER BY position",
        )
        .bind(artist_id)
        .fetch_all(&self.pool)
        .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM artist_pictures WHERE artist_id = ?")
            .bind(artist_id)
            .execute(&mut *tx)
            .await?;
        for (position, picture) in (0_i64..).zip(retained.iter().chain(pictures)) {
            sqlx::query(
                "INSERT INTO artist_pictures (artist_id, position, art_hash, credit, credit_url, source, remote_url)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(artist_id)
            .bind(position)
            .bind(&picture.art_hash)
            .bind(&picture.credit)
            .bind(&picture.credit_url)
            .bind(&picture.source)
            .bind(&picture.remote_url)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE artists SET pictures_checked_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(artist_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn add_artist_picture(&self, artist_id: i64, picture: &ArtistPicture) -> Result<()> {
        sqlx::query(
            "INSERT INTO artist_pictures (artist_id, position, art_hash, credit, credit_url, source, remote_url)
             VALUES (?, coalesce((SELECT max(position) + 1 FROM artist_pictures WHERE artist_id = ?), 0), ?, ?, ?, ?, ?)",
        )
        .bind(artist_id)
        .bind(artist_id)
        .bind(&picture.art_hash)
        .bind(&picture.credit)
        .bind(&picture.credit_url)
        .bind(&picture.source)
        .bind(&picture.remote_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_artist_picture(&self, artist_id: i64, hash: &str) -> Result<bool> {
        let removed =
            sqlx::query("DELETE FROM artist_pictures WHERE artist_id = ? AND art_hash = ?")
                .bind(artist_id)
                .bind(hash)
                .execute(&self.pool)
                .await?;
        Ok(removed.rows_affected() > 0)
    }

    pub async fn reorder_artist_pictures(&self, artist_id: i64, hashes: &[String]) -> Result<bool> {
        let existing = self.artist_pictures(artist_id).await?;
        if existing.len() != hashes.len()
            || !existing
                .iter()
                .all(|picture| hashes.contains(&picture.art_hash))
        {
            return Ok(false);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM artist_pictures WHERE artist_id = ?")
            .bind(artist_id)
            .execute(&mut *tx)
            .await?;
        for (position, hash) in (0_i64..).zip(hashes) {
            let Some(picture) = existing.iter().find(|picture| picture.art_hash == *hash) else {
                return Ok(false);
            };
            sqlx::query(
                "INSERT INTO artist_pictures (artist_id, position, art_hash, credit, credit_url, source, remote_url)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(artist_id)
            .bind(position)
            .bind(&picture.art_hash)
            .bind(&picture.credit)
            .bind(&picture.credit_url)
            .bind(&picture.source)
            .bind(&picture.remote_url)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn art_hash_in_use(&self, hash: &str) -> Result<bool> {
        let used: i64 = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM artist_pictures WHERE art_hash = ?
                UNION ALL SELECT 1 FROM tracks WHERE art_hash = ? OR embedded_art = ? OR folder_art = ? OR artist_art = ?
                UNION ALL SELECT 1 FROM albums WHERE art_hash = ? OR online_art_hash = ?
                UNION ALL SELECT 1 FROM artists WHERE art_hash = ? OR online_art_hash = ?
             )",
        )
        .bind(hash).bind(hash).bind(hash).bind(hash).bind(hash)
        .bind(hash).bind(hash).bind(hash).bind(hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(used != 0)
    }

    pub async fn recheck_artist_pictures(&self, artist_id: i64) -> Result<()> {
        sqlx::query("UPDATE artists SET pictures_checked_at = NULL WHERE id = ?")
            .bind(artist_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn recheck_all_artist_pictures(&self) -> Result<()> {
        sqlx::query("UPDATE artists SET pictures_checked_at = NULL")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Source changes discard online portraits and bios so the next lookup
    /// reflects the newly enabled providers.
    pub async fn clear_online_artist_visuals(&self) -> Result<()> {
        sqlx::query(
            "UPDATE artists SET online_art_hash = NULL, photo_credit = NULL,
                    photo_credit_url = NULL, bio = NULL, bio_url = NULL, info_checked_at = NULL",
        )
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
