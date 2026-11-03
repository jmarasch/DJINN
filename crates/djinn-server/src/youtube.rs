//! Reading a public YouTube or YouTube Music playlist through YouTube's
//! official Data API, when an admin has added an API key. Nothing else in
//! DJINN talks to YouTube.
//!
//! This goes one way. A request says which list and carries the key, and that
//! is all: no user agent, referrer or cookies, nothing about who asked, and no
//! following redirects to anywhere else. The key travels in a header so it
//! never ends up in a URL or a log. Links aren't kept, and only the list's id
//! is taken from them, so tracking codes like `si=` are never sent.

use djinn_library::playlist_file::{ImportedList, ImportedSong, ParsedImport};
use djinn_store::WantedSong;

/// Setting: the admin's YouTube Data API key.
pub const API_KEY: &str = "youtube_api_key";
pub const API: &str = "https://www.googleapis.com/youtube/v3";
/// The most YouTube sends at once.
const PAGE_SIZE: &str = "50";

/// Why a playlist couldn't be read, in words for whoever asked.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum YouTubeError {
    #[error("YouTube can't read that playlist: it may be private. Make it public or unlisted, or export it to a file")]
    NotPublic,
    #[error(
        "YouTube refused this server's API key. An admin can check it under Import a playlist"
    )]
    BadKey,
    #[error("this server has used up its YouTube allowance for today. Try again tomorrow, or export the playlist to a file")]
    OutOfQuota,
    #[error("that playlist has more than {0} songs")]
    TooLong(usize),
    #[error("couldn't read that playlist from YouTube")]
    Failed,
}

/// The list's id, if this text is one link to a YouTube playlist.
pub fn playlist_id(text: &str) -> Option<String> {
    let text = text.trim();
    if text.contains(char::is_whitespace) {
        return None;
    }
    let url = reqwest::Url::parse(text).ok()?;
    if !matches!(url.scheme(), "https" | "http") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    if !matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be"
    ) {
        return None;
    }
    let (_, id) = url.query_pairs().find(|(name, _)| name == "list")?;
    let valid = (2..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| id.into_owned())
}

/// A video's title and channel as a song. YouTube Music's own uploads have
/// channels like "Rihanna - Topic" and the plain song title; others are
/// mostly "Artist - Title (Official Video)".
fn song_from_video(title: &str, channel: Option<&str>) -> WantedSong {
    if let Some(artist) = channel.and_then(|c| c.strip_suffix(" - Topic")) {
        return WantedSong {
            title: title.trim().to_owned(),
            artist: Some(artist.trim().to_owned()),
            ..Default::default()
        };
    }
    for dash in [" - ", " \u{2013} ", " \u{2014} "] {
        if let Some((artist, song)) = title.split_once(dash) {
            return WantedSong {
                title: song.trim().to_owned(),
                artist: Some(artist.trim().to_owned()),
                either_way: true,
                ..Default::default()
            };
        }
    }
    let artist = channel.map(|c| c.strip_suffix("VEVO").unwrap_or(c).trim().to_owned());
    WantedSong {
        title: title.trim().to_owned(),
        artist,
        ..Default::default()
    }
}

/// One page of a playlist: its songs, how many videos can't be seen (private
/// or deleted, which have no channel), and where the next page is.
fn read_page(page: &serde_json::Value) -> (Vec<ImportedSong>, usize, Option<String>) {
    let items = page.get("items").and_then(|i| i.as_array());
    let (mut songs, mut hidden) = (Vec::new(), 0);
    for snippet in items
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("snippet"))
    {
        let title = snippet.get("title").and_then(|t| t.as_str());
        let channel = snippet
            .get("videoOwnerChannelTitle")
            .and_then(|c| c.as_str());
        match (title, channel) {
            (Some(title), Some(channel)) => songs.push(ImportedSong {
                song: song_from_video(title, Some(channel)),
                video_id: None,
            }),
            _ => hidden += 1,
        }
    }
    let next = page
        .get("nextPageToken")
        .and_then(|t| t.as_str())
        .map(str::to_owned);
    (songs, hidden, next)
}

/// Asks YouTube for something, saying no more than `query` and the key.
async fn ask(
    client: &reqwest::Client,
    url: String,
    key: &str,
    query: &[(&str, &str)],
) -> Result<serde_json::Value, YouTubeError> {
    let response = client
        .get(url)
        .query(query)
        .header("X-Goog-Api-Key", key)
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e.without_url(), "YouTube request failed");
            YouTubeError::Failed
        })?;
    let status = response.status().as_u16();
    let body: serde_json::Value = response
        .text()
        .await
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if (200..300).contains(&status) {
        return Ok(body);
    }
    let reason = body
        .pointer("/error/errors/0/reason")
        .and_then(|r| r.as_str())
        .unwrap_or_default();
    tracing::warn!(status, reason, "YouTube said no");
    Err(match (status, reason) {
        (403, "quotaExceeded" | "dailyLimitExceeded" | "rateLimitExceeded") => {
            YouTubeError::OutOfQuota
        }
        (400 | 401, _)
        | (
            403,
            "accessNotConfigured" | "forbidden" | "ipRefererBlocked" | "keyExpired" | "keyInvalid",
        ) => YouTubeError::BadKey,
        // Liked Music answers playlistForbidden, private lists something like it.
        (403 | 404, _) => YouTubeError::NotPublic,
        _ => YouTubeError::Failed,
    })
}

/// Reads a playlist's name and songs. `api` is where YouTube's API is, which
/// tests point elsewhere.
pub async fn read_playlist(
    api: &str,
    key: &str,
    id: &str,
    max_songs: usize,
) -> Result<ParsedImport, YouTubeError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .build()
        .map_err(|_| YouTubeError::Failed)?;

    let about = ask(
        &client,
        format!("{api}/playlists"),
        key,
        &[("part", "snippet"), ("id", id)],
    )
    .await?;
    let name = about
        .pointer("/items/0/snippet/title")
        .and_then(|t| t.as_str())
        .map(str::to_owned);
    // An unknown list comes back as no items, not an error.
    if name.is_none() {
        return Err(YouTubeError::NotPublic);
    }

    let (mut songs, mut skipped, mut token) = (Vec::new(), 0, None::<String>);
    loop {
        let mut query = vec![
            ("part", "snippet"),
            ("maxResults", PAGE_SIZE),
            ("playlistId", id),
        ];
        if let Some(token) = &token {
            query.push(("pageToken", token));
        }
        let page = ask(&client, format!("{api}/playlistItems"), key, &query).await?;
        let (found, hidden, next) = read_page(&page);
        songs.extend(found);
        skipped += hidden;
        if songs.len() > max_songs {
            return Err(YouTubeError::TooLong(max_songs));
        }
        match next {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    Ok(ParsedImport {
        lists: vec![ImportedList { name, songs }],
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn links_give_only_the_lists_id() {
        const LIST: &str = "PLsN_-xOmuMt8Ao48yVuSCzZXIrLxDv_uw";
        for link in [
            "https://music.youtube.com/playlist?list=PLsN_-xOmuMt8Ao48yVuSCzZXIrLxDv_uw&si=L3ujELnAD0FFpS69",
            "https://www.youtube.com/playlist?list=PLsN_-xOmuMt8Ao48yVuSCzZXIrLxDv_uw",
            "  https://youtube.com/watch?v=abc&list=PLsN_-xOmuMt8Ao48yVuSCzZXIrLxDv_uw\n",
            "https://youtu.be/abc?list=PLsN_-xOmuMt8Ao48yVuSCzZXIrLxDv_uw&si=x",
        ] {
            assert_eq!(playlist_id(link).as_deref(), Some(LIST), "{link}");
        }
        for not in [
            "https://open.spotify.com/playlist/3N39yn1USEz2d3WNZmrPFl",
            "https://youtube.com.evil.test/playlist?list=PL123",
            "https://evil.test/?u=youtube.com&list=PL123",
            "https://www.youtube.com/watch?v=abc",
            "https://www.youtube.com/playlist?list=../../etc",
            "ftp://youtube.com/playlist?list=PL123",
            "Rihanna - S&M",
            "https://youtube.com/playlist?list=PL1\nhttps://youtube.com/playlist?list=PL2",
        ] {
            assert_eq!(playlist_id(not), None, "{not}");
        }
    }

    #[test]
    fn video_titles_become_songs() {
        let topic = song_from_video("Numb", Some("Marshmello - Topic"));
        assert_eq!(topic.title, "Numb");
        assert_eq!(topic.artist.as_deref(), Some("Marshmello"));
        assert!(!topic.either_way);

        let official = song_from_video(
            "Rick Astley - Never Gonna Give You Up (Official Video)",
            Some("Rick Astley"),
        );
        assert_eq!(official.artist.as_deref(), Some("Rick Astley"));
        assert_eq!(official.title, "Never Gonna Give You Up (Official Video)");
        assert!(official.either_way);

        let vevo = song_from_video("Umbrella", Some("RihannaVEVO"));
        assert_eq!(vevo.artist.as_deref(), Some("Rihanna"));
    }

    /// A stand-in for YouTube that answers from a list and keeps every
    /// request exactly as it arrived.
    async fn stand_in(answers: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            for (status, body) in answers {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                }
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nLocation: http://127.0.0.1:9/elsewhere\r\n\
                     Set-Cookie: tracked=1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}"), seen)
    }

    fn about() -> String {
        serde_json::json!({ "items": [{ "snippet": { "title": "EDM" } }] }).to_string()
    }

    fn item(title: &str, channel: Option<&str>) -> serde_json::Value {
        let mut snippet = serde_json::json!({ "title": title });
        if let Some(channel) = channel {
            snippet["videoOwnerChannelTitle"] = channel.into();
        }
        serde_json::json!({ "snippet": snippet })
    }

    #[tokio::test]
    async fn it_says_only_which_list_and_the_key() {
        let first = serde_json::json!({
            "nextPageToken": "NEXT",
            "items": [item("S&M", Some("Rihanna - Topic")), item("Private video", None)],
        });
        let second = serde_json::json!({ "items": [item("Numb", Some("Marshmello - Topic"))] });
        let (api, seen) = stand_in(vec![
            (200, about()),
            (200, first.to_string()),
            (200, second.to_string()),
        ])
        .await;

        let parsed = read_playlist(&api, "SECRET", "PL123", 100).await.unwrap();
        assert_eq!(parsed.lists[0].name.as_deref(), Some("EDM"));
        let titles: Vec<_> = parsed.lists[0]
            .songs
            .iter()
            .map(|s| s.song.title.as_str())
            .collect();
        assert_eq!(titles, ["S&M", "Numb"]);
        assert_eq!(parsed.skipped, 1, "the private video");

        let seen = seen.lock().unwrap().clone();
        let lines: Vec<&str> = seen.iter().map(|r| r.lines().next().unwrap()).collect();
        assert_eq!(
            lines,
            [
                "GET /playlists?part=snippet&id=PL123 HTTP/1.1",
                "GET /playlistItems?part=snippet&maxResults=50&playlistId=PL123 HTTP/1.1",
                "GET /playlistItems?part=snippet&maxResults=50&playlistId=PL123&pageToken=NEXT HTTP/1.1",
            ],
            "nothing in the address but the list and how to page through it"
        );
        for request in &seen {
            let mut headers: Vec<String> = request
                .lines()
                .skip(1)
                .filter(|l| !l.is_empty())
                .map(|l| l.split(':').next().unwrap().to_ascii_lowercase())
                .collect();
            headers.sort();
            assert_eq!(
                headers,
                ["accept", "host", "x-goog-api-key"],
                "no user agent, cookies or referrer: {request}"
            );
            assert!(request.contains("x-goog-api-key: SECRET"), "{request}");
            assert!(!request.lines().next().unwrap().contains("SECRET"));
        }
    }

    #[tokio::test]
    async fn it_doesnt_follow_redirects_or_keep_cookies() {
        let (api, seen) = stand_in(vec![(302, String::new()), (200, about())]).await;
        assert!(read_playlist(&api, "K", "PL1", 100).await.is_err());
        assert_eq!(seen.lock().unwrap().len(), 1, "stopped at the redirect");
    }

    #[tokio::test]
    async fn refusals_are_explained() {
        let refusal = |status: u16, reason: &str| {
            (
                status,
                serde_json::json!({ "error": { "errors": [{ "reason": reason }] } }).to_string(),
            )
        };
        let cases = [
            (refusal(403, "quotaExceeded"), YouTubeError::OutOfQuota),
            (refusal(400, "badRequest"), YouTubeError::BadKey),
            (refusal(403, "accessNotConfigured"), YouTubeError::BadKey),
            (refusal(404, "playlistNotFound"), YouTubeError::NotPublic),
            (refusal(403, "playlistForbidden"), YouTubeError::NotPublic),
            (
                refusal(403, "playlistItemsNotAccessible"),
                YouTubeError::NotPublic,
            ),
            (refusal(500, "backendError"), YouTubeError::Failed),
        ];
        for (answer, expected) in cases {
            let (api, _) = stand_in(vec![answer]).await;
            let error = read_playlist(&api, "K", "PL1", 100).await.unwrap_err();
            assert_eq!(error, expected);
        }
        // An unknown list is an empty answer rather than an error.
        let (api, _) = stand_in(vec![(200, r#"{"items":[]}"#.to_owned())]).await;
        let error = read_playlist(&api, "K", "PL1", 100).await.unwrap_err();
        assert_eq!(error, YouTubeError::NotPublic);
    }

    #[tokio::test]
    async fn a_list_too_long_stops_early() {
        let page = serde_json::json!({
            "nextPageToken": "MORE",
            "items": [item("A", Some("B - Topic")), item("C", Some("D - Topic"))],
        });
        let (api, seen) = stand_in(vec![(200, about()), (200, page.to_string())]).await;
        let error = read_playlist(&api, "K", "PL1", 1).await.unwrap_err();
        assert_eq!(error, YouTubeError::TooLong(1));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }
}
