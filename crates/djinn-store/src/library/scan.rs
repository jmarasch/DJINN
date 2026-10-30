//! Saving what the scanner found: songs, albums, artists, artwork, and
//! songs whose files went away.

use super::*;

impl Store {
    pub async fn known_files(&self) -> Result<Vec<KnownFile>> {
        let files = sqlx::query_as(
            "SELECT id, path, size, mtime, removed_at IS NOT NULL AS removed FROM tracks",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(files)
    }

    /// Inserts or updates scanned files in one transaction.
    pub async fn save_scanned(&self, tracks: &[ScannedTrack]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let now = now_ms();
        for track in tracks {
            save_track(&mut tx, track, now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Picks album and artist images from what the scanner found, and
    /// deletes artists and albums left with no tracks (unless a playlist
    /// uses them). Run after a scan.
    pub async fn refresh_artwork(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        // Albums: the first listed song's embedded cover, then the album
        // folder's image, then any song's artwork, then a cover found online.
        sqlx::query(
            "UPDATE albums SET art_hash = coalesce(
               (SELECT t.embedded_art FROM tracks t
                WHERE t.album_id = albums.id AND t.removed_at IS NULL
                ORDER BY t.disc_no IS NULL, t.disc_no, t.track_no IS NULL, t.track_no, t.path
                LIMIT 1),
               (SELECT t.folder_art FROM tracks t
                WHERE t.album_id = albums.id AND t.removed_at IS NULL AND t.folder_art IS NOT NULL
                ORDER BY t.path LIMIT 1),
               (SELECT t.art_hash FROM tracks t
                WHERE t.album_id = albums.id AND t.removed_at IS NULL AND t.art_hash IS NOT NULL
                ORDER BY t.disc_no, t.track_no, t.path LIMIT 1),
               albums.online_art_hash)",
        )
        .execute(&mut *tx)
        .await?;
        // Artists: an image in the artist's folder, then a photo looked up
        // online, then their first album's cover, then any song's artwork.
        sqlx::query(concat!(
            "UPDATE artists SET art_hash = coalesce(
               (SELECT t.artist_art FROM tracks t
                WHERE t.id IN ",
            artist_songs!("artists.id"),
            " AND t.removed_at IS NULL AND t.artist_art IS NOT NULL
                ORDER BY t.path LIMIT 1),
               artists.online_art_hash,
               (SELECT al.art_hash FROM albums al
                WHERE al.artist_id = artists.id AND al.art_hash IS NOT NULL
                ORDER BY al.year IS NULL, al.year, al.title LIMIT 1),
               (SELECT t.art_hash FROM track_artists ta JOIN tracks t ON t.id = ta.track_id
                WHERE ta.artist_id = artists.id AND t.removed_at IS NULL AND t.art_hash IS NOT NULL
                ORDER BY t.path LIMIT 1))"
        ))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM albums
             WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = albums.id)
               AND NOT EXISTS (SELECT 1 FROM playlist_entries p
                               WHERE p.kind = 'album' AND p.ref_id = albums.id)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM artists
             WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = artists.id)
               AND NOT EXISTS (SELECT 1 FROM track_artists ta WHERE ta.artist_id = artists.id)
               AND NOT EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = artists.id)
               AND NOT EXISTS (SELECT 1 FROM playlist_entries p
                               WHERE p.kind = 'artist' AND p.ref_id = artists.id)",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_removed(&self, ids: &[i64]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let now = now_ms();
        for id in ids {
            sqlx::query("UPDATE tracks SET removed_at = ? WHERE id = ? AND removed_at IS NULL")
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

/// The scanned track with gaps filled from an applied identification, if
/// there's one for this file.
async fn fill_from_identification(
    tx: &mut Transaction<'_, Sqlite>,
    t: &ScannedTrack,
) -> Result<Option<ScannedTrack>> {
    type Row = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT ti.title, ti.artist, ti.album, ti.year, ti.genres FROM track_ids ti \
         JOIN tracks t ON t.id = ti.track_id WHERE t.path = ? AND ti.status = 'applied'",
    )
    .bind(&t.path)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((title, artist, album, year, genres)) = row else {
        return Ok(None);
    };
    let mut filled = t.clone();
    if filled.title.trim().is_empty() {
        if let Some(title) = title {
            filled.title = title;
        }
    }
    if filled.artist.is_none() {
        filled.artist = artist;
        filled.credits.clear();
    }
    if filled.album.is_none() {
        filled.album = album;
    }
    if filled.year.is_none() {
        filled.year = year;
    }
    if filled.genre.is_none() {
        filled.genre = genres.filter(|g| !g.is_empty());
    }
    Ok(Some(filled))
}

async fn save_track(tx: &mut Transaction<'_, Sqlite>, t: &ScannedTrack, now: i64) -> Result<()> {
    // An applied identification fills in what the tags lack.
    let filled = fill_from_identification(tx, t).await?;
    let t = filled.as_ref().unwrap_or(t);
    let credits: Vec<(&str, bool)> = if t.credits.is_empty() {
        t.artist
            .as_deref()
            .map(|a| (a, false))
            .into_iter()
            .collect()
    } else {
        t.credits.iter().map(|(n, f)| (n.as_str(), *f)).collect()
    };
    let mut credit_ids = Vec::with_capacity(credits.len());
    for (name, featured) in &credits {
        credit_ids.push((upsert_artist(tx, name).await?, *featured));
    }
    let artist_id = credit_ids.first().map(|&(id, _)| id);
    let album_id = match &t.album {
        Some(title) => {
            let album_artist = t
                .album_owner
                .as_deref()
                .or(t.album_artist.as_deref())
                .or(credits.first().map(|&(name, _)| name))
                .unwrap_or(UNKNOWN_ARTIST);
            let album_artist_id = upsert_artist(tx, album_artist).await?;
            Some(upsert_album(tx, title, album_artist_id, t.year).await?)
        }
        None => None,
    };
    let track_id: i64 = sqlx::query_scalar(
        "INSERT INTO tracks (path, size, mtime, duration_ms, title, artist, album, album_artist,
                             track_no, disc_no, year, genre, art_hash, sample_rate, channels,
                             codec, artist_id, album_id, added_at, embedded_art, folder_art,
                             artist_art, rg_track_gain, rg_track_peak, rg_album_gain,
                             rg_album_peak)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (path) DO UPDATE SET
            size = excluded.size, mtime = excluded.mtime, duration_ms = excluded.duration_ms,
            title = excluded.title, artist = excluded.artist, album = excluded.album,
            album_artist = excluded.album_artist, track_no = excluded.track_no,
            disc_no = excluded.disc_no, year = excluded.year, genre = excluded.genre,
            art_hash = excluded.art_hash, sample_rate = excluded.sample_rate,
            channels = excluded.channels, codec = excluded.codec,
            artist_id = excluded.artist_id, album_id = excluded.album_id, removed_at = NULL,
            embedded_art = excluded.embedded_art, folder_art = excluded.folder_art,
            artist_art = excluded.artist_art, rg_track_gain = excluded.rg_track_gain,
            rg_track_peak = excluded.rg_track_peak, rg_album_gain = excluded.rg_album_gain,
            rg_album_peak = excluded.rg_album_peak,
            -- A changed file gets measured again.
            loudness_lufs = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                 THEN loudness_lufs END,
            loudness_peak = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                 THEN loudness_peak END,
            loudness_checked_at = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                       THEN loudness_checked_at END,
            features_version = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                    THEN features_version END,
            heard_version = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                 THEN heard_version END
         RETURNING id",
    )
    .bind(&t.path)
    .bind(t.size)
    .bind(t.mtime)
    .bind(t.duration_ms)
    .bind(&t.title)
    .bind(&t.artist)
    .bind(&t.album)
    .bind(&t.album_artist)
    .bind(t.track_no)
    .bind(t.disc_no)
    .bind(t.year)
    .bind(&t.genre)
    .bind(t.art_hash())
    .bind(t.sample_rate)
    .bind(t.channels)
    .bind(&t.codec)
    .bind(artist_id)
    .bind(album_id)
    .bind(now)
    .bind(&t.embedded_art)
    .bind(&t.folder_art)
    .bind(&t.artist_art)
    .bind(t.replay_gain.track_gain)
    .bind(t.replay_gain.track_peak)
    .bind(t.replay_gain.album_gain)
    .bind(t.replay_gain.album_peak)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM track_artists WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut **tx)
        .await?;
    for (position, (artist, featured)) in credit_ids.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO track_artists (track_id, artist_id, position, featured)
             VALUES (?, ?, ?, ?)",
        )
        .bind(track_id)
        .bind(artist)
        .bind(i64::try_from(position).unwrap_or(i64::MAX))
        .bind(featured)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn upsert_artist(tx: &mut Transaction<'_, Sqlite>, name: &str) -> Result<i64> {
    // The no-op update makes RETURNING yield the existing row's id.
    let id = sqlx::query_scalar(
        "INSERT INTO artists (name) VALUES (?)
         ON CONFLICT (name) DO UPDATE SET name = artists.name
         RETURNING id",
    )
    .bind(name)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

async fn upsert_album(
    tx: &mut Transaction<'_, Sqlite>,
    title: &str,
    artist_id: i64,
    year: Option<i64>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO albums (title, artist_id, year) VALUES (?, ?, ?)
         ON CONFLICT (title, artist_id) DO UPDATE SET year = coalesce(albums.year, excluded.year)
         RETURNING id",
    )
    .bind(title)
    .bind(artist_id)
    .bind(year)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}
