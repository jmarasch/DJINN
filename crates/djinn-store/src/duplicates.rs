//! What finding copies of a song needs from the library: each playable
//! song's tags, file facts, identified recording and fingerprint, and the
//! pairs the admin said aren't copies.

use std::collections::HashSet;

use sqlx::FromRow;

use crate::{Result, Store};

/// A song as the duplicate finder sees it.
#[derive(Clone, Debug, Default, PartialEq, FromRow)]
pub struct DuplicateCandidate {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_id: Option<i64>,
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    pub size: i64,
    pub codec: Option<String>,
    pub sample_rate: Option<i64>,
    /// It or its album has a cover.
    pub has_art: bool,
    /// The MusicBrainz recording an applied identification named.
    pub recording: Option<String>,
    /// The start of its fingerprint, little-endian u32s, from sound
    /// analysis with `print_version`.
    pub print: Option<Vec<u8>>,
}

impl DuplicateCandidate {
    /// The fingerprint as numbers.
    pub fn print_values(&self) -> Option<Vec<u32>> {
        let bytes = self.print.as_deref()?;
        Some(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect(),
        )
    }
}

impl Store {
    /// Every playable song, with prints taken with `print_version`.
    pub async fn duplicate_candidates(
        &self,
        print_version: i64,
    ) -> Result<Vec<DuplicateCandidate>> {
        Ok(sqlx::query_as(
            "SELECT t.id, t.path, t.title, t.artist, t.album, t.album_id, t.year, t.duration_ms,
                    t.size, t.codec, t.sample_rate,
                    (t.art_hash IS NOT NULL OR a.art_hash IS NOT NULL
                        OR a.online_art_hash IS NOT NULL) AS has_art,
                    (SELECT ti.recording FROM track_ids ti
                     WHERE ti.track_id = t.id AND ti.status = 'applied') AS recording,
                    (SELECT s.print FROM track_sound s
                     WHERE s.track_id = t.id AND s.print_version = ?) AS print
             FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
             WHERE t.removed_at IS NULL
               AND NOT EXISTS (SELECT 1 FROM track_problems tp WHERE tp.track_id = t.id)
             ORDER BY t.id",
        )
        .bind(print_version)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Pairs (smaller id first) the admin said aren't copies.
    pub async fn dismissed_duplicates(&self) -> Result<HashSet<(i64, i64)>> {
        let pairs: Vec<(i64, i64)> = sqlx::query_as("SELECT a, b FROM duplicate_dismissed")
            .fetch_all(&self.pool)
            .await?;
        Ok(pairs.into_iter().collect())
    }

    /// Records that none of these songs are copies of each other.
    pub async fn dismiss_duplicates(&self, ids: &[i64]) -> Result<()> {
        let mut tx = self.write().await?;
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                if a == b {
                    continue;
                }
                sqlx::query("INSERT OR IGNORE INTO duplicate_dismissed (a, b) VALUES (?, ?)")
                    .bind(a.min(b))
                    .bind(a.max(b))
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Changes whenever the answer to "which songs are copies" might: the
    /// library, fingerprints, identifications or dismissals.
    pub async fn duplicates_version(&self) -> Result<(i64, i64, i64, i64)> {
        Ok(sqlx::query_as(
            "SELECT (SELECT version FROM browse_version),
                    (SELECT count(*) FROM track_sound WHERE print IS NOT NULL),
                    (SELECT count(*) FROM track_ids WHERE status = 'applied'),
                    (SELECT count(*) FROM duplicate_dismissed)
                    + (SELECT count(*) FROM track_problems)",
        )
        .fetch_one(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{ScannedTrack, Store};

    #[tokio::test]
    async fn candidates_carry_prints_and_dismissals_are_pairs() {
        let store = Store::open_in_memory().await.unwrap();
        let song = |path: &str| ScannedTrack {
            path: path.into(),
            title: "Song".into(),
            ..Default::default()
        };
        store
            .save_scanned(&[song("/m/a.flac"), song("/m/b.mp3"), song("/m/c.mp3")])
            .await
            .unwrap();
        let ids = store.library_track_ids().await.unwrap();
        store
            .save_analysis(ids[0], 1, 1, None, None, Some(&[1, 2, 3]))
            .await
            .unwrap();
        let before = store.duplicates_version().await.unwrap();
        let songs = store.duplicate_candidates(1).await.unwrap();
        assert_eq!(songs.len(), 3);
        assert_eq!(songs[0].print_values(), Some(vec![1, 2, 3]));
        assert_eq!(songs[1].print_values(), None);
        // A print of another version doesn't count.
        assert_eq!(store.duplicate_candidates(2).await.unwrap()[0].print, None);

        store
            .dismiss_duplicates(&[ids[2], ids[0], ids[1]])
            .await
            .unwrap();
        let dismissed = store.dismissed_duplicates().await.unwrap();
        assert_eq!(dismissed.len(), 3);
        assert!(dismissed.contains(&(ids[0], ids[2])));
        assert_ne!(store.duplicates_version().await.unwrap(), before);
    }
}
