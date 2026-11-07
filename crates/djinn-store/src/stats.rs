//! What's been played and asked for, for Admin > Stats: the party's plays
//! and requests and DJINN GO's plays, over a time range and optionally for
//! one person, with scoped clearing.

use serde::Serialize;
use sqlx::FromRow;

use crate::{Result, Store};

/// Which events count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsFilter {
    /// Unix ms, inclusive.
    pub from: i64,
    /// Unix ms, exclusive.
    pub until: i64,
    /// Only this person's: requests they made, party songs they asked for,
    /// and their GO plays.
    pub user: Option<i64>,
    pub party: bool,
    pub go: bool,
}

impl Default for StatsFilter {
    fn default() -> Self {
        Self {
            from: 0,
            until: i64::MAX,
            user: None,
            party: true,
            go: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, FromRow)]
pub struct StatsSummary {
    pub plays: i64,
    /// Time listened, in ms.
    pub listened_ms: i64,
    pub skips: i64,
    /// Different songs played.
    pub songs: i64,
    pub requests: i64,
    /// Different people who asked for songs.
    pub requesters: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct StatSong {
    pub track_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album_id: Option<i64>,
    pub plays: i64,
    pub skips: i64,
    pub listened_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct StatArtist {
    pub artist_id: i64,
    pub name: String,
    pub plays: i64,
    pub skips: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct StatPerson {
    pub user_id: Option<i64>,
    pub name: Option<String>,
    pub requests: i64,
}

/// Songs asked for most.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct StatRequested {
    pub track_id: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub requests: i64,
}

/// How much clearing with a filter would remove.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HistoryCounts {
    pub party_plays: i64,
    pub go_plays: i64,
    pub requests: i64,
}

/// Every play matching the filter: `?1` party, `?2` GO, `?3` from, `?4`
/// until, `?5` person (or NULL).
macro_rules! events {
    () => {
        "WITH ev AS (
            SELECT p.track_id, p.started_at AS at, coalesce(p.played_ms, 0) AS ms,
                   p.skipped, p.requested_by AS user_id
            FROM play_events p
            WHERE ?1 AND p.started_at >= ?3 AND p.started_at < ?4
              AND (?5 IS NULL OR p.requested_by = ?5)
            UNION ALL
            SELECT g.track_id, g.at, g.ms_played, g.skipped, g.user_id
            FROM go_plays g
            WHERE ?2 AND g.at >= ?3 AND g.at < ?4 AND (?5 IS NULL OR g.user_id = ?5)
        ) "
    };
}

/// Requests matching the filter, with the same numbered parameters.
macro_rules! requests_where {
    () => {
        " WHERE ?1 AND r.at >= ?3 AND r.at < ?4 AND (?5 IS NULL OR r.user_id = ?5) "
    };
}

macro_rules! song_rows {
    () => {
        "SELECT ev.track_id, t.title, t.artist, t.album_id, count(*) AS plays,
                sum(ev.skipped) AS skips, sum(ev.ms) AS listened_ms
         FROM ev JOIN tracks t ON t.id = ev.track_id GROUP BY ev.track_id "
    };
}

impl StatsFilter {
    fn bind<'q, O>(
        &self,
        query: sqlx::query::QueryAs<'q, sqlx::Sqlite, O, sqlx::sqlite::SqliteArguments>,
    ) -> sqlx::query::QueryAs<'q, sqlx::Sqlite, O, sqlx::sqlite::SqliteArguments> {
        query
            .bind(self.party)
            .bind(self.go)
            .bind(self.from)
            .bind(self.until)
            .bind(self.user)
    }
}

impl Store {
    pub async fn stats_summary(&self, filter: &StatsFilter) -> Result<StatsSummary> {
        let (plays, listened_ms, skips, songs): (i64, i64, i64, i64) = filter
            .bind(sqlx::query_as(concat!(
                events!(),
                "SELECT count(*), coalesce(sum(ms), 0), coalesce(sum(skipped), 0),
                        count(DISTINCT track_id) FROM ev"
            )))
            .fetch_one(&self.pool)
            .await?;
        let (requests, requesters): (i64, i64) = filter
            .bind(sqlx::query_as(concat!(
                "SELECT count(*), count(DISTINCT coalesce(CAST(r.user_id AS TEXT), r.name, r.mac, r.ip))
                 FROM request_events r",
                requests_where!()
            )))
            .fetch_one(&self.pool)
            .await?;
        Ok(StatsSummary {
            plays,
            listened_ms,
            skips,
            songs,
            requests,
            requesters,
        })
    }

    /// Songs played most.
    pub async fn top_songs(&self, filter: &StatsFilter, limit: i64) -> Result<Vec<StatSong>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                events!(),
                song_rows!(),
                "ORDER BY plays DESC, listened_ms DESC LIMIT ?6"
            )))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Songs skipped most for how often they played (at least three plays).
    pub async fn most_skipped(&self, filter: &StatsFilter, limit: i64) -> Result<Vec<StatSong>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                events!(),
                song_rows!(),
                "HAVING plays >= 3 AND skips > 0
                 ORDER BY skips * 1.0 / plays DESC, plays DESC LIMIT ?6"
            )))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn top_artists(&self, filter: &StatsFilter, limit: i64) -> Result<Vec<StatArtist>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                events!(),
                "SELECT ar.id AS artist_id, ar.name, count(*) AS plays, sum(ev.skipped) AS skips
                 FROM ev JOIN tracks t ON t.id = ev.track_id JOIN artists ar ON ar.id = t.artist_id
                 GROUP BY ar.id ORDER BY plays DESC LIMIT ?6"
            )))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Songs asked for most at the party.
    pub async fn top_requested(
        &self,
        filter: &StatsFilter,
        limit: i64,
    ) -> Result<Vec<StatRequested>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                "SELECT r.track_id, t.title, t.artist, count(*) AS requests
                 FROM request_events r LEFT JOIN tracks t ON t.id = r.track_id",
                requests_where!(),
                "GROUP BY r.track_id ORDER BY requests DESC LIMIT ?6"
            )))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Who asked for the most songs. Guests without accounts go by the
    /// name they gave.
    pub async fn top_requesters(
        &self,
        filter: &StatsFilter,
        limit: i64,
    ) -> Result<Vec<StatPerson>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                "SELECT r.user_id, coalesce(u.display_name, u.username, max(r.name)) AS name,
                        count(*) AS requests
                 FROM request_events r LEFT JOIN users u ON u.id = r.user_id",
                requests_where!(),
                "GROUP BY r.user_id, CASE WHEN r.user_id IS NULL THEN r.name END
                 ORDER BY requests DESC LIMIT ?6"
            )))
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Plays by hour of the day (0 to 23, this computer's time).
    pub async fn plays_by_hour(&self, filter: &StatsFilter) -> Result<Vec<(i64, i64)>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                events!(),
                "SELECT CAST(strftime('%H', at / 1000, 'unixepoch', 'localtime') AS INTEGER) AS h,
                        count(*) FROM ev GROUP BY h ORDER BY h"
            )))
            .fetch_all(&self.pool)
            .await?)
    }

    /// Plays by day of the week (0 is Sunday).
    pub async fn plays_by_weekday(&self, filter: &StatsFilter) -> Result<Vec<(i64, i64)>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                events!(),
                "SELECT CAST(strftime('%w', at / 1000, 'unixepoch', 'localtime') AS INTEGER) AS d,
                        count(*) FROM ev GROUP BY d ORDER BY d"
            )))
            .fetch_all(&self.pool)
            .await?)
    }

    /// Plays per day ("2026-11-06"), oldest first.
    pub async fn plays_by_day(&self, filter: &StatsFilter) -> Result<Vec<(String, i64)>> {
        Ok(filter
            .bind(sqlx::query_as(concat!(
                events!(),
                "SELECT date(at / 1000, 'unixepoch', 'localtime') AS d, count(*)
                 FROM ev GROUP BY d ORDER BY d"
            )))
            .fetch_all(&self.pool)
            .await?)
    }

    /// When the latest stretch of party music began: the first play after
    /// a break of at least `gap_ms` with nothing playing. `None` if nothing
    /// has played.
    pub async fn last_party_start(&self, gap_ms: i64) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT min(started_at) FROM play_events
             WHERE started_at > coalesce(
                 (SELECT p.started_at FROM play_events p
                  WHERE (SELECT min(n.started_at) FROM play_events n
                         WHERE n.started_at > p.started_at) - coalesce(p.ended_at, p.started_at) >= ?
                  ORDER BY p.started_at DESC LIMIT 1), -1)",
        )
        .bind(gap_ms)
        .fetch_one(&self.pool)
        .await?)
    }

    /// How much [`Store::clear_history_matching`] would remove.
    pub async fn history_counts(&self, filter: &StatsFilter) -> Result<HistoryCounts> {
        let (party_plays, go_plays, requests): (i64, i64, i64) = filter
            .bind(sqlx::query_as(
                "SELECT
                    (SELECT count(*) FROM play_events p WHERE ?1 AND p.started_at >= ?3
                        AND p.started_at < ?4 AND (?5 IS NULL OR p.requested_by = ?5)),
                    (SELECT count(*) FROM go_plays g WHERE ?2 AND g.at >= ?3 AND g.at < ?4
                        AND (?5 IS NULL OR g.user_id = ?5)),
                    (SELECT count(*) FROM request_events r WHERE ?1 AND r.at >= ?3 AND r.at < ?4
                        AND (?5 IS NULL OR r.user_id = ?5))",
            ))
            .fetch_one(&self.pool)
            .await?;
        Ok(HistoryCounts {
            party_plays,
            go_plays,
            requests,
        })
    }

    /// Forgets the plays and requests the filter covers. What the auto mix
    /// learned from them goes too, since it's worked out from them.
    pub async fn clear_history_matching(&self, filter: &StatsFilter) -> Result<HistoryCounts> {
        let mut tx = self.pool.begin().await?;
        let party = sqlx::query(
            "DELETE FROM play_events WHERE ? AND started_at >= ? AND started_at < ?
             AND (?4 IS NULL OR requested_by = ?4)",
        )
        .bind(filter.party)
        .bind(filter.from)
        .bind(filter.until)
        .bind(filter.user)
        .execute(&mut *tx)
        .await?;
        let go = sqlx::query(
            "DELETE FROM go_plays WHERE ? AND at >= ? AND at < ? AND (?4 IS NULL OR user_id = ?4)",
        )
        .bind(filter.go)
        .bind(filter.from)
        .bind(filter.until)
        .bind(filter.user)
        .execute(&mut *tx)
        .await?;
        let requests = sqlx::query(
            "DELETE FROM request_events WHERE ? AND at >= ? AND at < ?
             AND (?4 IS NULL OR user_id = ?4)",
        )
        .bind(filter.party)
        .bind(filter.from)
        .bind(filter.until)
        .bind(filter.user)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let n = |done: sqlx::sqlite::SqliteQueryResult| {
            i64::try_from(done.rows_affected()).unwrap_or(0)
        };
        Ok(HistoryCounts {
            party_plays: n(party),
            go_plays: n(go),
            requests: n(requests),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, ScannedTrack};

    async fn setup() -> (Store, Vec<i64>, i64, i64) {
        let store = Store::open_in_memory().await.unwrap();
        let song = |path: &str, title: &str, artist: &str| ScannedTrack {
            path: path.into(),
            title: title.into(),
            artist: Some(artist.into()),
            ..Default::default()
        };
        store
            .save_scanned(&[
                song("/m/a.mp3", "A", "Ann"),
                song("/m/b.mp3", "B", "Bob"),
                song("/m/c.mp3", "C", "Ann"),
            ])
            .await
            .unwrap();
        let ids = store.library_track_ids().await.unwrap();
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        let kim = store.create_user("kim", "h", Role::User).await.unwrap().id;
        let play = |track: i64, at: i64, user: Option<i64>, skipped: bool| {
            let store = store.clone();
            async move {
                sqlx::query(
                    "INSERT INTO play_events (track_id, source, requested_by, started_at, ended_at,
                        played_ms, skipped) VALUES (?, ?, ?, ?, ?, 60000, ?)",
                )
                .bind(track)
                .bind(if user.is_some() { "request" } else { "pool" })
                .bind(user)
                .bind(at)
                .bind(at + 60_000)
                .bind(skipped)
                .execute(&store.pool)
                .await
                .unwrap();
            }
        };
        let hour = 3_600_000;
        // An old party, a long break, then tonight.
        play(ids[0], hour, Some(sam), false).await;
        play(ids[1], 2 * hour, None, true).await;
        play(ids[0], 100 * hour, Some(sam), false).await;
        play(ids[0], 100 * hour + 120_000, None, false).await;
        play(ids[1], 100 * hour + 240_000, Some(kim), true).await;
        play(ids[1], 100 * hour + 360_000, None, true).await;
        play(ids[1], 100 * hour + 480_000, None, false).await;
        for (track, user, at) in [
            (ids[0], sam, hour),
            (ids[0], sam, 100 * hour),
            (ids[1], kim, 100 * hour),
        ] {
            sqlx::query("INSERT INTO request_events (track_id, user_id, at) VALUES (?, ?, ?)")
                .bind(track)
                .bind(user)
                .bind(at)
                .execute(&store.pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO go_plays (user_id, track_id, at, ms_played) VALUES (?, ?, ?, 30000)",
        )
        .bind(kim)
        .bind(ids[2])
        .bind(100 * hour)
        .execute(&store.pool)
        .await
        .unwrap();
        (store, ids, sam, kim)
    }

    #[tokio::test]
    async fn counts_plays_requests_and_people() {
        let (store, ids, sam, kim) = setup().await;
        let all = StatsFilter::default();
        let summary = store.stats_summary(&all).await.unwrap();
        assert_eq!(
            (
                summary.plays,
                summary.skips,
                summary.songs,
                summary.requests,
                summary.requesters
            ),
            (8, 3, 3, 3, 2)
        );
        let top = store.top_songs(&all, 10).await.unwrap();
        assert_eq!((top[0].track_id, top[0].plays), (ids[1], 4));
        let skipped = store.most_skipped(&all, 10).await.unwrap();
        assert_eq!((skipped[0].track_id, skipped[0].skips), (ids[1], 3));
        let artists = store.top_artists(&all, 10).await.unwrap();
        // Ann (two songs) and Bob tie.
        assert_eq!(artists.iter().map(|a| a.plays).collect::<Vec<_>>(), [4, 4]);
        let people = store.top_requesters(&all, 10).await.unwrap();
        assert_eq!((people[0].user_id, people[0].requests), (Some(sam), 2));
        let requested = store.top_requested(&all, 10).await.unwrap();
        assert_eq!((requested[0].track_id, requested[0].requests), (ids[0], 2));

        // Just GO, just Kim.
        let go = StatsFilter {
            party: false,
            ..StatsFilter::default()
        };
        assert_eq!(store.stats_summary(&go).await.unwrap().plays, 1);
        let kims = StatsFilter {
            user: Some(kim),
            ..StatsFilter::default()
        };
        let summary = store.stats_summary(&kims).await.unwrap();
        assert_eq!((summary.plays, summary.requests), (2, 1));
        assert_eq!(
            store
                .plays_by_hour(&all)
                .await
                .unwrap()
                .iter()
                .map(|(_, n)| n)
                .sum::<i64>(),
            8
        );
        assert!(!store.plays_by_day(&all).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn finds_tonight_and_clears_what_the_filter_covers() {
        let (store, _, sam, _) = setup().await;
        let hour = 3_600_000;
        assert_eq!(
            store.last_party_start(6 * hour).await.unwrap(),
            Some(100 * hour)
        );

        let old = StatsFilter {
            until: 50 * hour,
            ..StatsFilter::default()
        };
        let counts = store.history_counts(&old).await.unwrap();
        assert_eq!(
            counts,
            HistoryCounts {
                party_plays: 2,
                go_plays: 0,
                requests: 1
            }
        );
        assert_eq!(store.clear_history_matching(&old).await.unwrap(), counts);
        assert_eq!(
            store
                .stats_summary(&StatsFilter::default())
                .await
                .unwrap()
                .plays,
            6
        );

        let sams = StatsFilter {
            user: Some(sam),
            ..StatsFilter::default()
        };
        assert_eq!(
            store.history_counts(&sams).await.unwrap(),
            HistoryCounts {
                party_plays: 1,
                go_plays: 0,
                requests: 1
            }
        );
    }
}
