//! `GET /api/discover`: shelves to find something to play, the way a streaming
//! service's home page does: genres to dive into, what you played lately, new
//! albums, what's popular, artists like one you favorited, and a few to
//! explore. The party app, DJINN GO's web app and the Android app all show it.

use std::collections::{HashMap, HashSet};

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use djinn_store::{
    Album, AlbumFilter, AlbumSort, ArtistSort, ArtistSummary, LyricsFilter, Track, TrackSort,
};
use serde::Serialize;

use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::settings;
use crate::state::AppState;

/// Setting key: minutes before the shelves are drawn again; 0 draws them on every visit.
pub const REFRESH_MINUTES: &str = "discover_refresh_minutes";
pub const REFRESH_DEFAULT: i64 = 1;

/// Items on each shelf.
const SHELF: i64 = 14;

pub fn router() -> Router<AppState> {
    Router::new().route("/discover", get(discover))
}

#[derive(Serialize)]
struct GenreChip {
    name: String,
    songs: usize,
}

/// Artists like one you favorited.
#[derive(Serialize)]
struct Because {
    artist: ArtistSummary,
    similar: Vec<ArtistSummary>,
}

/// One of the speed dial's quick starts.
#[derive(Serialize)]
struct Dial {
    /// `artist` or `album`.
    kind: &'static str,
    id: i64,
    name: String,
    art_hash: Option<String>,
}

/// A playlist idea drawn from what you listen to: a mix around an artist or a genre.
#[derive(Serialize)]
struct Mix {
    /// `artist` or `genre`.
    kind: &'static str,
    /// The artist's id; 0 for a genre.
    id: i64,
    name: String,
    art_hash: Option<String>,
}

/// Shelves that come and go: each one is about something (a decade, a genre,
/// an artist, a song) and holds songs, albums or artists.
#[derive(Serialize)]
struct Shelf {
    /// `decade`, `genre`, `listened` or `like_song`.
    kind: &'static str,
    name: String,
    songs: Vec<Track>,
    albums: Vec<Album>,
    artists: Vec<ArtistSummary>,
}

/// A small shuffler: the same seed gives the same draws, so a page holds still
/// until the refresh time comes round.
struct Dice(u64);

impl Dice {
    fn new(seed: usize) -> Self {
        Self((u64::try_from(seed).unwrap_or(1) ^ 0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, len: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(len.max(1)).unwrap_or(1)).unwrap_or(0)
    }

    fn some<T>(&mut self, mut items: Vec<T>, count: usize) -> Vec<T> {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
        items.truncate(count);
        items
    }
}

#[derive(Serialize)]
struct Discover {
    /// Playlist ideas from what you listen to.
    mixes: Vec<Mix>,
    /// Shelves that change from visit to visit.
    more: Vec<Shelf>,
    /// Nine quick starts: your favorite artists and albums, then others to fill the grid.
    speed_dial: Vec<Dial>,
    /// Songs you favorited that you haven't played lately.
    forgotten: Vec<Track>,
    /// The biggest genres, to dive into.
    genres: Vec<GenreChip>,
    /// What you played lately (DJINN GO's history), newest first, each song once.
    listen_again: Vec<Track>,
    new_albums: Vec<Album>,
    /// The oldest albums, for a look back.
    throwbacks: Vec<Album>,
    popular_songs: Vec<Track>,
    because: Option<Because>,
    explore: Vec<ArtistSummary>,
}

async fn discover(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Discover>> {
    // The shelves change as often as the admin chose: every visit, or every so many minutes.
    let minutes = settings::number(&state.store, REFRESH_MINUTES, REFRESH_DEFAULT)
        .await?
        .clamp(0, 7 * 24 * 60);
    let seed = if minutes == 0 {
        // Every visit: whatever the clock's fine ticks say.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() % 1_000_000_007);
        usize::try_from(nanos).unwrap_or(1)
    } else {
        usize::try_from(djinn_store::now_ms() / (minutes * 60_000)).unwrap_or(0)
    };

    let genres = crate::smart::top_genres(&state, 18)
        .into_iter()
        .map(|(name, songs)| GenreChip { name, songs })
        .collect();

    // What you've played, a long way back, so one session doesn't fill the shelf.
    let history: Vec<Track> = state
        .store
        .go_history(user.id, 0, 600)
        .await?
        .into_iter()
        .map(|played| played.track)
        .collect();
    let mut dice = Dice::new(seed);
    let listen_again: Vec<Track> = {
        let mut seen = HashSet::new();
        let mut per_artist: HashMap<Option<i64>, usize> = HashMap::new();
        let spread: Vec<Track> = history
            .iter()
            .filter(|track| seen.insert(track.id))
            // At most two songs by anyone, so two big blocks don't take over.
            .filter(|track| {
                let count = per_artist.entry(track.artist_id).or_insert(0);
                *count += 1;
                *count <= 2
            })
            .cloned()
            .collect();
        dice.some(spread, usize::try_from(SHELF).unwrap_or(14))
    };

    let new_albums = state
        .store
        .albums_page_filtered(AlbumSort::Added, AlbumFilter::All, 0, SHELF)
        .await?
        .items;
    let throwbacks = state
        .store
        .albums_page_filtered(AlbumSort::Oldest, AlbumFilter::All, 0, SHELF)
        .await?
        .items;
    let popular_songs = state
        .store
        .tracks_page_filtered(TrackSort::Popular, LyricsFilter::All, 0, SHELF)
        .await?
        .items;

    // A different favorite artist each time, and the artists most like them.
    let liked = state.store.liked(user.id).await?;
    let because = match liked
        .artists
        .get(seed.checked_rem(liked.artists.len()).unwrap_or(0))
    {
        Some(artist) => {
            let ids: Vec<i64> = crate::smart::similar_artist_ids(&state, artist.id)
                .into_iter()
                .take(usize::try_from(SHELF).unwrap_or(14))
                .collect();
            let similar = state.store.artists_by_ids(&ids).await?;
            (!similar.is_empty()).then(|| Because {
                artist: artist.clone(),
                similar,
            })
        }
        None => None,
    };

    let show = settings::flag(&state.store, "show_collaboration_artists", true).await?;
    let artists = state
        .store
        .artists_page_filtered(ArtistSort::Name, show, 0, 0)
        .await?;
    let room = (artists.total - SHELF).max(1);
    let offset = i64::try_from(seed).unwrap_or(0).wrapping_mul(7919) % room;
    let explore = state
        .store
        .artists_page_filtered(ArtistSort::Name, show, offset, SHELF)
        .await?
        .items;

    // The speed dial: favorites first, a few at a time that change, then fillers.
    let mut speed_dial: Vec<Dial> = Vec::new();
    let rotate = |len: usize| seed.checked_rem(len).unwrap_or(0);
    for artist in liked
        .artists
        .iter()
        .cycle()
        .skip(rotate(liked.artists.len()))
        .take(liked.artists.len().min(4))
    {
        speed_dial.push(Dial {
            kind: "artist",
            id: artist.id,
            name: artist.name.clone(),
            art_hash: artist.art_hash.clone(),
        });
    }
    for album in liked
        .albums
        .iter()
        .cycle()
        .skip(rotate(liked.albums.len()))
        .take(liked.albums.len().min(3))
    {
        speed_dial.push(Dial {
            kind: "album",
            id: album.id,
            name: album.title.clone(),
            art_hash: album.art_hash.clone(),
        });
    }
    for album in &new_albums {
        if speed_dial.len() >= 9 {
            break;
        }
        if !speed_dial
            .iter()
            .any(|d| d.kind == "album" && d.id == album.id)
        {
            speed_dial.push(Dial {
                kind: "album",
                id: album.id,
                name: album.title.clone(),
                art_hash: album.art_hash.clone(),
            });
        }
    }

    // Favorites that haven't come up lately.
    let played: HashSet<i64> = history.iter().map(|t| t.id).collect();
    let candidates: Vec<&Track> = liked
        .tracks
        .iter()
        .filter(|t| !played.contains(&t.id))
        .collect();
    let forgotten: Vec<Track> = candidates
        .iter()
        .cycle()
        .skip(rotate(candidates.len()))
        .take(candidates.len().min(usize::try_from(SHELF).unwrap_or(14)))
        .map(|t| (*t).clone())
        .collect();

    // ---- What you listen to: ideas for what to play next ----
    let shelf_len = usize::try_from(SHELF).unwrap_or(14);
    let history_ids: Vec<i64> = history.iter().map(|t| t.id).collect();
    let genres_of = crate::smart::list_genres(&state, history_ids.iter().copied());
    // Plays per artist and per genre, and a song that shows each.
    let mut artist_plays: HashMap<i64, (usize, &Track)> = HashMap::new();
    let mut genre_plays: HashMap<String, (usize, &Track)> = HashMap::new();
    for track in &history {
        if let Some(artist) = track.artist_id {
            artist_plays.entry(artist).or_insert((0, track)).0 += 1;
        }
        for genre in genres_of.get(&track.id).into_iter().flatten().take(2) {
            genre_plays.entry(genre.clone()).or_insert((0, track)).0 += 1;
        }
    }
    let mut top_artists: Vec<(i64, (usize, &Track))> = artist_plays.into_iter().collect();
    top_artists.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    top_artists.truncate(10);
    let mut top_genre_plays: Vec<(String, (usize, &Track))> = genre_plays.into_iter().collect();
    top_genre_plays.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
    top_genre_plays.truncate(8);

    // Playlist ideas: a few artists and genres you keep coming back to.
    let mut mixes: Vec<Mix> = Vec::new();
    for (id, (_, track)) in dice.some(top_artists.clone(), 3) {
        mixes.push(Mix {
            kind: "artist",
            id,
            name: track.artist.clone().unwrap_or_default(),
            art_hash: track.art_hash.clone(),
        });
    }
    for (name, (_, track)) in dice.some(top_genre_plays.clone(), 3) {
        mixes.push(Mix {
            kind: "genre",
            id: 0,
            name,
            art_hash: track.art_hash.clone(),
        });
    }
    let mixes = dice.some(mixes, 6);

    let mut candidates_more: Vec<Shelf> = Vec::new();
    // A decade of albums.
    let years = state
        .store
        .albums_page_filtered(AlbumSort::Year, AlbumFilter::All, 0, 400)
        .await?
        .items;
    let mut by_decade: HashMap<i64, Vec<Album>> = HashMap::new();
    for album in years {
        if let Some(year) = album.year.filter(|y| *y > 1900) {
            by_decade.entry(year / 10 * 10).or_default().push(album);
        }
    }
    let mut decades: Vec<i64> = by_decade
        .iter()
        .filter(|(_, albums)| albums.len() >= 5)
        .map(|(decade, _)| *decade)
        .collect();
    decades.sort_unstable();
    if let Some(decade) = decades.get(dice.below(decades.len())).copied() {
        let albums = dice.some(by_decade.remove(&decade).unwrap_or_default(), shelf_len);
        candidates_more.push(Shelf {
            kind: "decade",
            name: format!("{decade}s"),
            songs: Vec::new(),
            albums,
            artists: Vec::new(),
        });
    }
    // More of a genre.
    let genre_pool = crate::smart::top_genres(&state, 20);
    if let Some((genre, _)) = genre_pool.get(dice.below(genre_pool.len())) {
        let ids = crate::smart::genre_song_ids(&state, genre, 80).await;
        let songs = state
            .store
            .tracks_by_ids(&dice.some(ids, shelf_len))
            .await?;
        if !songs.is_empty() {
            candidates_more.push(Shelf {
                kind: "genre",
                name: genre.clone(),
                songs,
                albums: Vec::new(),
                artists: Vec::new(),
            });
        }
    }
    // Artists like one you listen to.
    if let Some((id, (_, track))) = top_artists.get(dice.below(top_artists.len().min(6))) {
        let similar_ids: Vec<i64> = crate::smart::similar_artist_ids(&state, *id)
            .into_iter()
            .take(shelf_len)
            .collect();
        let artists = state.store.artists_by_ids(&similar_ids).await?;
        if !artists.is_empty() {
            candidates_more.push(Shelf {
                kind: "listened",
                name: track.artist.clone().unwrap_or_default(),
                songs: Vec::new(),
                albums: Vec::new(),
                artists,
            });
        }
    }
    // Songs like one you played.
    if let Some(track) = history.get(dice.below(history.len().min(40))) {
        let similar_ids = crate::smart::similar_track_ids(&state, track.id, shelf_len);
        let songs = state.store.tracks_by_ids(&similar_ids).await?;
        if !songs.is_empty() {
            candidates_more.push(Shelf {
                kind: "like_song",
                name: track.title.clone(),
                songs,
                albums: Vec::new(),
                artists: Vec::new(),
            });
        }
    }

    // A few of the shelves that come and go, a different few each time.
    let mut throwbacks = throwbacks;
    let mut because = because;
    let mut forgotten = forgotten;
    let mut roster: Vec<&str> = Vec::new();
    if !throwbacks.is_empty() {
        roster.push("throwbacks");
    }
    if because.is_some() {
        roster.push("because");
    }
    if !forgotten.is_empty() {
        roster.push("forgotten");
    }
    roster.extend(candidates_more.iter().map(|shelf| shelf.kind));
    let kept: HashSet<&str> = dice.some(roster, 5).into_iter().collect();
    if !kept.contains("throwbacks") {
        throwbacks.clear();
    }
    if !kept.contains("because") {
        because = None;
    }
    if !kept.contains("forgotten") {
        forgotten.clear();
    }
    candidates_more.retain(|shelf| kept.contains(shelf.kind));

    Ok(Json(Discover {
        mixes,
        more: candidates_more,
        speed_dial,
        forgotten,
        genres,
        listen_again,
        new_albums,
        throwbacks,
        popular_songs,
        because,
        explore,
    }))
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use crate::test_util::{get_body, session};
    use axum::http::StatusCode;
    use djinn_store::Role;

    #[tokio::test]
    async fn an_empty_library_has_empty_shelves() {
        let state = AppState::for_tests().await;
        let cookie = session(&state, "bo", Role::User).await;
        let (status, body) = get_body(&state, "/api/discover", &cookie).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let shelves: serde_json::Value = serde_json::from_str(&body).unwrap();
        for key in [
            "genres",
            "listen_again",
            "new_albums",
            "throwbacks",
            "popular_songs",
            "explore",
        ] {
            assert_eq!(shelves[key], serde_json::json!([]), "{key}");
        }
        assert!(shelves["because"].is_null());
        let (status, _) = get_body(&state, "/api/discover", "").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}
