//! Identifying songs by their audio: the fingerprint goes to AcoustID,
//! which knows which MusicBrainz recording it is; MusicBrainz then says
//! which album, year, and genres. For songs whose tags are missing or
//! wrong. Opt-in (it sends fingerprints online), and nothing is written to
//! the files.

use std::time::Duration;

use serde::Deserialize;
use tokio::time::Instant;

use crate::vibe::normalize;

const ACOUSTID: &str = "https://api.acoustid.org/v2/lookup";
const MUSICBRAINZ: &str = "https://musicbrainz.org/ws/2";
/// AcoustID allows 3 requests a second; MusicBrainz 1.
const ACOUSTID_GAP: Duration = Duration::from_millis(350);
const MUSICBRAINZ_GAP: Duration = Duration::from_millis(1100);
/// Matches below this AcoustID score aren't trusted.
const MIN_SCORE: f32 = 0.75;

#[derive(Debug, thiserror::Error)]
pub enum IdentifyError {
    #[error("{0}")]
    Network(String),
    #[error("AcoustID said: {0}")]
    AcoustId(String),
}

/// What a song's tags already say, to check matches against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hint {
    pub title: Option<String>,
    pub artist: Option<String>,
}

/// Who a song turned out to be.
#[derive(Clone, Debug, PartialEq)]
pub struct Identified {
    pub score: f32,
    pub recording: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_type: Option<String>,
    pub year: Option<i64>,
    pub genres: Vec<String>,
}

pub struct Identifier {
    client: reqwest::Client,
    key: String,
    last_acoustid: Option<Instant>,
    last_musicbrainz: Option<Instant>,
}

impl Identifier {
    /// `key` is the AcoustID application key; `contact` is sent to
    /// MusicBrainz, which asks apps to say who to contact.
    pub fn new(key: &str, contact: &str) -> Result<Self, IdentifyError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(format!(
                "DJINN/{} ( {} )",
                env!("CARGO_PKG_VERSION"),
                contact.trim()
            ))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| IdentifyError::Network(e.to_string()))?;
        Ok(Self {
            client,
            key: key.trim().to_owned(),
            last_acoustid: None,
            last_musicbrainz: None,
        })
    }

    /// Looks a fingerprint up. `None` if nothing trustworthy matched.
    pub async fn identify(
        &mut self,
        fingerprint: &str,
        duration_secs: u64,
        hint: &Hint,
    ) -> Result<Option<Identified>, IdentifyError> {
        pace(&mut self.last_acoustid, ACOUSTID_GAP).await;
        let duration = duration_secs.to_string();
        let response: AcoustIdResponse = self
            .client
            .post(ACOUSTID)
            .form(&[
                ("client", self.key.as_str()),
                ("duration", duration.as_str()),
                ("fingerprint", fingerprint),
                ("meta", "recordings releasegroups compress"),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|e| IdentifyError::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| IdentifyError::Network(e.to_string()))?;
        let Some(mut found) = choose(response, hint)? else {
            return Ok(None);
        };
        // Year and genres from MusicBrainz.
        pace(&mut self.last_musicbrainz, MUSICBRAINZ_GAP).await;
        let url = format!(
            "{MUSICBRAINZ}/recording/{}?inc=genres&fmt=json",
            found.recording
        );
        if let Ok(response) = self.client.get(&url).send().await {
            if let Ok(recording) = response.json::<MbRecording>().await {
                found.year = recording
                    .first_release_date
                    .as_deref()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse().ok());
                let mut genres = recording.genres;
                genres.sort_by_key(|g| std::cmp::Reverse(g.count));
                found.genres = genres.into_iter().take(5).map(|g| g.name).collect();
            }
        }
        Ok(Some(found))
    }
}

async fn pace(last: &mut Option<Instant>, gap: Duration) {
    if let Some(at) = *last {
        tokio::time::sleep_until(at + gap).await;
    }
    *last = Some(Instant::now());
}

#[derive(Deserialize)]
struct AcoustIdResponse {
    status: String,
    #[serde(default)]
    error: Option<AcoustIdError>,
    #[serde(default)]
    results: Vec<AcoustIdResult>,
}

#[derive(Deserialize)]
struct AcoustIdError {
    message: String,
}

#[derive(Deserialize)]
struct AcoustIdResult {
    score: f32,
    #[serde(default)]
    recordings: Vec<Recording>,
}

#[derive(Deserialize)]
struct Recording {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artists: Vec<Named>,
    #[serde(default)]
    releasegroups: Vec<ReleaseGroup>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    secondarytypes: Vec<String>,
}

#[derive(Deserialize)]
struct MbRecording {
    #[serde(default, rename = "first-release-date")]
    first_release_date: Option<String>,
    #[serde(default)]
    genres: Vec<MbGenre>,
}

#[derive(Deserialize)]
struct MbGenre {
    name: String,
    #[serde(default)]
    count: i64,
}

/// The best trustworthy recording in an AcoustID answer.
fn choose(response: AcoustIdResponse, hint: &Hint) -> Result<Option<Identified>, IdentifyError> {
    if response.status != "ok" {
        let message = response
            .error
            .map(|e| e.message)
            .unwrap_or_else(|| response.status.clone());
        return Err(IdentifyError::AcoustId(message));
    }
    let mut results = response.results;
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    for result in results.into_iter().filter(|r| r.score >= MIN_SCORE) {
        for recording in result.recordings {
            let Some(title) = recording.title.clone() else {
                continue;
            };
            let artist = recording
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(" & ");
            if !agrees(hint, &title, &artist) {
                continue;
            }
            let group = best_group(&recording.releasegroups);
            return Ok(Some(Identified {
                score: result.score,
                recording: recording.id,
                title,
                artist,
                album: group.and_then(|g| g.title.clone()),
                album_type: group.and_then(|g| g.kind.clone()),
                year: None,
                genres: Vec::new(),
            }));
        }
    }
    Ok(None)
}

/// Albums first, then EPs, then singles; compilations and live releases
/// last.
fn best_group(groups: &[ReleaseGroup]) -> Option<&ReleaseGroup> {
    groups.iter().min_by_key(|g| {
        let kind = match g.kind.as_deref() {
            Some("Album") => 0,
            Some("EP") => 1,
            Some("Single") => 2,
            _ => 3,
        };
        let secondary = if g.secondarytypes.is_empty() { 0 } else { 10 };
        kind + secondary
    })
}

/// A match must agree with the tags it has: the titles share their base,
/// and the artists share a name.
fn agrees(hint: &Hint, title: &str, artist: &str) -> bool {
    let base = |t: &str| -> String {
        let cut = [" (", " [", " - "]
            .iter()
            .filter_map(|m| t.find(m))
            .min()
            .unwrap_or(t.len());
        normalize(&t[..cut])
    };
    let title_ok = hint.title.as_deref().is_none_or(|h| {
        let (h, t) = (base(h), base(title));
        !h.is_empty() && (h == t || h.contains(&t) || t.contains(&h))
    });
    let artist_ok = hint.artist.as_deref().is_none_or(|h| {
        let theirs = normalize(artist);
        normalize(h)
            .split(" & ")
            .flat_map(|part| part.split(", "))
            .any(|name| !name.is_empty() && theirs.contains(name))
    });
    title_ok && artist_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(json: &str) -> AcoustIdResponse {
        serde_json::from_str(json).unwrap()
    }

    const FOUND: &str = r#"{"status":"ok","results":[
        {"score":0.62,"id":"x","recordings":[{"id":"low","title":"Taped Up Heart","artists":[{"name":"KREAM"}]}]},
        {"score":0.96,"id":"y","recordings":[
            {"id":"cover","title":"Something Else","artists":[{"name":"Other Band"}]},
            {"id":"rec-1","title":"Taped Up Heart","artists":[{"name":"KREAM"},{"name":"Clara Mae"}],
             "releasegroups":[
                {"title":"Dance Hits 2017","type":"Album","secondarytypes":["Compilation"]},
                {"title":"Taped Up Heart","type":"Single"},
                {"title":"KREAM Collected","type":"Album"}]}]}]}"#;

    #[test]
    fn picks_the_matching_recording_and_its_album() {
        let hint = Hint {
            title: Some("Taped Up Heart (feat. Clara Mae)".into()),
            artist: Some("KREAM".into()),
        };
        let found = choose(answer(FOUND), &hint).unwrap().unwrap();
        assert_eq!(
            found.recording, "rec-1",
            "the cover doesn't agree with the tags"
        );
        assert_eq!(found.artist, "KREAM & Clara Mae");
        assert_eq!(
            found.album.as_deref(),
            Some("KREAM Collected"),
            "an album, not a compilation"
        );
        assert!((found.score - 0.96).abs() < 1e-6);
    }

    #[test]
    fn untagged_songs_take_the_best_match() {
        let found = choose(answer(FOUND), &Hint::default()).unwrap().unwrap();
        assert_eq!(found.recording, "cover", "nothing to check against");
    }

    #[test]
    fn low_scores_and_errors() {
        let weak = r#"{"status":"ok","results":[{"score":0.5,"id":"x","recordings":[{"id":"a","title":"T"}]}]}"#;
        assert!(choose(answer(weak), &Hint::default()).unwrap().is_none());
        let error = r#"{"status":"error","error":{"code":4,"message":"invalid API key"}}"#;
        assert!(matches!(
            choose(answer(error), &Hint::default()),
            Err(IdentifyError::AcoustId(m)) if m == "invalid API key"
        ));
    }

    #[test]
    fn agreement_ignores_version_notes() {
        let hint = Hint {
            title: Some("Roads [AM1R Remix]".into()),
            artist: Some("KREAM & Dan Caplen".into()),
        };
        assert!(agrees(&hint, "Roads (feat. Dan Caplen)", "KREAM"));
        assert!(!agrees(&hint, "Rivers", "KREAM"));
        assert!(!agrees(&hint, "Roads", "Somebody Else"));
    }
}
