//! What DJINN measures in each song: loudness (for ReplayGain on files
//! without tags) and sound features (for smart playlists), from one pass.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// Everything needed to work out a track's ReplayGain.
#[derive(Clone, Debug, Default, PartialEq, FromRow)]
pub struct GainInfo {
    pub rg_track_gain: Option<f64>,
    pub rg_track_peak: Option<f64>,
    pub rg_album_gain: Option<f64>,
    pub rg_album_peak: Option<f64>,
    pub loudness_lufs: Option<f64>,
    pub loudness_peak: Option<f64>,
    /// Measured loudness of the whole album (from its measured tracks).
    #[sqlx(skip)]
    pub album_lufs: Option<f64>,
    #[sqlx(skip)]
    pub album_peak: Option<f64>,
}

/// One song's sound features.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundRow {
    pub track_id: i64,
    pub album_id: Option<i64>,
    /// Credited artists.
    pub artist_ids: Vec<i64>,
    pub title: String,
    pub features: Vec<f32>,
}

/// What search needs to know about a song to match descriptions like
/// "80s hair metal" or "chill house".
#[derive(Clone, Debug, PartialEq, FromRow)]
pub struct TrackFacts {
    pub id: i64,
    /// The song's year, else its album's.
    pub year: Option<i64>,
    /// Genre words from the file's tag, its album, and its artists
    /// (MusicBrainz), comma separated, lowercase.
    pub genres: String,
    /// Times requested plus times played.
    pub popularity: i64,
}

/// An artist's genres, for similar artists.
#[derive(Clone, Debug, PartialEq, FromRow)]
pub struct ArtistGenres {
    pub id: i64,
    pub name: String,
    /// Comma separated, lowercase; empty if unknown.
    pub genres: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LoudnessProgress {
    /// Songs whose loudness is known (from tags or measured).
    pub known: i64,
    /// Songs still to measure.
    pub waiting: i64,
    /// Songs whose sound has been analyzed.
    pub analyzed: i64,
}

impl Store {
    /// Up to `limit` tracks to analyze: never analyzed with the current
    /// `version` of the features. As (id, path).
    pub async fn tracks_to_analyze(&self, version: i64, limit: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query_as(
            "SELECT id, path FROM tracks \
             WHERE removed_at IS NULL AND (features_version IS NULL OR features_version < ?) \
             ORDER BY id LIMIT ?",
        )
        .bind(version)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Records an analysis. `None`s mean there was nothing to measure, so
    /// the track isn't tried again until it changes.
    pub async fn save_analysis(
        &self,
        id: i64,
        version: i64,
        loudness: Option<(f64, f64)>,
        features: Option<(&[f32], f32)>,
    ) -> Result<()> {
        let blob = features.map(|(values, _)| {
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>()
        });
        sqlx::query(
            "UPDATE tracks SET loudness_lufs = ?, loudness_peak = ?, loudness_checked_at = ?, \
                    features = ?, bpm = ?, features_version = ? WHERE id = ?",
        )
        .bind(loudness.map(|l| l.0))
        .bind(loudness.map(|l| l.1))
        .bind(now_ms())
        .bind(blob)
        .bind(features.map(|(_, bpm)| bpm))
        .bind(version)
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Every playable song's features, for the similarity index.
    pub async fn sound_rows(&self, version: i64) -> Result<Vec<SoundRow>> {
        // id, album, features, title, credited artist ids (comma separated).
        type Row = (i64, Option<i64>, Vec<u8>, String, String);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT t.id, t.album_id, t.features, t.title, \
                    coalesce((SELECT group_concat(ta.artist_id) FROM track_artists ta WHERE ta.track_id = t.id), '') \
             FROM tracks t \
             WHERE t.removed_at IS NULL AND t.features IS NOT NULL AND t.features_version = ?",
        )
        .bind(version)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(track_id, album_id, blob, title, artists)| SoundRow {
                track_id,
                album_id,
                title,
                artist_ids: artists.split(',').filter_map(|a| a.parse().ok()).collect(),
                features: blob
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect(),
            })
            .collect())
    }

    /// Up to `limit` songs the AI model hasn't listened to with `version`,
    /// as (id, path).
    pub async fn tracks_to_hear(&self, version: i64, limit: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query_as(
            "SELECT id, path FROM tracks \
             WHERE removed_at IS NULL AND (heard_version IS NULL OR heard_version < ?) \
             ORDER BY id LIMIT ?",
        )
        .bind(version)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Records what the AI model heard; `None` if there was nothing.
    pub async fn save_heard(&self, id: i64, version: i64, heard: Option<&[f32]>) -> Result<()> {
        let blob = heard.map(|v| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>());
        sqlx::query("UPDATE tracks SET heard = ?, heard_version = ? WHERE id = ?")
            .bind(blob)
            .bind(version)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Every playable song the AI model heard with `version`.
    pub async fn heard_rows(&self, version: i64) -> Result<Vec<(i64, Vec<f32>)>> {
        let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT id, heard FROM tracks \
             WHERE removed_at IS NULL AND heard IS NOT NULL AND heard_version = ?",
        )
        .bind(version)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, blob)| {
                let v = blob
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect();
                (id, v)
            })
            .collect())
    }

    /// (heard, waiting) song counts for the AI model's `version`.
    pub async fn heard_progress(&self, version: i64) -> Result<(i64, i64)> {
        let counts = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE heard_version >= ?1), \
                    count(*) FILTER (WHERE heard_version IS NULL OR heard_version < ?1) \
             FROM tracks WHERE removed_at IS NULL",
        )
        .bind(version)
        .fetch_one(self.pool())
        .await?;
        Ok(counts)
    }

    /// Facts about every playable song, for descriptive search.
    pub async fn track_facts(&self) -> Result<Vec<TrackFacts>> {
        let rows = sqlx::query_as(
            "SELECT t.id, coalesce(t.year, al.year) AS year, \
                    lower(coalesce(t.genre, '') || ',' || coalesce(al.genres, '') || ',' || \
                      coalesce((SELECT group_concat(ar.genres, ',') FROM track_artists ta \
                                JOIN artists ar ON ar.id = ta.artist_id \
                                WHERE ta.track_id = t.id AND ar.genres IS NOT NULL), '')) AS genres, \
                    (SELECT count(*) FROM request_events r WHERE r.track_id = t.id) + \
                    (SELECT count(*) FROM play_events p WHERE p.track_id = t.id) AS popularity \
             FROM tracks t LEFT JOIN albums al ON al.id = t.album_id \
             WHERE t.removed_at IS NULL",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Every artist with playable songs and their genres.
    pub async fn artist_genres(&self) -> Result<Vec<ArtistGenres>> {
        let rows = sqlx::query_as(
            "SELECT ar.id, ar.name, lower(coalesce(ar.genres, '')) AS genres FROM artists ar \
             WHERE EXISTS (SELECT 1 FROM track_artists ta JOIN tracks t ON t.id = ta.track_id \
                           WHERE ta.artist_id = ar.id AND t.removed_at IS NULL)",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// How far measuring has got, `version` being the current features
    /// version.
    pub async fn loudness_progress(&self, version: i64) -> Result<LoudnessProgress> {
        let (known, waiting, analyzed): (i64, i64, i64) = sqlx::query_as(
            "SELECT \
                count(*) FILTER (WHERE rg_track_gain IS NOT NULL OR loudness_lufs IS NOT NULL), \
                count(*) FILTER (WHERE features_version IS NULL OR features_version < ?1), \
                count(*) FILTER (WHERE features_version >= ?1) \
             FROM tracks WHERE removed_at IS NULL",
        )
        .bind(version)
        .fetch_one(self.pool())
        .await?;
        Ok(LoudnessProgress {
            known,
            waiting,
            analyzed,
        })
    }

    pub async fn gain_info(&self, id: i64) -> Result<Option<GainInfo>> {
        let info: Option<GainInfo> = sqlx::query_as(
            "SELECT rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak, \
                    loudness_lufs, loudness_peak \
             FROM tracks WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        let Some(mut info) = info else {
            return Ok(None);
        };
        let album: Vec<(f64, Option<f64>, Option<i64>)> = sqlx::query_as(
            "SELECT t.loudness_lufs, t.loudness_peak, t.duration_ms FROM tracks t \
             JOIN tracks me ON me.id = ? AND me.album_id = t.album_id \
             WHERE t.removed_at IS NULL AND t.loudness_lufs IS NOT NULL",
        )
        .bind(id)
        .fetch_all(self.pool())
        .await?;
        // Loudness adds up as energy, weighted by how long each track is.
        let (energy, weight) = album.iter().fold((0.0, 0.0), |(e, w), (lufs, _, ms)| {
            let secs = ms.map_or(1.0, |ms| (ms as f64 / 1000.0).max(1.0));
            (e + secs * 10f64.powf(lufs / 10.0), w + secs)
        });
        if weight > 0.0 {
            info.album_lufs = Some(10.0 * (energy / weight).log10());
            info.album_peak = album.iter().filter_map(|a| a.1).reduce(f64::max);
        }
        Ok(Some(info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScannedTrack;

    fn track(path: &str, ms: i64) -> ScannedTrack {
        ScannedTrack {
            path: path.to_owned(),
            size: 1,
            mtime: 1,
            title: path.to_owned(),
            artist: Some("A".into()),
            album: Some("Album".into()),
            duration_ms: Some(ms),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn analysis_is_stored_and_redone_when_files_change() {
        let store = Store::open_in_memory().await.unwrap();
        let mut tagged = track("/m/tagged.flac", 60_000);
        tagged.replay_gain.track_gain = Some(-5.0);
        store
            .save_scanned(&[
                track("/m/a.flac", 60_000),
                track("/m/b.flac", 60_000),
                tagged,
            ])
            .await
            .unwrap();
        let todo = store.tracks_to_analyze(1, 10).await.unwrap();
        assert_eq!(todo.len(), 3, "every song gets analyzed");
        assert_eq!(
            store.loudness_progress(1).await.unwrap(),
            LoudnessProgress {
                known: 1,
                waiting: 3,
                analyzed: 0
            }
        );

        let features = [0.5f32, -1.25, 3.0];
        store
            .save_analysis(todo[0].0, 1, Some((-10.0, 0.9)), Some((&features, 128.0)))
            .await
            .unwrap();
        store
            .save_analysis(todo[1].0, 1, Some((-10.0, 0.7)), None)
            .await
            .unwrap();
        store.save_analysis(todo[2].0, 1, None, None).await.unwrap();
        assert!(store.tracks_to_analyze(1, 10).await.unwrap().is_empty());
        assert_eq!(
            store.tracks_to_analyze(2, 10).await.unwrap().len(),
            3,
            "a new version redoes all"
        );

        let info = store.gain_info(todo[0].0).await.unwrap().unwrap();
        assert_eq!(info.loudness_lufs, Some(-10.0));
        let album = info.album_lufs.unwrap();
        assert!((album + 10.0).abs() < 1e-9, "album {album}");
        assert_eq!(info.album_peak, Some(0.9));

        let rows = store.sound_rows(1).await.unwrap();
        assert_eq!(rows.len(), 1, "only songs with features");
        assert_eq!(rows[0].features, features);
        assert_eq!(rows[0].artist_ids.len(), 1);

        // What the AI heard is kept, and redone with the file too.
        store
            .save_heard(todo[0].0, 1, Some(&[0.6, 0.8]))
            .await
            .unwrap();
        assert_eq!(
            store.heard_rows(1).await.unwrap(),
            [(todo[0].0, vec![0.6, 0.8])]
        );
        assert_eq!(store.heard_progress(1).await.unwrap(), (1, 2));

        // A changed file is analyzed again.
        let mut changed = track("/m/a.flac", 60_000);
        changed.mtime = 2;
        store.save_scanned(&[changed]).await.unwrap();
        assert_eq!(store.tracks_to_analyze(1, 10).await.unwrap().len(), 1);
        assert_eq!(store.tracks_to_hear(1, 10).await.unwrap().len(), 3);
    }
}
