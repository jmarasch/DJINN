//! `GET /api/discover`: shelves to find something to play, the way a streaming
//! service's home page does: genres to dive into, what you played lately, new
//! albums, what's popular, artists like one you favorited, and a few to
//! explore. The party app, DJINN GO's web app and the Android app all show it.

use std::collections::HashSet;

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

#[derive(Serialize)]
struct Discover {
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
    // Changes from visit to visit, without anyone having to ask.
    let seed = usize::try_from(djinn_store::now_ms() / 60_000).unwrap_or(0);

    let genres = crate::smart::top_genres(&state, 18)
        .into_iter()
        .map(|(name, songs)| GenreChip { name, songs })
        .collect();

    let mut seen = HashSet::new();
    let listen_again: Vec<Track> = state
        .store
        .go_history(user.id, 0, 60)
        .await?
        .into_iter()
        .map(|played| played.track)
        .filter(|track| seen.insert(track.id))
        .take(usize::try_from(SHELF).unwrap_or(14))
        .collect();

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
    let played: HashSet<i64> = state
        .store
        .go_history(user.id, 0, 200)
        .await?
        .into_iter()
        .map(|p| p.track.id)
        .collect();
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

    Ok(Json(Discover {
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
