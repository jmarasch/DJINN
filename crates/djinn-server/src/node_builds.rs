//! DJINN updates for nodes, which take them from their main DJINN and never
//! from the internet on their own (the way phones get GO's app from it).
//!
//! When a node checks in with an older version, Main fetches the release
//! file for that node's processor from its own release (checked against the
//! signed SHA256SUMS, like Main's own updates), keeps it in
//! `<db dir>/node-builds/<version>/`, and offers it in the node's next
//! check-in. The node downloads it over its pinned connection, checks the
//! sum, puts it in place and starts again. Only ever forward: an older
//! program can't open a newer node's database.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::State;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ApiError, ApiResult};
use crate::nodes::NodeAuth;
use crate::state::AppState;

/// This program's version, which nodes are brought up to.
const VERSION: &str = env!("DJINN_RELEASE_VERSION");
/// After a failed fetch, how long before trying that file again.
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

/// Files being fetched now, and when each last failed.
type Fetching = (HashSet<String>, HashMap<String, Instant>);
static FETCHING: Mutex<Option<Fetching>> = Mutex::new(None);

/// An update offered to a node in its check-in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeUpdate {
    pub version: String,
    /// SHA-256 of the file, in hex.
    pub sha256: String,
}

/// The release file for a node's platform ("linux-aarch64" and so on), if
/// releases have one.
fn asset(platform: &str) -> Option<String> {
    matches!(platform, "linux-x86_64" | "linux-aarch64" | "linux-arm")
        .then(|| format!("djinn-{platform}"))
}

fn builds_dir(state: &AppState) -> PathBuf {
    state
        .art_dir
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        .join("node-builds")
        .join(VERSION)
}

/// Whether a node at `theirs` should be brought up to `ours`.
fn should_update(ours: &str, theirs: Option<&str>) -> bool {
    theirs.is_some_and(|theirs| crate::update::newer(ours, theirs))
}

/// The update for `node`, if it's behind and the file is ready. Starts
/// fetching the file if it isn't there yet (offered at a later check-in).
pub fn offer(state: &AppState, node: &djinn_store::Node) -> Option<NodeUpdate> {
    if node.status != "approved" || !should_update(VERSION, node.version.as_deref()) {
        return None;
    }
    let name = asset(node.platform.as_deref()?)?;
    let dir = builds_dir(state);
    let sum = std::fs::read_to_string(dir.join(format!("{name}.sha256"))).ok();
    match sum {
        Some(sum) if dir.join(&name).is_file() => Some(NodeUpdate {
            version: VERSION.to_owned(),
            sha256: sum.trim().to_owned(),
        }),
        _ => {
            fetch(dir, name);
            None
        }
    }
}

/// Fetches a release file in the background, once at a time, and not again
/// for a while after it failed.
fn fetch(dir: PathBuf, name: String) {
    {
        let Ok(mut guard) = FETCHING.lock() else {
            return;
        };
        let (running, failed) = guard.get_or_insert_with(Default::default);
        if running.contains(&name)
            || failed
                .get(&name)
                .is_some_and(|at| at.elapsed() < RETRY_AFTER)
        {
            return;
        }
        running.insert(name.clone());
    }
    tokio::spawn(async move {
        let result = download(&dir, &name).await;
        if let Ok(mut guard) = FETCHING.lock() {
            let (running, failed) = guard.get_or_insert_with(Default::default);
            running.remove(&name);
            match &result {
                Ok(()) => {
                    failed.remove(&name);
                }
                Err(_) => {
                    failed.insert(name.clone(), Instant::now());
                }
            }
        }
        match result {
            Ok(()) => tracing::info!(file = %name, version = VERSION, "node update ready"),
            Err(e) => tracing::warn!(file = %name, error = %e, "couldn't get a node update"),
        }
    });
}

async fn download(dir: &Path, name: &str) -> Result<(), String> {
    let release = crate::update::this_release()
        .await?
        .ok_or("there's no release of this version to take node updates from")?;
    let url = release
        .files
        .iter()
        .find(|file| file.0 == name)
        .map(|file| file.1.clone())
        .ok_or_else(|| format!("this release has no {name}"))?;
    let expected = crate::update::expected_sum(&release.sums, name)
        .ok_or_else(|| format!("{name} isn't in the release's checksums"))?;
    let bytes = crate::update::client()?
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    let got = hex(&Sha256::digest(&bytes));
    if got != expected {
        return Err(format!("{name} doesn't match the signed checksum"));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = dir.join(format!("{name}.part"));
    std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&part, dir.join(name)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{name}.sha256")), &got).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The update file for the asking node's processor.
pub async fn serve(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    use tower::ServiceExt;
    if node.status != "approved" {
        return Err(ApiError::forbidden());
    }
    let name = node
        .platform
        .as_deref()
        .and_then(asset)
        .ok_or_else(ApiError::not_found)?;
    let path = builds_dir(&state).join(name);
    if !path.is_file() {
        return Err(ApiError::not_found());
    }
    let response = tower_http::services::ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(axum::response::IntoResponse::into_response(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_only_move_forward() {
        assert!(should_update(
            "2026.11.9-nightly",
            Some("2026.11.8-nightly")
        ));
        assert!(
            should_update("2026.11.9", Some("2026.11.9-nightly")),
            "stable after its nightly"
        );
        assert!(
            !should_update("2026.11.9-nightly", Some("2026.11.9-nightly")),
            "same"
        );
        assert!(
            !should_update("2026.11.8", Some("2026.11.9-nightly")),
            "never back"
        );
        assert!(!should_update("2026.11.9", None), "unknown version");
    }

    #[test]
    fn release_files_by_processor() {
        assert_eq!(
            asset("linux-aarch64").as_deref(),
            Some("djinn-linux-aarch64")
        );
        assert_eq!(asset("linux-arm").as_deref(), Some("djinn-linux-arm"));
        assert_eq!(asset("windows-x86_64"), None);
    }
}
