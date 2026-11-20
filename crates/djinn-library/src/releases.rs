//! Releases of an album to compare with the songs DJINN has, for the library
//! editor's album layout (like Picard): every release MusicBrainz has of an
//! album, and searches by artist and album on MusicBrainz, Deezer, Apple's
//! iTunes catalog (those two need no key) and Spotify (with the admin's
//! developer app's client ID and secret). A release comes with its track
//! list and where its cover is. Only what the admin typed or picked is sent,
//! and only when they ask.

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::LibraryError;

/// Where a release comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    MusicBrainz,
    Deezer,
    Itunes,
    Spotify,
}

/// The accounts some sources need.
#[derive(Clone, Debug, Default)]
pub struct Accounts {
    /// The contact MusicBrainz asks for in each request.
    pub musicbrainz: Option<String>,
    /// A Spotify developer app's client ID and secret.
    pub spotify: Option<(String, String)>,
}

/// A release found, before its track list is fetched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReleaseHit {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: Option<String>,
    pub country: Option<String>,
    /// "CD", "Digital Media", "Vinyl"... or the kind (album, single).
    pub format: Option<String>,
    pub tracks: usize,
    /// What sets it apart ("deluxe edition", "remastered").
    pub note: Option<String>,
}

/// One track of a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReleaseTrack {
    pub disc: i64,
    pub number: i64,
    pub title: String,
    /// The track's own artist when it differs from the release's.
    pub artist: Option<String>,
    /// The track's own genre when it differs from the release's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    pub length_ms: Option<i64>,
    /// Other tags returned for this track (by lofty `ItemKey` name, e.g. `"Isrc"`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// A release with its track list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Release {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    pub has_cover: bool,
    /// Other tags returned for the release (by lofty `ItemKey` name, e.g. `"Label"`, `"Barcode"`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
    pub tracks: Vec<ReleaseTrack>,
}

fn put_extra(map: &mut BTreeMap<String, String>, key: &str, value: Option<String>) {
    if let Some(clean) = value
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|v| !v.is_empty())
    {
        map.insert(key.to_owned(), clean);
    }
}

fn join_unique(
    values: impl IntoIterator<Item = String>,
    sep: &str,
    limit: usize,
) -> Option<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in values {
        let clean = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if clean.is_empty() || !seen.insert(clean.to_ascii_lowercase()) {
            continue;
        }
        out.push(clean);
        if out.len() >= limit {
            break;
        }
    }
    (!out.is_empty()).then(|| out.join(sep))
}

const MB_INTERVAL: Duration = Duration::from_millis(1100);
static MB_NEXT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

fn client(agent: &str) -> Result<reqwest::Client, LibraryError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(agent)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| LibraryError::Task(format!("http client: {e}")))
}

fn failed(what: &str, e: impl std::fmt::Display) -> LibraryError {
    LibraryError::Task(format!("{what} didn't answer: {e}"))
}

async fn get<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
    what: &str,
) -> Result<T, LibraryError> {
    // A busy service (MusicBrainz says 503 when it's had too many asks,
    // others 429) gets a few more tries, waiting as it says or a little
    // longer each time.
    for attempt in 1u64..=4 {
        let response = client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|e| failed(what, e))?;
        let status = response.status();
        let busy = status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE;
        if busy && attempt < 4 {
            let wait = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
                .map_or(Duration::from_secs(2 * attempt), Duration::from_secs)
                .min(Duration::from_secs(10));
            tokio::time::sleep(wait).await;
            continue;
        }
        if busy {
            return Err(LibraryError::Task(format!(
                "{what} is busy right now; try again in a minute"
            )));
        }
        let response = response.error_for_status().map_err(|e| failed(what, e))?;
        return response.json().await.map_err(|e| failed(what, e));
    }
    Err(LibraryError::Task(format!("{what} didn't answer")))
}

/// A MusicBrainz call, spaced to its one-a-second limit.
async fn mb<T: serde::de::DeserializeOwned>(
    contact: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<T, LibraryError> {
    let wait = {
        let mut next = MB_NEXT.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let at = next.map_or(now, |n| n.max(now));
        *next = Some(at + MB_INTERVAL);
        at - now
    };
    tokio::time::sleep(wait).await;
    let agent = format!("DJINN/{} ( {contact} )", env!("CARGO_PKG_VERSION"));
    let mut query = query.to_vec();
    query.push(("fmt", "json"));
    get(
        &client(&agent)?,
        &format!("https://musicbrainz.org/ws/2/{path}"),
        &query,
        "MusicBrainz",
    )
    .await
}

const AGENT: &str = concat!("DJINN/", env!("CARGO_PKG_VERSION"));

// --- MusicBrainz ---

#[derive(Deserialize)]
struct MbList {
    #[serde(default)]
    releases: Vec<MbRelease>,
}

#[derive(Deserialize)]
struct MbCredit {
    name: String,
    #[serde(default)]
    joinphrase: String,
}

fn credit(credits: &[MbCredit]) -> String {
    credits
        .iter()
        .map(|c| format!("{}{}", c.name, c.joinphrase))
        .collect::<String>()
        .trim()
        .to_owned()
}

#[derive(Deserialize)]
struct MbGenre {
    name: String,
    #[serde(default)]
    count: i64,
}

#[derive(Deserialize)]
struct MbReleaseGroup {
    #[serde(default)]
    genres: Vec<MbGenre>,
}

#[derive(Deserialize)]
struct MbName {
    name: String,
}

#[derive(Deserialize)]
struct MbLabelInfo {
    #[serde(default, rename = "catalog-number")]
    catalog_number: Option<String>,
    #[serde(default)]
    label: Option<MbName>,
}

#[derive(Deserialize)]
struct MbCoverArt {
    #[serde(default)]
    front: bool,
}

#[derive(Deserialize)]
struct MbRecording {
    #[serde(default)]
    isrcs: Vec<String>,
    #[serde(default)]
    genres: Vec<MbGenre>,
}

#[derive(Deserialize)]
struct MbRelease {
    id: String,
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default)]
    barcode: Option<String>,
    #[serde(default)]
    genres: Vec<MbGenre>,
    #[serde(default, rename = "release-group")]
    release_group: Option<MbReleaseGroup>,
    #[serde(default, rename = "label-info")]
    label_info: Vec<MbLabelInfo>,
    #[serde(default, rename = "cover-art-archive")]
    cover_art_archive: Option<MbCoverArt>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<MbCredit>,
    #[serde(default)]
    media: Vec<MbMedium>,
}

#[derive(Deserialize)]
struct MbMedium {
    #[serde(default)]
    position: i64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default, rename = "track-count")]
    track_count: usize,
    #[serde(default)]
    tracks: Vec<MbTrack>,
}

#[derive(Deserialize)]
struct MbTrack {
    #[serde(default)]
    position: i64,
    title: String,
    #[serde(default)]
    length: Option<i64>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<MbCredit>,
    #[serde(default)]
    recording: Option<MbRecording>,
}

impl MbRelease {
    fn hit(self) -> ReleaseHit {
        let mut formats: Vec<String> = self.media.iter().filter_map(|m| m.format.clone()).collect();
        formats.dedup();
        let note = [
            self.disambiguation.clone(),
            self.status.clone().filter(|s| s != "Official"),
        ]
        .into_iter()
        .flatten()
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
        ReleaseHit {
            source: Source::MusicBrainz,
            tracks: self
                .media
                .iter()
                .map(|m| m.track_count.max(m.tracks.len()))
                .sum(),
            artist: credit(&self.artist_credit),
            format: (!formats.is_empty()).then(|| {
                if self.media.len() > 1 {
                    format!("{} x {}", self.media.len(), formats.join(" + "))
                } else {
                    formats.join(" + ")
                }
            }),
            note: (!note.is_empty()).then_some(note),
            id: self.id,
            title: self.title,
            date: self.date.filter(|d| !d.is_empty()),
            country: self.country,
        }
    }
}

/// Every release MusicBrainz has of an album (its release group), oldest
/// first.
pub async fn releases_of_group(
    contact: &str,
    group: &str,
) -> Result<Vec<ReleaseHit>, LibraryError> {
    let list: MbList = mb(
        contact,
        "release",
        &[
            ("release-group", group),
            ("inc", "media+artist-credits"),
            ("limit", "100"),
        ],
    )
    .await?;
    let mut hits: Vec<ReleaseHit> = list.releases.into_iter().map(MbRelease::hit).collect();
    hits.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(hits)
}

/// Lucene's special characters, escaped inside a quoted phrase.
fn phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn year_of(date: Option<&str>) -> Option<i64> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok())
}

fn mb_genres(a: &[MbGenre], b: &[MbGenre]) -> Option<String> {
    let mut combined: Vec<&MbGenre> = a.iter().chain(b.iter()).collect();
    combined.sort_by_key(|g| std::cmp::Reverse(g.count));
    join_unique(combined.into_iter().map(|g| g.name.clone()), ", ", 3)
}

async fn mb_release(contact: &str, id: &str) -> Result<Option<Release>, LibraryError> {
    let release: MbRelease = mb(
        contact,
        &format!("release/{id}"),
        &[(
            "inc",
            "recordings+artist-credits+media+genres+release-groups+labels+isrcs",
        )],
    )
    .await?;
    let artist = credit(&release.artist_credit);
    let group_genres = release
        .release_group
        .as_ref()
        .map_or(&[][..], |g| &g.genres);
    let genre = mb_genres(&release.genres, group_genres);
    let has_cover = release.cover_art_archive.as_ref().is_none_or(|c| c.front);
    let mut extra = BTreeMap::new();
    put_extra(
        &mut extra,
        "Label",
        join_unique(
            release
                .label_info
                .iter()
                .filter_map(|li| li.label.as_ref().map(|l| l.name.clone())),
            ", ",
            5,
        ),
    );
    put_extra(
        &mut extra,
        "CatalogNumber",
        join_unique(
            release
                .label_info
                .iter()
                .filter_map(|li| li.catalog_number.clone()),
            ", ",
            5,
        ),
    );
    put_extra(&mut extra, "Barcode", release.barcode);
    put_extra(&mut extra, "ReleaseCountry", release.country);
    Ok(Some(Release {
        source: Source::MusicBrainz,
        year: year_of(release.date.as_deref()),
        genre: genre.clone(),
        has_cover,
        extra,
        tracks: release
            .media
            .iter()
            .flat_map(|m| {
                let artist = artist.clone();
                let genre = genre.clone();
                let disc_title = m.title.clone();
                m.tracks.iter().map(move |t| {
                    let own = credit(&t.artist_credit);
                    let track_genre = t
                        .recording
                        .as_ref()
                        .and_then(|r| mb_genres(&r.genres, &[]))
                        .filter(|g| Some(g) != genre.as_ref());
                    let mut track_extra = BTreeMap::new();
                    if let Some(rec) = &t.recording {
                        put_extra(
                            &mut track_extra,
                            "Isrc",
                            join_unique(rec.isrcs.iter().cloned(), "; ", 5),
                        );
                    }
                    put_extra(&mut track_extra, "SetSubtitle", disc_title.clone());
                    ReleaseTrack {
                        disc: m.position.max(1),
                        number: t.position,
                        title: t.title.clone(),
                        artist: (!own.is_empty() && own != artist).then_some(own),
                        genre: track_genre,
                        length_ms: t.length,
                        extra: track_extra,
                    }
                })
            })
            .collect(),
        id: release.id,
        title: release.title,
        artist,
    }))
}

// --- Deezer ---

#[derive(Deserialize)]
struct DzList<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct DzName {
    name: String,
}

#[derive(Deserialize)]
struct DzAlbum {
    id: i64,
    title: String,
    artist: DzName,
    #[serde(default)]
    nb_tracks: usize,
    #[serde(default)]
    record_type: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    cover_xl: Option<String>,
    #[serde(default)]
    genres: Option<DzList<DzName>>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    upc: Option<String>,
}

#[derive(Deserialize)]
struct DzTrack {
    title: String,
    #[serde(default)]
    duration: Option<i64>,
    #[serde(default)]
    track_position: Option<i64>,
    #[serde(default)]
    disk_number: Option<i64>,
    #[serde(default)]
    artist: Option<DzName>,
    #[serde(default)]
    isrc: Option<String>,
}

async fn dz_album(id: &str) -> Result<DzAlbum, LibraryError> {
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(LibraryError::Task("not a Deezer album".into()));
    }
    get(
        &client(AGENT)?,
        &format!("https://api.deezer.com/album/{id}"),
        &[],
        "Deezer",
    )
    .await
}

async fn dz_release(id: &str) -> Result<Option<Release>, LibraryError> {
    let album = dz_album(id).await?;
    let tracks: DzList<DzTrack> = get(
        &client(AGENT)?,
        &format!("https://api.deezer.com/album/{id}/tracks"),
        &[("limit", "500")],
        "Deezer",
    )
    .await?;
    let artist = album.artist.name.clone();
    let genre = album
        .genres
        .and_then(|g| join_unique(g.data.into_iter().map(|n| n.name), ", ", 3));
    let has_cover = album.cover_xl.as_deref().is_some_and(|u| !u.is_empty());
    let mut extra = BTreeMap::new();
    put_extra(&mut extra, "Label", album.label);
    put_extra(&mut extra, "Barcode", album.upc);
    Ok(Some(Release {
        source: Source::Deezer,
        id: id.to_owned(),
        title: album.title,
        year: year_of(album.release_date.as_deref()),
        genre,
        has_cover,
        extra,
        tracks: tracks
            .data
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let mut track_extra = BTreeMap::new();
                put_extra(&mut track_extra, "Isrc", t.isrc);
                ReleaseTrack {
                    disc: t.disk_number.unwrap_or(1).max(1),
                    number: t.track_position.unwrap_or(i as i64 + 1),
                    title: t.title,
                    artist: t.artist.map(|a| a.name).filter(|a| *a != artist),
                    genre: None,
                    length_ms: t.duration.map(|s| s * 1000),
                    extra: track_extra,
                }
            })
            .collect(),
        artist,
    }))
}

// --- Apple (iTunes Search) ---

#[derive(Deserialize)]
struct ItList {
    #[serde(default)]
    results: Vec<ItItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItItem {
    #[serde(default)]
    wrapper_type: String,
    #[serde(default)]
    collection_id: Option<i64>,
    #[serde(default)]
    collection_name: Option<String>,
    #[serde(default)]
    artist_name: Option<String>,
    #[serde(default)]
    track_count: Option<usize>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    collection_explicitness: Option<String>,
    #[serde(default)]
    artwork_url100: Option<String>,
    #[serde(default)]
    track_name: Option<String>,
    #[serde(default)]
    track_number: Option<i64>,
    #[serde(default)]
    disc_number: Option<i64>,
    #[serde(default)]
    track_time_millis: Option<i64>,
    #[serde(default)]
    primary_genre_name: Option<String>,
    #[serde(default)]
    copyright: Option<String>,
}

async fn it_lookup(id: &str) -> Result<ItList, LibraryError> {
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(LibraryError::Task("not an Apple album".into()));
    }
    get(
        &client(AGENT)?,
        "https://itunes.apple.com/lookup",
        &[("id", id), ("entity", "song"), ("limit", "300")],
        "Apple",
    )
    .await
}

async fn it_release(id: &str) -> Result<Option<Release>, LibraryError> {
    let list = it_lookup(id).await?;
    let Some(album) = list.results.iter().find(|r| r.wrapper_type == "collection") else {
        return Ok(None);
    };
    let artist = album.artist_name.clone().unwrap_or_default();
    let genre = album
        .primary_genre_name
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned);
    let has_cover = album
        .artwork_url100
        .as_deref()
        .is_some_and(|u| !u.is_empty());
    let mut extra = BTreeMap::new();
    put_extra(&mut extra, "CopyrightMessage", album.copyright.clone());
    put_extra(&mut extra, "ReleaseCountry", album.country.clone());
    let mut tracks: Vec<ReleaseTrack> = list
        .results
        .iter()
        .filter(|r| r.wrapper_type == "track")
        .map(|t| ReleaseTrack {
            disc: t.disc_number.unwrap_or(1).max(1),
            number: t.track_number.unwrap_or(0),
            title: t.track_name.clone().unwrap_or_default(),
            artist: t.artist_name.clone().filter(|a| *a != artist),
            genre: t
                .primary_genre_name
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(str::to_owned)
                .filter(|g| Some(g) != genre.as_ref()),
            length_ms: t.track_time_millis,
            extra: BTreeMap::new(),
        })
        .collect();
    tracks.sort_by_key(|t| (t.disc, t.number));
    Ok(Some(Release {
        source: Source::Itunes,
        id: id.to_owned(),
        title: album.collection_name.clone().unwrap_or_default(),
        year: year_of(album.release_date.as_deref()),
        genre,
        has_cover,
        extra,
        tracks,
        artist,
    }))
}

// --- Spotify (Web API, client credentials) ---

/// A token for the app, and when it runs out.
static SPOTIFY_TOKEN: std::sync::Mutex<Option<(String, String, Instant)>> =
    std::sync::Mutex::new(None);

#[derive(Deserialize)]
struct SpToken {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

/// A token for the Spotify app `keys` (client ID, secret), kept until it
/// runs out. An error says why Spotify refused.
pub async fn spotify_token(keys: &(String, String)) -> Result<String, LibraryError> {
    if let Some((id, token, until)) = SPOTIFY_TOKEN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        if id == keys.0 && Instant::now() < until {
            return Ok(token);
        }
    }
    let response = client(AGENT)?
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(&keys.0, Some(&keys.1))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .map_err(|e| failed("Spotify", e))?;
    if response.status() == reqwest::StatusCode::BAD_REQUEST
        || response.status() == reqwest::StatusCode::UNAUTHORIZED
    {
        return Err(LibraryError::Task(
            "Spotify didn't accept the client ID and secret".into(),
        ));
    }
    let token: SpToken = response
        .error_for_status()
        .map_err(|e| failed("Spotify", e))?
        .json()
        .await
        .map_err(|e| failed("Spotify", e))?;
    let until = Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(60).max(60));
    *SPOTIFY_TOKEN.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((keys.0.clone(), token.access_token.clone(), until));
    Ok(token.access_token)
}

async fn spotify_get<T: serde::de::DeserializeOwned>(
    keys: &(String, String),
    url: &str,
    query: &[(&str, &str)],
) -> Result<T, LibraryError> {
    let token = spotify_token(keys).await?;
    let response = client(AGENT)?
        .get(url)
        .bearer_auth(token)
        .query(query)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| failed("Spotify", e))?;
    response.json().await.map_err(|e| failed("Spotify", e))
}

#[derive(Deserialize)]
struct SpName {
    #[serde(default)]
    id: Option<String>,
    name: String,
}

#[derive(Deserialize)]
struct SpImage {
    url: String,
}

#[derive(Deserialize)]
struct SpExternalIds {
    #[serde(default)]
    upc: Option<String>,
    #[serde(default)]
    ean: Option<String>,
    #[serde(default)]
    isrc: Option<String>,
}

#[derive(Deserialize)]
struct SpCopyright {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct SpArtist {
    #[serde(default)]
    genres: Vec<String>,
}

#[derive(Deserialize)]
struct SpAlbum {
    id: String,
    name: String,
    #[serde(default)]
    artists: Vec<SpName>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    total_tracks: usize,
    #[serde(default)]
    album_type: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    external_ids: Option<SpExternalIds>,
    #[serde(default)]
    copyrights: Vec<SpCopyright>,
    #[serde(default)]
    images: Vec<SpImage>,
    #[serde(default)]
    tracks: Option<SpPage<SpTrack>>,
}

#[derive(Deserialize)]
struct SpPage<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
struct SpSearch {
    albums: SpPage<SpAlbum>,
}

#[derive(Deserialize)]
struct SpTrack {
    name: String,
    #[serde(default)]
    track_number: i64,
    #[serde(default)]
    disc_number: i64,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    artists: Vec<SpName>,
    #[serde(default)]
    external_ids: Option<SpExternalIds>,
}

fn names(list: &[SpName]) -> String {
    list.iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

async fn sp_album(keys: &(String, String), id: &str) -> Result<SpAlbum, LibraryError> {
    spotify_get(
        keys,
        &format!("https://api.spotify.com/v1/albums/{id}"),
        &[],
    )
    .await
}

async fn sp_release(keys: &(String, String), id: &str) -> Result<Option<Release>, LibraryError> {
    let album = sp_album(keys, id).await?;
    let artist = names(&album.artists);
    let mut genre = join_unique(album.genres.clone(), ", ", 3);
    if genre.is_none() {
        if let Some(artist_id) = album
            .artists
            .first()
            .and_then(|a| a.id.as_deref())
            .filter(|i| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric()))
        {
            if let Ok(sp_artist) = spotify_get::<SpArtist>(
                keys,
                &format!("https://api.spotify.com/v1/artists/{artist_id}"),
                &[],
            )
            .await
            {
                genre = join_unique(sp_artist.genres, ", ", 3);
            }
        }
    }
    let has_cover = !album.images.is_empty();
    let mut extra = BTreeMap::new();
    put_extra(&mut extra, "Label", album.label);
    put_extra(
        &mut extra,
        "Barcode",
        album.external_ids.and_then(|e| e.upc.or(e.ean)),
    );
    put_extra(
        &mut extra,
        "CopyrightMessage",
        album.copyrights.into_iter().next().map(|c| c.text),
    );
    let mut items = Vec::new();
    let mut page = album.tracks;
    // Long albums come in pages of 50; only Spotify's own next links are followed.
    for _ in 0..20 {
        let Some(current) = page.take() else { break };
        items.extend(current.items);
        match current
            .next
            .filter(|n| n.starts_with("https://api.spotify.com/"))
        {
            Some(next) => page = Some(spotify_get(keys, &next, &[]).await?),
            None => break,
        }
    }
    Ok(Some(Release {
        source: Source::Spotify,
        id: album.id,
        title: album.name,
        year: year_of(album.release_date.as_deref()),
        genre,
        has_cover,
        extra,
        tracks: items
            .into_iter()
            .map(|t| {
                let own = names(&t.artists);
                let mut track_extra = BTreeMap::new();
                put_extra(
                    &mut track_extra,
                    "Isrc",
                    t.external_ids.and_then(|e| e.isrc),
                );
                ReleaseTrack {
                    disc: t.disc_number.max(1),
                    number: t.track_number,
                    title: t.name,
                    artist: (own != artist && !own.is_empty()).then_some(own),
                    genre: None,
                    length_ms: t.duration_ms,
                    extra: track_extra,
                }
            })
            .collect(),
        artist,
    }))
}

// --- All sources ---

/// Releases matching an artist and album on `source`, best first.
pub async fn search(
    source: Source,
    accounts: &Accounts,
    artist: &str,
    album: &str,
) -> Result<Vec<ReleaseHit>, LibraryError> {
    let contact = accounts.musicbrainz.as_deref();
    let (artist, album) = (artist.trim(), album.trim());
    if artist.is_empty() && album.is_empty() {
        return Ok(Vec::new());
    }
    match source {
        Source::MusicBrainz => {
            let contact = contact.ok_or_else(|| {
                LibraryError::Task("set the MusicBrainz contact under Rules first".into())
            })?;
            let query = [
                (!album.is_empty()).then(|| format!("release:{}", phrase(album))),
                (!artist.is_empty()).then(|| format!("artist:{}", phrase(artist))),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" AND ");
            let list: MbList =
                mb(contact, "release", &[("query", &query), ("limit", "25")]).await?;
            Ok(list.releases.into_iter().map(MbRelease::hit).collect())
        }
        Source::Deezer => {
            let query = [
                (!artist.is_empty()).then(|| format!("artist:{}", phrase(artist))),
                (!album.is_empty()).then(|| format!("album:{}", phrase(album))),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            let list: DzList<DzAlbum> = get(
                &client(AGENT)?,
                "https://api.deezer.com/search/album",
                &[("q", &query), ("limit", "25")],
                "Deezer",
            )
            .await?;
            Ok(list
                .data
                .into_iter()
                .map(|a| ReleaseHit {
                    source: Source::Deezer,
                    id: a.id.to_string(),
                    title: a.title,
                    artist: a.artist.name,
                    date: a.release_date,
                    country: None,
                    format: a.record_type,
                    tracks: a.nb_tracks,
                    note: None,
                })
                .collect())
        }
        Source::Itunes => {
            // Apple's search ranks poorly on mixed words: look by the artist
            // and by the album title, and put what fits both first.
            let fold = |t: &str| -> String {
                t.chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect()
            };
            let (want_artist, want_album) = (fold(artist), fold(album));
            let mut found: Vec<(u8, ReleaseHit)> = Vec::new();
            for (term, attribute) in [(artist, "artistTerm"), (album, "albumTerm")] {
                if term.is_empty() {
                    continue;
                }
                let list: ItList = get(
                    &client(AGENT)?,
                    "https://itunes.apple.com/search",
                    &[
                        ("term", term),
                        ("entity", "album"),
                        ("attribute", attribute),
                        ("limit", "200"),
                    ],
                    "Apple",
                )
                .await?;
                for a in list.results {
                    let (Some(id), Some(title)) = (a.collection_id, a.collection_name) else {
                        continue;
                    };
                    let id = id.to_string();
                    if found.iter().any(|(_, h)| h.id == id) {
                        continue;
                    }
                    let name = a.artist_name.unwrap_or_default();
                    let artist_fits = want_artist.is_empty() || fold(&name) == want_artist;
                    let album_fits = want_album.is_empty() || fold(&title).contains(&want_album);
                    let rank = match (artist_fits, album_fits) {
                        (true, true) => 0,
                        (true, false) => 2,
                        (false, true) => 1,
                        (false, false) => continue,
                    };
                    found.push((
                        rank,
                        ReleaseHit {
                            source: Source::Itunes,
                            id,
                            title,
                            artist: name,
                            date: a.release_date.map(|d| d.chars().take(10).collect()),
                            country: a.country,
                            format: None,
                            tracks: a.track_count.unwrap_or(0),
                            note: a.collection_explicitness.filter(|e| e == "explicit"),
                        },
                    ));
                }
            }
            found.sort_by_key(|(rank, _)| *rank);
            Ok(found.into_iter().map(|(_, hit)| hit).take(25).collect())
        }
        Source::Spotify => {
            let keys = spotify_keys(accounts)?;
            let query = [
                (!album.is_empty()).then(|| format!("album:{album}")),
                (!artist.is_empty()).then(|| format!("artist:{artist}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            let found: SpSearch = spotify_get(
                keys,
                "https://api.spotify.com/v1/search",
                &[("q", &query), ("type", "album"), ("limit", "20")],
            )
            .await?;
            Ok(found
                .albums
                .items
                .into_iter()
                .map(|a| ReleaseHit {
                    source: Source::Spotify,
                    artist: names(&a.artists),
                    id: a.id,
                    title: a.name,
                    date: a.release_date,
                    country: None,
                    format: a.album_type,
                    tracks: a.total_tracks,
                    note: None,
                })
                .collect())
        }
    }
}

fn spotify_keys(accounts: &Accounts) -> Result<&(String, String), LibraryError> {
    accounts.spotify.as_ref().ok_or_else(|| {
        LibraryError::Task("add the Spotify account under Admin > Library first".into())
    })
}

/// A release with its track list.
pub async fn release(
    source: Source,
    accounts: &Accounts,
    id: &str,
) -> Result<Option<Release>, LibraryError> {
    let contact = accounts.musicbrainz.as_deref();
    let id = id.trim();
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(LibraryError::Task("that isn't a release".into()));
    }
    match source {
        Source::MusicBrainz => {
            let contact = contact.ok_or_else(|| {
                LibraryError::Task("set the MusicBrainz contact under Rules first".into())
            })?;
            mb_release(contact, id).await
        }
        Source::Deezer => dz_release(id).await,
        Source::Itunes => it_release(id).await,
        Source::Spotify => sp_release(spotify_keys(accounts)?, id).await,
    }
}

/// A release's front cover (JPEG or PNG bytes), if it has one.
pub async fn cover(
    source: Source,
    accounts: &Accounts,
    id: &str,
) -> Result<Option<Vec<u8>>, LibraryError> {
    let id = id.trim();
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(LibraryError::Task("that isn't a release".into()));
    }
    let url = match source {
        Source::MusicBrainz => Some(format!(
            "https://coverartarchive.org/release/{id}/front-500"
        )),
        Source::Deezer => dz_album(id).await?.cover_xl,
        Source::Itunes => it_lookup(id)
            .await?
            .results
            .into_iter()
            .find(|r| r.wrapper_type == "collection")
            .and_then(|r| r.artwork_url100)
            .map(|u| u.replace("100x100bb", "600x600bb")),
        Source::Spotify => sp_album(spotify_keys(accounts)?, id)
            .await?
            .images
            .into_iter()
            .next()
            .map(|i| i.url),
    };
    let Some(url) = url else { return Ok(None) };
    let allowed = |url: &str| {
        reqwest::Url::parse(url).ok().is_some_and(|u| {
            u.scheme() == "https"
                && u.host_str().is_some_and(|h| {
                    h == "coverartarchive.org"
                        || h.ends_with(".dzcdn.net")
                        || h.ends_with(".mzstatic.com")
                })
        })
    };
    if !allowed(&url) {
        return Ok(None);
    }
    // Deezer's main picture host refuses at times; its others don't.
    let mut last = None;
    let mut found = None;
    for url in crate::online::deezer_picture_urls(&url) {
        if !allowed(&url) {
            continue;
        }
        let response = match client(AGENT)?.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                last = Some(failed("the cover's site", e));
                continue;
            }
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        match response.error_for_status() {
            Ok(r) => {
                found = Some(r);
                break;
            }
            Err(e) => last = Some(failed("the cover's site", e)),
        }
    }
    let Some(response) = found else {
        return match last {
            Some(e) => Err(e),
            None => Ok(None),
        };
    };
    let bytes = response
        .bytes()
        .await
        .map_err(|e| failed("the cover's site", e))?;
    let picture = bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(&[0x89, b'P', b'N', b'G']);
    Ok(picture.then(|| bytes.to_vec()))
}

/// Matches songs (id, title, length) to a release's tracks by title (case,
/// punctuation and version notes aside), the closest length deciding
/// between tracks of one title. Returns (song id, track index).
pub fn match_songs(release: &Release, songs: &[(i64, String, Option<i64>)]) -> Vec<(i64, usize)> {
    let list = crate::online::TrackList {
        release: release.title.clone(),
        tracks: release
            .tracks
            .iter()
            .map(|t| crate::online::ListedTrack {
                disc: t.disc,
                number: t.number,
                title: t.title.clone(),
                length_ms: t.length_ms,
            })
            .collect(),
    };
    crate::online::match_tracks(&list, songs)
        .into_iter()
        .filter_map(|(id, disc, number)| {
            release
                .tracks
                .iter()
                .position(|t| t.disc == disc && t.number == number)
                .map(|i| (id, i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn musicbrainz_answers_read_as_releases() {
        let json = r#"{"releases":[{"id":"r1","title":"Out of Time","date":"1991-03-12","country":"US",
            "status":"Official","disambiguation":"","barcode":"075992649629",
            "genres":[{"name":"alternative rock","count":10},{"name":"rock","count":5}],
            "release-group":{"genres":[{"name":"jangle pop","count":7},{"name":"alternative rock","count":2}]},
            "label-info":[{"catalog-number":"9 26496-2","label":{"name":"Warner Bros."}}],
            "cover-art-archive":{"front":true},
            "artist-credit":[{"name":"R.E.M.","joinphrase":""}],
            "media":[{"position":1,"format":"CD","track-count":2,"tracks":[
              {"position":1,"title":"Radio Song","length":255000,
               "artist-credit":[{"name":"R.E.M.","joinphrase":" feat. "},{"name":"KRS-One","joinphrase":""}],
               "recording":{"isrcs":["USWB19100001"],"genres":[{"name":"pop rap","count":4}]}},
              {"position":2,"title":"Losing My Religion","length":268000,"artist-credit":[{"name":"R.E.M.","joinphrase":""}]}]}]}]}"#;
        let list: MbList = serde_json::from_str(json).unwrap();
        let hit = list.releases.into_iter().next().unwrap().hit();
        assert_eq!(
            (hit.tracks, hit.format.as_deref(), hit.note),
            (2, Some("CD"), None)
        );
        let release: MbRelease = serde_json::from_str::<MbList>(json)
            .unwrap()
            .releases
            .remove(0);
        let artist = credit(&release.artist_credit);
        assert_eq!(artist, "R.E.M.");
        assert_eq!(
            credit(&release.media[0].tracks[0].artist_credit),
            "R.E.M. feat. KRS-One"
        );
        let group_genres = release
            .release_group
            .as_ref()
            .map_or(&[][..], |g| &g.genres);
        assert_eq!(
            mb_genres(&release.genres, group_genres).as_deref(),
            Some("alternative rock, jangle pop, rock")
        );
        assert_eq!(release.barcode.as_deref(), Some("075992649629"));
        assert_eq!(
            release.label_info[0].catalog_number.as_deref(),
            Some("9 26496-2")
        );
        assert_eq!(
            release.media[0].tracks[0].recording.as_ref().unwrap().isrcs,
            ["USWB19100001"]
        );
    }

    #[test]
    fn spotify_answers_read_as_releases() {
        let json = r#"{"id":"a1","name":"Out of Time","release_date":"1991-03-12","total_tracks":2,
            "album_type":"album","genres":["alternative rock"],"label":"Warner Bros.",
            "external_ids":{"upc":"075992649629"},
            "artists":[{"id":"art1","name":"R.E.M."}],"images":[{"url":"https://i.scdn.co/image/x"}],
            "tracks":{"items":[{"name":"Radio Song","track_number":1,"disc_number":1,"duration_ms":255000,
              "external_ids":{"isrc":"USWB19100001"},
              "artists":[{"name":"R.E.M."},{"name":"KRS-One"}]}],"next":null}}"#;
        let album: SpAlbum = serde_json::from_str(json).unwrap();
        assert_eq!(names(&album.artists), "R.E.M.");
        assert_eq!(album.genres, ["alternative rock"]);
        assert_eq!(album.label.as_deref(), Some("Warner Bros."));
        let tracks = album.tracks.unwrap();
        assert_eq!(names(&tracks.items[0].artists), "R.E.M., KRS-One");
        assert_eq!(
            tracks.items[0]
                .external_ids
                .as_ref()
                .and_then(|e| e.isrc.as_deref()),
            Some("USWB19100001")
        );
        assert_eq!(album.images[0].url, "https://i.scdn.co/image/x");
    }

    #[test]
    fn songs_land_on_their_tracks() {
        let release = Release {
            source: Source::Deezer,
            id: "1".into(),
            title: "A".into(),
            artist: "B".into(),
            year: None,
            genre: None,
            has_cover: false,
            extra: BTreeMap::new(),
            tracks: vec![
                ReleaseTrack {
                    disc: 1,
                    number: 1,
                    title: "One".into(),
                    artist: None,
                    genre: None,
                    length_ms: None,
                    extra: BTreeMap::new(),
                },
                ReleaseTrack {
                    disc: 1,
                    number: 2,
                    title: "Two (Remastered)".into(),
                    artist: None,
                    genre: None,
                    length_ms: None,
                    extra: BTreeMap::new(),
                },
            ],
        };
        let songs = vec![
            (7, "two".to_owned(), None),
            (8, "Unknown thing".to_owned(), None),
        ];
        assert_eq!(match_songs(&release, &songs), [(7, 1)]);
    }

    #[test]
    fn only_plain_ids_are_asked_for() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let none = Accounts::default();
        assert!(rt
            .block_on(release(Source::Deezer, &none, "../etc"))
            .is_err());
        assert!(rt.block_on(cover(Source::Itunes, &none, "1?x=y")).is_err());
        // Spotify without an account says so, before asking anything.
        let err = rt
            .block_on(release(Source::Spotify, &none, "abc"))
            .unwrap_err();
        assert!(err.to_string().contains("Spotify account"), "{err}");
    }
}
