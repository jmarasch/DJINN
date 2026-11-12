//! How often songs, albums and artists were played and how many people
//! favorited them, for the library lists.

use super::*;
use std::collections::HashMap;

/// Plays count the party and GO alike, as the Stats page does, but only
/// ones that heard at least the given part of the song (`percent`, 0 to 100),
/// so a song started again for a moment isn't a play. A play or song whose
/// length isn't known counts. Likes are people who favorited it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub plays: i64,
    pub likes: i64,
}

/// The play heard enough of song `t`: `$ms` is its heard time, `?2` the percent.
macro_rules! heard {
    ($ms:literal) => {
        concat!(
            "(",
            $ms,
            " IS NULL OR t.duration_ms IS NULL OR t.duration_ms <= 0 OR ",
            $ms,
            " * 100 >= t.duration_ms * ?2)"
        )
    };
}

impl Store {
    pub async fn track_counts(&self, ids: &[i64], percent: i64) -> Result<HashMap<i64, Counts>> {
        self.counts(
            concat!(
                "SELECT j.value, \
                   (SELECT count(*) FROM play_events p JOIN tracks t ON t.id = p.track_id \
                    WHERE p.track_id = j.value AND ",
                heard!("p.played_ms"),
                ") + (SELECT count(*) FROM go_plays g JOIN tracks t ON t.id = g.track_id \
                    WHERE g.track_id = j.value AND ",
                heard!("g.ms_played"),
                "), (SELECT count(*) FROM go_likes l WHERE l.kind = 'track' AND l.ref_id = j.value) \
                 FROM json_each(?1) j"
            ),
            ids,
            percent,
        )
        .await
    }

    pub async fn album_counts(&self, ids: &[i64], percent: i64) -> Result<HashMap<i64, Counts>> {
        self.counts(
            concat!(
                "SELECT j.value, \
                   (SELECT count(*) FROM tracks t JOIN play_events p ON p.track_id = t.id \
                    WHERE t.album_id = j.value AND ",
                heard!("p.played_ms"),
                ") + (SELECT count(*) FROM tracks t JOIN go_plays g ON g.track_id = t.id \
                    WHERE t.album_id = j.value AND ",
                heard!("g.ms_played"),
                "), (SELECT count(*) FROM go_likes l WHERE l.kind = 'album' AND l.ref_id = j.value) \
                 FROM json_each(?1) j"
            ),
            ids,
            percent,
        )
        .await
    }

    /// An artist's songs are the ones credited to them or on their albums,
    /// as the artist list counts them.
    pub async fn artist_counts(&self, ids: &[i64], percent: i64) -> Result<HashMap<i64, Counts>> {
        self.counts(
            concat!(
                "SELECT j.value, \
                   (SELECT count(*) FROM play_events p JOIN tracks t ON t.id = p.track_id \
                    WHERE p.track_id IN ",
                artist_songs!("j.value"),
                " AND ",
                heard!("p.played_ms"),
                ") + (SELECT count(*) FROM go_plays g JOIN tracks t ON t.id = g.track_id \
                    WHERE g.track_id IN ",
                artist_songs!("j.value"),
                " AND ",
                heard!("g.ms_played"),
                "), (SELECT count(*) FROM go_likes l WHERE l.kind = 'artist' AND l.ref_id = j.value) \
                 FROM json_each(?1) j"
            ),
            ids,
            percent,
        )
        .await
    }

    async fn counts(
        &self,
        sql: &'static str,
        ids: &[i64],
        percent: i64,
    ) -> Result<HashMap<i64, Counts>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(sql)
            .bind(json_ids(ids))
            .bind(percent.clamp(0, 100))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, plays, likes)| (id, Counts { plays, likes }))
            .collect())
    }
}
