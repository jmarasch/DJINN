//! How often songs, albums and artists were played and how many people
//! favorited them, for the library lists.

use super::*;
use std::collections::HashMap;

/// Plays count the party and GO alike, skipped or not, as the Stats page
/// does. Likes are people who favorited it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub plays: i64,
    pub likes: i64,
}

/// The ids as a JSON array, for `json_each(?)`.
fn id_list(ids: &[i64]) -> String {
    let inner: Vec<String> = ids.iter().map(i64::to_string).collect();
    format!("[{}]", inner.join(","))
}

impl Store {
    pub async fn track_counts(&self, ids: &[i64]) -> Result<HashMap<i64, Counts>> {
        self.counts(
            "SELECT j.value, \
               (SELECT count(*) FROM play_events p WHERE p.track_id = j.value) + \
               (SELECT count(*) FROM go_plays g WHERE g.track_id = j.value), \
               (SELECT count(*) FROM go_likes l WHERE l.kind = 'track' AND l.ref_id = j.value) \
             FROM json_each(?) j",
            ids,
        )
        .await
    }

    pub async fn album_counts(&self, ids: &[i64]) -> Result<HashMap<i64, Counts>> {
        self.counts(
            "SELECT j.value, \
               (SELECT count(*) FROM tracks t JOIN play_events p ON p.track_id = t.id \
                WHERE t.album_id = j.value) + \
               (SELECT count(*) FROM tracks t JOIN go_plays g ON g.track_id = t.id \
                WHERE t.album_id = j.value), \
               (SELECT count(*) FROM go_likes l WHERE l.kind = 'album' AND l.ref_id = j.value) \
             FROM json_each(?) j",
            ids,
        )
        .await
    }

    /// An artist's songs are the ones credited to them or on their albums,
    /// as the artist list counts them.
    pub async fn artist_counts(&self, ids: &[i64]) -> Result<HashMap<i64, Counts>> {
        self.counts(
            concat!(
                "SELECT j.value, \
                   (SELECT count(*) FROM play_events p WHERE p.track_id IN ",
                artist_songs!("j.value"),
                ") + (SELECT count(*) FROM go_plays g WHERE g.track_id IN ",
                artist_songs!("j.value"),
                "), (SELECT count(*) FROM go_likes l WHERE l.kind = 'artist' AND l.ref_id = j.value) \
                 FROM json_each(?) j"
            ),
            ids,
        )
        .await
    }

    async fn counts(&self, sql: &'static str, ids: &[i64]) -> Result<HashMap<i64, Counts>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(sql)
            .bind(id_list(ids))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, plays, likes)| (id, Counts { plays, likes }))
            .collect())
    }
}
