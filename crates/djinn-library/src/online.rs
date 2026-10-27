//! Artist and album info from the internet (opt-in). Everything found is
//! downloaded into the local art cache; nothing is loaded from these
//! services when someone browses.
//!
//! - Deezer (no key): artist photos.
//! - Wikipedia (no key): artist bios.
//! - MusicBrainz (no key, but the host must give a contact, as its rules
//!   ask): exact artist and album identities and genres. Through its links
//!   to Wikidata: the artist's exact Wikipedia page, and a free-licensed
//!   photo on Wikimedia Commons when Deezer has none. Album covers come from
//!   the Cover Art Archive.
//!
//! Matching is deliberately strict: a wrong photo or someone else's bio is
//! worse than none. When several MusicBrainz artists share a name, one of
//! our own song titles must appear in their catalog.

use std::collections::HashMap;
use std::time::Duration;

use djinn_store::{ArtistInfo, Store};
use serde::Deserialize;
use tokio::time::Instant;

use crate::{art, LibraryError};

const USER_AGENT: &str = concat!(
    "DJINN/",
    env!("CARGO_PKG_VERSION"),
    " (self-hosted party jukebox)"
);
/// Pause between artists, to be polite to Deezer and Wikipedia.
const PAUSE: Duration = Duration::from_millis(600);
/// MusicBrainz allows one request per second.
const MUSICBRAINZ_INTERVAL: Duration = Duration::from_millis(1100);
/// Longest wait honored when a service asks us to slow down.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(10);
/// Genres kept per artist or album.
const MAX_GENRES: usize = 5;
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

#[derive(Clone, Debug, Default)]
pub struct LookupOptions {
    /// Look everything up again, not just new or stale entries.
    pub force: bool,
    /// Contact (email or website) sent to MusicBrainz. MusicBrainz lookups
    /// only run when this is set.
    pub musicbrainz_contact: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct LookupSummary {
    /// Artists every service answered for.
    pub checked: usize,
    pub photos: usize,
    pub bios: usize,
    /// Album covers found (MusicBrainz only).
    pub covers: usize,
    /// Artists and albums that got genres (MusicBrainz only).
    pub genres: usize,
}

/// Looks up artists (and, with MusicBrainz, albums) that haven't been
/// looked up yet (new ones, or ones whose lookup failed), or every one with
/// `force`. Then refreshes which pictures are shown.
pub async fn lookup_artists(
    store: &Store,
    art_dir: &std::path::Path,
    options: &LookupOptions,
) -> Result<LookupSummary, LibraryError> {
    let mut services = Services::new(options.musicbrainz_contact.as_deref())?;
    // Without `force`, only never-checked ones (no check time is older
    // than the oldest possible time).
    let older_than = if options.force { i64::MAX } else { i64::MIN };
    let mut summary = LookupSummary::default();

    for artist in store.artists_for_info_lookup(older_than).await? {
        let titles: Vec<&str> = artist.sample_titles.lines().collect();
        let mut failed = false;

        let mb = services.musicbrainz_artist(&artist.name, &titles).await;
        failed |= mb.failed();
        let mb = mb.found();
        let wikidata = match mb.as_ref().and_then(|a| a.wikidata.as_deref()) {
            Some(id) => {
                let found = services.wikidata(id).await;
                failed |= found.failed();
                found.found()
            }
            None => None,
        };

        let mut info = ArtistInfo::default();
        // Artists with a picture in their own folder don't need a photo.
        if !artist.has_folder_art {
            match services.deezer_photo(&artist.name).await {
                Fetch::Found(bytes) => info.photo = art::store(art_dir, &bytes).ok(),
                Fetch::NotFound => {
                    if let Some(file) = wikidata.as_ref().and_then(|w| w.image.as_deref()) {
                        let photo = services.commons_photo(file).await;
                        failed |= photo.failed();
                        if let Some(photo) = photo.found() {
                            info.photo = art::store(art_dir, &photo.bytes).ok();
                            info.photo_credit = Some(photo.credit);
                            info.photo_credit_url = Some(photo.page);
                        }
                    }
                }
                Fetch::Failed => failed = true,
            }
        }
        // MusicBrainz's link to the exact page beats searching by name.
        let bio = match wikidata.as_ref().and_then(|w| w.enwiki.as_deref()) {
            Some(title) => services.wikipedia_summary(title).await,
            None => services.wikipedia_bio(&artist.name).await,
        };
        failed |= bio.failed();
        (info.bio, info.bio_url) = bio.found().unzip();
        if let Some(mb) = mb {
            info.genres = mb.genres;
            info.mbid = Some(mb.mbid);
        }
        info.checked = !failed;

        summary.checked += usize::from(info.checked);
        summary.photos += usize::from(info.photo.is_some());
        summary.bios += usize::from(info.bio.is_some());
        summary.genres += usize::from(info.genres.is_some());
        store.save_artist_info(artist.id, &info).await?;
        tokio::time::sleep(PAUSE).await;
    }

    if services.musicbrainz.is_some() {
        for album in store.albums_for_info_lookup(older_than).await? {
            let mb = services
                .musicbrainz_album(&album.title, &album.artist)
                .await;
            let mut failed = mb.failed();
            let mb = mb.found();
            let mut cover = None;
            if let (Some(mb), true) = (&mb, album.needs_cover) {
                match services.cover_art(&mb.mbid).await {
                    Fetch::Found(bytes) => cover = art::store(art_dir, &bytes).ok(),
                    Fetch::NotFound => {}
                    Fetch::Failed => failed = true,
                }
            }
            summary.covers += usize::from(cover.is_some());
            summary.genres += usize::from(mb.as_ref().is_some_and(|m| m.genres.is_some()));
            store
                .save_album_info(
                    album.id,
                    cover.as_deref(),
                    mb.as_ref().map(|m| m.mbid.as_str()),
                    mb.as_ref().and_then(|m| m.genres.as_deref()),
                    !failed,
                )
                .await?;
        }
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

    fn from_option(value: Option<Option<T>>) -> Self {
        match value {
            Some(Some(value)) => Fetch::Found(value),
            Some(None) => Fetch::NotFound,
            None => Fetch::Failed,
        }
    }
}

struct MusicBrainz {
    client: reqwest::Client,
    next_request: Instant,
}

struct Services {
    web: reqwest::Client,
    musicbrainz: Option<MusicBrainz>,
}

impl Services {
    fn new(contact: Option<&str>) -> Result<Self, LibraryError> {
        // reqwest is built without a default TLS provider; use ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let build = |agent: &str| {
            reqwest::Client::builder()
                .user_agent(agent)
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|e| LibraryError::Task(format!("http client: {e}")))
        };
        let musicbrainz = match contact.map(str::trim).filter(|c| !c.is_empty()) {
            // MusicBrainz asks for "Application/version ( contact )".
            Some(contact) => Some(MusicBrainz {
                client: build(&format!(
                    "DJINN/{} ( {contact} )",
                    env!("CARGO_PKG_VERSION")
                ))?,
                next_request: Instant::now(),
            }),
            None => None,
        };
        Ok(Self {
            web: build(USER_AGENT)?,
            musicbrainz,
        })
    }

    /// A MusicBrainz web service call, spaced to the rate limit.
    async fn musicbrainz_get<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Option<T> {
        let mb = self.musicbrainz.as_mut()?;
        tokio::time::sleep_until(mb.next_request).await;
        mb.next_request = Instant::now() + MUSICBRAINZ_INTERVAL;
        let request = mb
            .client
            .get(format!("https://musicbrainz.org/ws/2/{path}"))
            .query(query)
            .query(&[("fmt", "json")]);
        get_json(request).await
    }

    async fn musicbrainz_artist(&mut self, name: &str, titles: &[&str]) -> Fetch<MbArtist> {
        if self.musicbrainz.is_none() {
            return Fetch::NotFound;
        }
        let query = format!("artist:{}", lucene_phrase(name));
        let Some(search) = self
            .musicbrainz_get::<MbArtistSearch>("artist", &[("query", &query), ("limit", "10")])
            .await
        else {
            return Fetch::Failed;
        };
        let candidates = exact_artists(&search.artists, name);
        let chosen = match candidates.as_slice() {
            [] => return Fetch::NotFound,
            [only] if only.score >= 90 => Some(only.id.clone()),
            // Several artists share the name: the one with our songs.
            _ => {
                let mut verified = None;
                for candidate in candidates.iter().take(3) {
                    match self.has_recording(&candidate.id, titles).await {
                        Some(true) => {
                            verified = Some(candidate.id.clone());
                            break;
                        }
                        Some(false) => {}
                        None => return Fetch::Failed,
                    }
                }
                verified
            }
        };
        let Some(mbid) = chosen else {
            return Fetch::NotFound;
        };
        let path = format!("artist/{mbid}");
        let Some(detail) = self
            .musicbrainz_get::<MbArtistDetail>(&path, &[("inc", "url-rels+genres")])
            .await
        else {
            return Fetch::Failed;
        };
        Fetch::Found(MbArtist {
            genres: top_genres(&detail.genres),
            wikidata: detail.relations.iter().find_map(|r| {
                (r.kind == "wikidata")
                    .then(|| {
                        r.url
                            .as_ref()?
                            .resource
                            .rsplit('/')
                            .next()
                            .map(str::to_owned)
                    })
                    .flatten()
            }),
            mbid,
        })
    }

    /// Whether MusicBrainz lists one of `titles` by this artist. `None` if
    /// the request failed.
    async fn has_recording(&mut self, mbid: &str, titles: &[&str]) -> Option<bool> {
        let Some(title) = titles.first() else {
            return Some(false);
        };
        let query = format!("arid:{mbid} AND recording:{}", lucene_phrase(title));
        let result: MbCount = self
            .musicbrainz_get("recording", &[("query", &query), ("limit", "1")])
            .await?;
        Some(result.count > 0)
    }

    async fn musicbrainz_album(&mut self, title: &str, artist: &str) -> Fetch<MbAlbum> {
        let query = format!(
            "releasegroup:{} AND artist:{}",
            lucene_phrase(title),
            lucene_phrase(artist)
        );
        let Some(search) = self
            .musicbrainz_get::<MbGroupSearch>("release-group", &[("query", &query), ("limit", "5")])
            .await
        else {
            return Fetch::Failed;
        };
        let Some(group) = pick_group(&search.groups, title, artist) else {
            return Fetch::NotFound;
        };
        let mbid = group.id.clone();
        let path = format!("release-group/{mbid}");
        let Some(detail) = self
            .musicbrainz_get::<MbGenres>(&path, &[("inc", "genres")])
            .await
        else {
            return Fetch::Failed;
        };
        Fetch::Found(MbAlbum {
            genres: top_genres(&detail.genres),
            mbid,
        })
    }

    /// The front cover from the Cover Art Archive, 500 pixels wide.
    async fn cover_art(&self, release_group: &str) -> Fetch<Vec<u8>> {
        let url = format!("https://coverartarchive.org/release-group/{release_group}/front-500");
        let response = match self.web.get(url).send().await {
            Ok(response) => response,
            Err(_) => return Fetch::Failed,
        };
        match response.status() {
            s if s.is_success() => match response.bytes().await {
                Ok(bytes) if art::mime_type(&bytes).starts_with("image/") => {
                    Fetch::Found(bytes.to_vec())
                }
                Ok(_) => Fetch::NotFound,
                Err(_) => Fetch::Failed,
            },
            reqwest::StatusCode::NOT_FOUND => Fetch::NotFound,
            _ => Fetch::Failed,
        }
    }

    /// The artist's English Wikipedia page title and Commons image.
    async fn wikidata(&self, id: &str) -> Fetch<Wikidata> {
        let url = format!("https://www.wikidata.org/wiki/Special:EntityData/{id}.json");
        let Some(response) = get_json::<WdResponse>(self.web.get(url)).await else {
            return Fetch::Failed;
        };
        let Some(entity) = response.entities.into_values().next() else {
            return Fetch::NotFound;
        };
        Fetch::Found(Wikidata {
            enwiki: entity.sitelinks.get("enwiki").map(|s| s.title.clone()),
            image: entity
                .claims
                .get("P18")
                .and_then(|claims| claims.first())
                .and_then(|claim| claim.mainsnak.datavalue.as_ref())
                .and_then(|value| value.value.as_str().map(str::to_owned)),
        })
    }

    /// A Commons photo, 500 pixels wide, with its author and license.
    async fn commons_photo(&self, file: &str) -> Fetch<CommonsPhoto> {
        let title = format!("File:{file}");
        let info: Option<CommonsQuery> = get_json(
            self.web
                .get("https://commons.wikimedia.org/w/api.php")
                .query(&[
                    ("action", "query"),
                    ("titles", &title),
                    ("prop", "imageinfo"),
                    ("iiprop", "extmetadata|url"),
                    ("iiurlwidth", "500"),
                    ("format", "json"),
                ]),
        )
        .await;
        let Some(info) = info else {
            return Fetch::Failed;
        };
        let Some(image) = info
            .query
            .pages
            .into_values()
            .find_map(|p| p.imageinfo.into_iter().next())
        else {
            return Fetch::NotFound;
        };
        let (Some(url), Some(page)) = (image.thumburl, image.descriptionurl) else {
            return Fetch::NotFound;
        };
        let meta = |key: &str| {
            image
                .extmetadata
                .get(key)
                .and_then(|m| m.value.as_str())
                .map(strip_html)
                .filter(|v| !v.is_empty())
        };
        let credit = match (meta("Artist"), meta("LicenseShortName")) {
            (Some(author), Some(license)) => format!("{author} ({license})"),
            (Some(author), None) => author,
            (None, Some(license)) => license,
            (None, None) => "Wikimedia Commons".to_owned(),
        };
        let Some(response) = send(self.web.get(url)).await else {
            return Fetch::Failed;
        };
        match response.bytes().await {
            Ok(bytes) if art::mime_type(&bytes).starts_with("image/") => {
                Fetch::Found(CommonsPhoto {
                    bytes: bytes.to_vec(),
                    credit,
                    page,
                })
            }
            Ok(_) => Fetch::NotFound,
            Err(_) => Fetch::Failed,
        }
    }

    /// The photo of the best-known Deezer artist with exactly this name.
    async fn deezer_photo(&self, name: &str) -> Fetch<Vec<u8>> {
        let search: Option<DeezerSearch> = get_json(
            self.web
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
        let Some(response) = send(self.web.get(url)).await else {
            return Fetch::Failed;
        };
        match response.bytes().await {
            Ok(bytes) if art::mime_type(&bytes).starts_with("image/") => {
                Fetch::Found(bytes.to_vec())
            }
            Ok(_) => Fetch::NotFound,
            Err(_) => Fetch::Failed,
        }
    }

    /// The artist's English Wikipedia intro, found by searching titles.
    async fn wikipedia_bio(&self, name: &str) -> Fetch<(String, String)> {
        let search: Option<WikiSearch> = get_json(
            self.web
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
        let key = page.key.clone();
        self.wikipedia_summary(&key).await
    }

    /// The intro of a known Wikipedia page, and its address.
    async fn wikipedia_summary(&self, title: &str) -> Fetch<(String, String)> {
        let url = format!(
            "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
            urlencode(&title.replace(' ', "_"))
        );
        Fetch::from_option(get_json::<WikiSummary>(self.web.get(url)).await.map(|s| {
            (s.kind == "standard" && !s.extract.is_empty())
                .then_some((s.extract, s.content_urls.desktop.page))
        }))
    }
}

/// Sends a request, retrying once if the service is busy (429 or 5xx).
async fn send(request: reqwest::RequestBuilder) -> Option<reqwest::Response> {
    // Busy spells (MusicBrainz's "server is busy") can outlast one retry.
    let mut request = request;
    for attempt in 1u64..=3 {
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
            .map_or(Duration::from_secs(2 * attempt), Duration::from_secs)
            .min(MAX_RETRY_WAIT);
        tokio::time::sleep(wait).await;
        request = retry?;
    }
    None
}

async fn get_json<T: serde::de::DeserializeOwned>(request: reqwest::RequestBuilder) -> Option<T> {
    send(request).await?.json().await.ok()
}

// --- MusicBrainz ---

#[derive(Deserialize)]
struct MbArtistSearch {
    artists: Vec<MbArtistHit>,
}

#[derive(Deserialize)]
struct MbArtistHit {
    id: String,
    name: String,
    score: u32,
}

#[derive(Deserialize)]
struct MbCount {
    count: u64,
}

#[derive(Deserialize)]
struct MbArtistDetail {
    #[serde(default)]
    genres: Vec<MbGenre>,
    #[serde(default)]
    relations: Vec<MbRelation>,
}

#[derive(Deserialize)]
struct MbGenres {
    #[serde(default)]
    genres: Vec<MbGenre>,
}

#[derive(Deserialize)]
struct MbGenre {
    name: String,
    count: i64,
}

#[derive(Deserialize)]
struct MbRelation {
    #[serde(rename = "type")]
    kind: String,
    url: Option<MbUrl>,
}

#[derive(Deserialize)]
struct MbUrl {
    resource: String,
}

#[derive(Deserialize)]
struct MbGroupSearch {
    #[serde(rename = "release-groups", default)]
    groups: Vec<MbGroupHit>,
}

#[derive(Deserialize)]
struct MbGroupHit {
    id: String,
    title: String,
    score: u32,
    #[serde(rename = "artist-credit", default)]
    credit: Vec<MbCredit>,
}

#[derive(Deserialize)]
struct MbCredit {
    name: String,
}

struct MbArtist {
    mbid: String,
    genres: Option<String>,
    /// Wikidata item id, like "Q27916259".
    wikidata: Option<String>,
}

struct MbAlbum {
    mbid: String,
    genres: Option<String>,
}

/// Search hits whose name matches exactly (ignoring case and punctuation),
/// best score first.
fn exact_artists<'a>(hits: &'a [MbArtistHit], name: &str) -> Vec<&'a MbArtistHit> {
    let wanted = simplify(name);
    let mut exact: Vec<_> = hits
        .iter()
        .filter(|hit| hit.score >= 85 && simplify(&hit.name) == wanted)
        .collect();
    exact.sort_by_key(|hit| std::cmp::Reverse(hit.score));
    exact
}

/// The release group with this exact title, credited to this artist.
fn pick_group<'a>(groups: &'a [MbGroupHit], title: &str, artist: &str) -> Option<&'a MbGroupHit> {
    let (title, artist) = (simplify(title), simplify(artist));
    groups.iter().find(|group| {
        group.score >= 90
            && simplify(&group.title) == title
            && group.credit.iter().any(|c| simplify(&c.name) == artist)
    })
}

/// The most voted genres, comma separated.
fn top_genres(genres: &[MbGenre]) -> Option<String> {
    let mut genres: Vec<&MbGenre> = genres.iter().collect();
    genres.sort_by_key(|g| std::cmp::Reverse(g.count));
    let names: Vec<&str> = genres
        .iter()
        .take(MAX_GENRES)
        .map(|g| g.name.as_str())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// A quoted Lucene phrase for MusicBrainz search.
fn lucene_phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

// --- Wikidata and Commons ---

#[derive(Deserialize)]
struct WdResponse {
    entities: HashMap<String, WdEntity>,
}

#[derive(Deserialize)]
struct WdEntity {
    #[serde(default)]
    sitelinks: HashMap<String, WdSitelink>,
    #[serde(default)]
    claims: HashMap<String, Vec<WdClaim>>,
}

#[derive(Deserialize)]
struct WdSitelink {
    title: String,
}

#[derive(Deserialize)]
struct WdClaim {
    mainsnak: WdSnak,
}

#[derive(Deserialize)]
struct WdSnak {
    datavalue: Option<WdValue>,
}

#[derive(Deserialize)]
struct WdValue {
    value: serde_json::Value,
}

struct Wikidata {
    /// Title of the English Wikipedia page.
    enwiki: Option<String>,
    /// Commons file name of the main image.
    image: Option<String>,
}

#[derive(Deserialize)]
struct CommonsQuery {
    query: CommonsPages,
}

#[derive(Deserialize)]
struct CommonsPages {
    pages: HashMap<String, CommonsPage>,
}

#[derive(Deserialize)]
struct CommonsPage {
    #[serde(default)]
    imageinfo: Vec<CommonsImage>,
}

#[derive(Deserialize)]
struct CommonsImage {
    thumburl: Option<String>,
    descriptionurl: Option<String>,
    #[serde(default)]
    extmetadata: HashMap<String, CommonsMeta>,
}

#[derive(Deserialize)]
struct CommonsMeta {
    value: serde_json::Value,
}

struct CommonsPhoto {
    bytes: Vec<u8>,
    /// "Author (License)".
    credit: String,
    /// The file's page on Commons.
    page: String,
}

/// Plain text from Commons' HTML metadata (author names are often links).
fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// --- Deezer ---

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

fn pick_deezer<'a>(artists: &'a [DeezerArtist], name: &str) -> Option<&'a str> {
    let wanted = simplify(name);
    artists
        .iter()
        // An empty image id ("/artist//") is Deezer's placeholder.
        .filter(|a| simplify(&a.name) == wanted && !a.picture_big.contains("/artist//"))
        .max_by_key(|a| a.nb_fan)
        .map(|a| a.picture_big.as_str())
}

// --- Wikipedia ---

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

    fn hit(id: &str, name: &str, score: u32) -> MbArtistHit {
        MbArtistHit {
            id: id.into(),
            name: name.into(),
            score,
        }
    }

    #[test]
    fn musicbrainz_exact_name_candidates() {
        let hits = [
            hit("no", "KREAM", 100),
            hit("maxo", "Maxo Kream", 92),
            hit("kr", "KREAM", 90),
            hit("weak", "Kream", 60),
        ];
        let ids: Vec<&str> = exact_artists(&hits, "Kream")
            .iter()
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["no", "kr"],
            "exact names only, best first, weak scores dropped"
        );
    }

    #[test]
    fn release_group_needs_title_and_artist() {
        let group = |id: &str, title: &str, artist: &str, score| MbGroupHit {
            id: id.into(),
            title: title.into(),
            score,
            credit: vec![MbCredit {
                name: artist.into(),
            }],
        };
        let groups = [
            group(
                "collab",
                "Random Access Memories: The Collaborators",
                "Daft Punk",
                88,
            ),
            group("other", "Random Access Memories", "Tribute Band", 95),
            group("ram", "Random Access Memories", "Daft Punk", 100),
        ];
        let picked = pick_group(&groups, "Random Access Memories", "Daft Punk");
        assert_eq!(picked.map(|g| g.id.as_str()), Some("ram"));
    }

    #[test]
    fn genres_and_text_helpers() {
        let genres = [
            MbGenre {
                name: "house".into(),
                count: 1,
            },
            MbGenre {
                name: "edm".into(),
                count: 3,
            },
        ];
        assert_eq!(top_genres(&genres).as_deref(), Some("edm, house"));
        assert_eq!(top_genres(&[]), None);
        assert_eq!(lucene_phrase(r#"Say "Hi" \o/"#), r#""Say \"Hi\" \\o/""#);
        assert_eq!(
            strip_html(r#"<a href="//commons.wikimedia.org/wiki/User:X">Mitchell  Bethea</a>"#),
            "Mitchell Bethea"
        );
    }

    /// Talks to Wikimedia Commons; run by hand with `--ignored`.
    #[tokio::test]
    #[ignore = "needs the internet"]
    async fn commons_photo_has_credit() {
        let services = Services::new(None).unwrap();
        let photo = services
            .commons_photo("DSC00802 KREAM at Electric Zoo 2.jpg")
            .await
            .found()
            .expect("photo");
        assert_eq!(art::mime_type(&photo.bytes), "image/jpeg");
        assert_eq!(photo.credit, "Mitchell Bethea (CC BY-SA 4.0)");
        assert!(photo
            .page
            .starts_with("https://commons.wikimedia.org/wiki/File:"));
    }
}
