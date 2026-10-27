//! Songs identified by audio fingerprint, and what the admin chose to do
//! with them.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// A song to identify.
#[derive(Clone, Debug, PartialEq, FromRow)]
pub struct ToIdentify {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: Option<i64>,
}

/// What identification found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Identification {
    pub score: f32,
    pub recording: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub genres: Vec<String>,
}

/// A found identification next to what the tags say, for review.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct IdentificationRow {
    pub track_id: i64,
    pub status: String,
    pub score: Option<f64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub genres: Option<String>,
    pub current_title: String,
    pub current_artist: Option<String>,
    pub current_album: Option<String>,
    pub current_year: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct IdentifyProgress {
    /// Songs with something missing (album, artist, or year) not checked yet.
    pub waiting: i64,
    /// Found and waiting for the admin.
    pub found: i64,
    pub applied: i64,
}

impl Store {
    /// Songs missing an album, artist, or year that haven't been looked up.
    pub async fn tracks_to_identify(&self, limit: i64) -> Result<Vec<ToIdentify>> {
        let rows = sqlx::query_as(
            "SELECT t.id, t.path, t.title, t.artist, t.duration_ms FROM tracks t \
             WHERE t.removed_at IS NULL \
               AND (t.album IS NULL OR t.artist IS NULL OR t.year IS NULL) \
               AND NOT EXISTS (SELECT 1 FROM track_ids ti WHERE ti.track_id = t.id) \
             ORDER BY t.id LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Records a lookup: what was found (waiting for review), or that
    /// nothing trustworthy matched.
    pub async fn save_identification(
        &self,
        track_id: i64,
        found: Option<&Identification>,
    ) -> Result<()> {
        let status = if found.is_some() { "found" } else { "none" };
        sqlx::query(
            "INSERT INTO track_ids (track_id, checked_at, status, score, recording, title, artist, album, year, genres) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (track_id) DO UPDATE SET checked_at = excluded.checked_at, status = excluded.status, \
                score = excluded.score, recording = excluded.recording, title = excluded.title, \
                artist = excluded.artist, album = excluded.album, year = excluded.year, genres = excluded.genres",
        )
        .bind(track_id)
        .bind(now_ms())
        .bind(status)
        .bind(found.map(|f| f64::from(f.score)))
        .bind(found.map(|f| f.recording.as_str()))
        .bind(found.map(|f| f.title.as_str()))
        .bind(found.map(|f| f.artist.as_str()))
        .bind(found.and_then(|f| f.album.as_deref()))
        .bind(found.and_then(|f| f.year))
        .bind(found.map(|f| f.genres.join(", ")))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Identifications with `status`, best matches first.
    pub async fn identifications(&self, status: &str) -> Result<Vec<IdentificationRow>> {
        let rows = sqlx::query_as(
            "SELECT ti.track_id, ti.status, ti.score, ti.title, ti.artist, ti.album, ti.year, ti.genres, \
                    t.title AS current_title, t.artist AS current_artist, t.album AS current_album, \
                    t.year AS current_year \
             FROM track_ids ti JOIN tracks t ON t.id = ti.track_id \
             WHERE ti.status = ? AND t.removed_at IS NULL \
             ORDER BY ti.score DESC, t.title",
        )
        .bind(status)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Marks identifications applied or dismissed. Returns the paths of the
    /// songs changed, to re-read them.
    pub async fn set_identification_status(
        &self,
        track_ids: &[i64],
        status: &str,
    ) -> Result<Vec<String>> {
        let mut paths = Vec::new();
        for &id in track_ids {
            let path: Option<String> = sqlx::query_scalar(
                "UPDATE track_ids SET status = ? WHERE track_id = ? AND status IN ('found', 'applied', 'dismissed') \
                 RETURNING (SELECT path FROM tracks WHERE id = track_id)",
            )
            .bind(status)
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
            paths.extend(path);
        }
        Ok(paths)
    }

    pub async fn identify_progress(&self) -> Result<IdentifyProgress> {
        let (waiting, found, applied) = sqlx::query_as(
            "SELECT \
                (SELECT count(*) FROM tracks t WHERE t.removed_at IS NULL \
                   AND (t.album IS NULL OR t.artist IS NULL OR t.year IS NULL) \
                   AND NOT EXISTS (SELECT 1 FROM track_ids ti WHERE ti.track_id = t.id)), \
                (SELECT count(*) FROM track_ids WHERE status = 'found'), \
                (SELECT count(*) FROM track_ids WHERE status = 'applied')",
        )
        .fetch_one(self.pool())
        .await?;
        Ok(IdentifyProgress {
            waiting,
            found,
            applied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScannedTrack;

    #[tokio::test]
    async fn applied_identifications_fill_gaps_on_every_scan() {
        let store = Store::open_in_memory().await.unwrap();
        let untagged = ScannedTrack {
            path: "/m/kream/taped.m4a".into(),
            size: 1,
            mtime: 1,
            title: "Taped Up Heart".into(),
            artist: Some("KREAM".into()),
            ..Default::default()
        };
        let complete = ScannedTrack {
            path: "/m/a.flac".into(),
            size: 1,
            mtime: 1,
            title: "Song".into(),
            artist: Some("A".into()),
            album: Some("Album".into()),
            year: Some(2001),
            ..Default::default()
        };
        store
            .save_scanned(&[untagged.clone(), complete])
            .await
            .unwrap();
        let todo = store.tracks_to_identify(10).await.unwrap();
        assert_eq!(todo.len(), 1, "only songs missing something");

        let found = Identification {
            score: 0.95,
            recording: "rec".into(),
            title: "Taped Up Heart".into(),
            artist: "KREAM & Clara Mae".into(),
            album: Some("Taped Up Heart".into()),
            year: Some(2017),
            genres: vec!["house".into()],
        };
        store
            .save_identification(todo[0].id, Some(&found))
            .await
            .unwrap();
        assert!(store.tracks_to_identify(10).await.unwrap().is_empty());
        let review = store.identifications("found").await.unwrap();
        assert_eq!(review[0].album.as_deref(), Some("Taped Up Heart"));
        assert_eq!(review[0].current_album, None);

        let paths = store
            .set_identification_status(&[todo[0].id], "applied")
            .await
            .unwrap();
        assert_eq!(paths, ["/m/kream/taped.m4a"]);
        // The scanner re-reads the file: the gaps get filled, and stay
        // filled on later scans; the artist from the tags is kept.
        store.save_scanned(&[untagged]).await.unwrap();
        let track = store.track(todo[0].id).await.unwrap().unwrap();
        assert_eq!(track.album.as_deref(), Some("Taped Up Heart"));
        assert_eq!(track.year, Some(2017));
        assert_eq!(track.artist.as_deref(), Some("KREAM"));
        assert_eq!(track.genre.as_deref(), Some("house"));
        assert_eq!(
            store.identify_progress().await.unwrap(),
            IdentifyProgress {
                waiting: 0,
                found: 0,
                applied: 1
            }
        );
    }
}
