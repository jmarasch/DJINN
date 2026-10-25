//! Artist photos and bios from the internet (opt-in): photos from Deezer's
//! public API, bios from Wikipedia. Neither needs an API key. Only artist
//! names are sent.
//!
//! Matching is deliberately strict: a wrong photo or someone else's bio is
//! worse than none. Photos need an exact name match on Deezer; bios need a
//! Wikipedia page titled after the artist whose description is about music.

use std::time::Duration;

use djinn_store::{now_ms, Store};
use serde::Deserialize;

use crate::{art, LibraryError};

const USER_AGENT: &str = concat!(
    "DJINN/",
    env!("CARGO_PKG_VERSION"),
    " (self-hosted party jukebox)"
);
/// Look artists up again after this long, in case they gained a page.
const RECHECK_AFTER_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Pause between artists, to be polite to both services.
const PAUSE: Duration = Duration::from_millis(600);
/// Longest wait honored when a service asks us to slow down.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(10);
/// Words in a Wikipedia description that mean the page is about a musician.
const MUSIC_WORDS: &[&str] = &[
    "music",
    "musician",
    "band",
    "singer",
    "rapper",
    "dj",
    "duo",
    "trio",
    "group",
    "producer",
    "composer",
    "songwriter",
    "vocalist",
    "guitarist",
    "pianist",
    "drummer",
    "orchestra",
    "ensemble",
    "choir",
    "record",
];

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct LookupSummary {
    /// Artists both services answered for.
    pub checked: usize,
    pub photos: usize,
    pub bios: usize,
}

/// Looks up artists not checked recently (or all, with `force`), then
/// refreshes which picture each artist shows.
pub async fn lookup_artists(
    store: &Store,
    art_dir: &std::path::Path,
    force: bool,
) -> Result<LookupSummary, LibraryError> {
    let client = client()?;
    let older_than = if force {
        i64::MAX
    } else {
        now_ms() - RECHECK_AFTER_MS
    };
    let artists = store.artists_for_info_lookup(older_than).await?;
    let mut summary = LookupSummary::default();
    for artist in artists {
        // Artists with a picture in their own folder don't need a photo.
        let photo = match artist.has_folder_art {
            true => Fetch::NotFound,
            false => deezer_photo(&client, &artist.name).await,
        };
        let bio = wikipedia_bio(&client, &artist.name).await;
        // Only a real answer from both counts as checked; after a failed
        // request the artist is tried again next time.
        let checked = !photo.failed() && !bio.failed();
        let photo = photo
            .found()
            .and_then(|bytes| art::store(art_dir, &bytes).ok());
        let (bio, bio_url) = bio.found().unzip();
        summary.checked += usize::from(checked);
        summary.photos += usize::from(photo.is_some());
        summary.bios += usize::from(bio.is_some());
        store
            .save_artist_info(
                artist.id,
                photo.as_deref(),
                bio.as_deref(),
                bio_url.as_deref(),
                checked,
            )
            .await?;
        tokio::time::sleep(PAUSE).await;
    }
    store.refresh_artwork().await?;
    Ok(summary)
}

/// The outcome of asking a service: found it, it has nothing, or the
/// request failed (worth retrying later).
enum Fetch<T> {
    Found(T),
    NotFound,
    Failed,
}

impl<T> Fetch<T> {
    fn failed(&self) -> bool {
        matches!(self, Fetch::Failed)
    }

    fn found(self) -> Option<T> {
        match self {
            Fetch::Found(value) => Some(value),
            _ => None,
        }
    }
}

fn client() -> Result<reqwest::Client, LibraryError> {
    // reqwest is built without a default TLS provider; use ring.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| LibraryError::Task(format!("http client: {e}")))
}

/// Sends a request, retrying once if the service is busy (429 or 5xx).
async fn send(request: reqwest::RequestBuilder) -> Option<reqwest::Response> {
    let retry = request.try_clone();
    let response = request.send().await.ok()?;
    let status = response.status();
    if status.is_success() {
        return Some(response);
    }
    if status != reqwest::StatusCode::TOO_MANY_REQUESTS && !status.is_server_error() {
        return None;
    }
    let wait = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .map_or(Duration::from_secs(2), Duration::from_secs)
        .min(MAX_RETRY_WAIT);
    tokio::time::sleep(wait).await;
    let response = retry?.send().await.ok()?;
    response.status().is_success().then_some(response)
}

async fn get_json<T: serde::de::DeserializeOwned>(request: reqwest::RequestBuilder) -> Option<T> {
    send(request).await?.json().await.ok()
}

#[derive(Deserialize)]
struct DeezerSearch {
    data: Vec<DeezerArtist>,
}

#[derive(Deserialize)]
struct DeezerArtist {
    name: String,
    nb_fan: u64,
    picture_big: String,
}

/// The photo of the best-known Deezer artist with exactly this name.
async fn deezer_photo(client: &reqwest::Client, name: &str) -> Fetch<Vec<u8>> {
    let search: Option<DeezerSearch> = get_json(
        client
            .get("https://api.deezer.com/search/artist")
            .query(&[("q", name), ("limit", "10")]),
    )
    .await;
    let Some(search) = search else {
        return Fetch::Failed;
    };
    let Some(url) = pick_deezer(&search.data, name) else {
        return Fetch::NotFound;
    };
    let Some(response) = send(client.get(url)).await else {
        return Fetch::Failed;
    };
    match response.bytes().await {
        Ok(bytes) if art::mime_type(&bytes).starts_with("image/") => Fetch::Found(bytes.to_vec()),
        Ok(_) => Fetch::NotFound,
        Err(_) => Fetch::Failed,
    }
}

fn pick_deezer<'a>(artists: &'a [DeezerArtist], name: &str) -> Option<&'a str> {
    let wanted = simplify(name);
    artists
        .iter()
        // An empty image id ("/artist//") is Deezer's placeholder.
        .filter(|a| simplify(&a.name) == wanted && !a.picture_big.contains("/artist//"))
        .max_by_key(|a| a.nb_fan)
        .map(|a| a.picture_big.as_str())
}

#[derive(Deserialize)]
struct WikiSearch {
    pages: Vec<WikiPage>,
}

#[derive(Deserialize)]
struct WikiPage {
    key: String,
    title: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct WikiSummary {
    #[serde(rename = "type")]
    kind: String,
    extract: String,
    content_urls: WikiUrls,
}

#[derive(Deserialize)]
struct WikiUrls {
    desktop: WikiUrl,
}

#[derive(Deserialize)]
struct WikiUrl {
    page: String,
}

/// The intro of the artist's English Wikipedia page, and its address.
async fn wikipedia_bio(client: &reqwest::Client, name: &str) -> Fetch<(String, String)> {
    let search: Option<WikiSearch> = get_json(
        client
            .get("https://en.wikipedia.org/w/rest.php/v1/search/title")
            .query(&[("q", name), ("limit", "10")]),
    )
    .await;
    let Some(search) = search else {
        return Fetch::Failed;
    };
    let Some(page) = pick_wikipedia(&search.pages, name) else {
        return Fetch::NotFound;
    };
    let summary: Option<WikiSummary> = get_json(client.get(format!(
        "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
        urlencode(&page.key)
    )))
    .await;
    match summary {
        Some(s) if s.kind == "standard" && !s.extract.is_empty() => {
            Fetch::Found((s.extract, s.content_urls.desktop.page))
        }
        Some(_) => Fetch::NotFound,
        None => Fetch::Failed,
    }
}

/// A page titled after the artist (optionally with a qualifier, like
/// "Punctual (duo)") whose description is about music.
fn pick_wikipedia<'a>(pages: &'a [WikiPage], name: &str) -> Option<&'a WikiPage> {
    let wanted = simplify(name);
    pages.iter().find(|page| {
        let base = page.title.split(" (").next().unwrap_or(&page.title);
        let about_music = page.description.as_deref().is_some_and(|d| {
            let d = d.to_lowercase();
            d.split(|c: char| !c.is_alphanumeric())
                .any(|word| MUSIC_WORDS.contains(&word))
        });
        simplify(base) == wanted && about_music
    })
}

/// Lowercase letters and digits only.
fn simplify(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Percent-encodes a Wikipedia page key for a URL path.
fn urlencode(key: &str) -> String {
    key.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'(' | b')' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deezer(name: &str, fans: u64, picture: &str) -> DeezerArtist {
        DeezerArtist {
            name: name.into(),
            nb_fan: fans,
            picture_big: picture.into(),
        }
    }

    fn page(title: &str, description: Option<&str>) -> WikiPage {
        WikiPage {
            key: title.replace(' ', "_"),
            title: title.into(),
            description: description.map(Into::into),
        }
    }

    #[test]
    fn deezer_picks_exact_name_with_most_fans() {
        let results = [
            deezer("Kream", 43, "https://x/artist/a/500x500.jpg"),
            deezer("KREAM", 9066, "https://x/artist/b/500x500.jpg"),
            deezer("Kream Band", 99999, "https://x/artist/c/500x500.jpg"),
            deezer("Kream", 100000, "https://x/artist//500x500-000000.jpg"),
        ];
        assert_eq!(
            pick_deezer(&results, "KREAM"),
            Some("https://x/artist/b/500x500.jpg")
        );
        assert_eq!(pick_deezer(&results, "Nobody"), None);
    }

    #[test]
    fn wikipedia_needs_matching_title_and_music() {
        let kream = [
            page("Kream", Some("Norwegian DJ duo")),
            page("Maxo Kream", Some("American rapper (born 1990)")),
        ];
        assert_eq!(
            pick_wikipedia(&kream, "KREAM").map(|p| p.title.as_str()),
            Some("Kream")
        );

        let punctual = [
            page(
                "Punctuality",
                Some("Doing something at or before a previously designated time"),
            ),
            page("Punctual (duo)", Some("British music production duo")),
        ];
        assert_eq!(
            pick_wikipedia(&punctual, "Punctual").map(|p| p.title.as_str()),
            Some("Punctual (duo)")
        );

        // A namesake who isn't a musician, or a musician with another name.
        let morten = [
            page("Morten", Some("Given name")),
            page("Morten Harket", Some("Norwegian singer (born 1959)")),
        ];
        assert!(pick_wikipedia(&morten, "MORTEN").is_none());
    }

    #[test]
    fn page_keys_are_encoded() {
        assert_eq!(urlencode("Punctual_(duo)"), "Punctual_(duo)");
        assert_eq!(urlencode("Björk"), "Bj%C3%B6rk");
        assert_eq!(urlencode("AC/DC"), "AC%2FDC");
    }
}
