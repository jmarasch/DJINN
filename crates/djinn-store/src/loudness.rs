//! Track loudness for ReplayGain: values from tags, and what DJINN measured
//! for files without them.

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LoudnessProgress {
    /// Songs whose loudness is known (from tags or measured).
    pub known: i64,
    /// Songs still to measure.
    pub waiting: i64,
}

impl Store {
    /// Up to `limit` tracks without ReplayGain tags that haven't been
    /// measured, as (id, path).
    pub async fn tracks_to_measure(&self, limit: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query_as(
            "SELECT id, path FROM tracks \
             WHERE removed_at IS NULL AND rg_track_gain IS NULL AND loudness_checked_at IS NULL \
             ORDER BY id LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    /// Records a measurement; `None` marks the track as unmeasurable
    /// (silent or unreadable) so it isn't tried again until it changes.
    pub async fn save_loudness(&self, id: i64, loudness: Option<(f64, f64)>) -> Result<()> {
        sqlx::query(
            "UPDATE tracks SET loudness_lufs = ?, loudness_peak = ?, loudness_checked_at = ? \
             WHERE id = ?",
        )
        .bind(loudness.map(|l| l.0))
        .bind(loudness.map(|l| l.1))
        .bind(now_ms())
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn loudness_progress(&self) -> Result<LoudnessProgress> {
        let (known, waiting): (i64, i64) = sqlx::query_as(
            "SELECT \
                count(*) FILTER (WHERE rg_track_gain IS NOT NULL OR loudness_lufs IS NOT NULL), \
                count(*) FILTER (WHERE rg_track_gain IS NULL AND loudness_checked_at IS NULL) \
             FROM tracks WHERE removed_at IS NULL",
        )
        .fetch_one(self.pool())
        .await?;
        Ok(LoudnessProgress { known, waiting })
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
    async fn measured_loudness_and_album_average() {
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
        let todo = store.tracks_to_measure(10).await.unwrap();
        assert_eq!(todo.len(), 2, "tagged tracks need no measuring");
        assert_eq!(
            store.loudness_progress().await.unwrap(),
            LoudnessProgress {
                known: 1,
                waiting: 2
            }
        );

        store
            .save_loudness(todo[0].0, Some((-10.0, 0.9)))
            .await
            .unwrap();
        store
            .save_loudness(todo[1].0, Some((-10.0, 0.7)))
            .await
            .unwrap();
        assert!(store.tracks_to_measure(10).await.unwrap().is_empty());
        let info = store.gain_info(todo[0].0).await.unwrap().unwrap();
        assert_eq!(info.loudness_lufs, Some(-10.0));
        let album = info.album_lufs.unwrap();
        assert!((album + 10.0).abs() < 1e-9, "album {album}");
        assert_eq!(info.album_peak, Some(0.9));

        // A changed file is measured again.
        let mut changed = track("/m/a.flac", 60_000);
        changed.mtime = 2;
        store.save_scanned(&[changed]).await.unwrap();
        assert_eq!(store.tracks_to_measure(10).await.unwrap().len(), 1);
    }
}
