//! About DJINN and updates (admin): the version, and new releases on
//! GitHub. Checks once a day while that's on; installs when the admin asks,
//! or by itself when nothing is playing if the admin allows it. Downloads
//! must match a SHA256SUMS file signed with DJINN's release key (see
//! docs/releasing.md), then replace the program and restart it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_store::now_ms;
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::events::PlayState;
use crate::state::AppState;

pub const REPOSITORY: &str = "jmarasch/DJINN";
pub const CONTACT: &str = "DJINN@newterrastudios.com";
/// DJINN's release key; its secret half signs each release's SHA256SUMS.
const PUBLIC_KEY: &str = "af3998f0197817106cb123aaf315015b8867dd91d0acd60127e4e86bebb69f9e";
/// Set for the new process after an update restarts DJINN.
pub const RESTARTED: &str = "DJINN_RESTARTED";

/// Setting keys: check GitHub once a day; install updates by themselves.
const CHECK_UPDATES: &str = "update_check";
const AUTO_UPDATE: &str = "update_auto";
const CHECK_EVERY_MS: i64 = 24 * 60 * 60 * 1000;
/// How often the background job wakes to see if it's time to check or
/// install.
const TICK: Duration = Duration::from_secs(10 * 60);
/// Name of the half-downloaded update next to the program.
const PARTIAL: &str = ".djinn-update";

pub struct UpdateState {
    /// The program's path, taken at startup (on Linux it reads as
    /// "(deleted)" once replaced).
    exe: Option<PathBuf>,
    status: Mutex<Checked>,
    installing: AtomicBool,
    restart: AtomicBool,
}

#[derive(Clone, Default)]
struct Checked {
    at: Option<i64>,
    latest: Option<Release>,
    error: Option<String>,
    /// (done, total) bytes while downloading an update.
    progress: Option<(u64, u64)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Release {
    version: String,
    url: String,
    notes: String,
    published_at: Option<String>,
    #[serde(skip)]
    asset: Option<Asset>,
    #[serde(skip)]
    sums: Option<(String, String)>,
}

#[derive(Clone, Debug)]
struct Asset {
    name: String,
    url: String,
    size: u64,
}

impl UpdateState {
    pub fn new() -> Self {
        Self {
            exe: std::env::current_exe().ok(),
            status: Mutex::new(Checked::default()),
            installing: AtomicBool::new(false),
            restart: AtomicBool::new(false),
        }
    }

    /// True once an update is installed and DJINN should start again.
    pub fn restart_requested(&self) -> bool {
        self.restart.load(Ordering::Acquire)
    }

    pub fn exe(&self) -> Option<&Path> {
        self.exe.as_deref()
    }

    fn checked(&self) -> Checked {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut Checked)) {
        if let Ok(mut status) = self.status.lock() {
            f(&mut status);
        }
    }

    /// Why this copy can't update itself to `release`, if it can't.
    fn blocker(&self, release: &Release) -> Option<String> {
        let Some(exe) = &self.exe else {
            return Some("DJINN can't tell where it's installed.".into());
        };
        let parts: Vec<_> = exe.components().map(|c| c.as_os_str()).collect();
        if parts
            .windows(2)
            .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"))
        {
            return Some(
                "This copy runs from a build folder; update it with git and cargo.".into(),
            );
        }
        if release.asset.is_none() || release.sums.is_none() {
            return Some(format!(
                "This release has no download for {} {}.",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
        }
        None
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/about", get(about))
        .route("/update/check", post(check_now))
        .route("/update/install", post(install_now))
        .route("/update/settings", put(set_settings))
}

#[derive(Serialize)]
struct About {
    version: &'static str,
    commit: &'static str,
    commit_date: &'static str,
    repository: String,
    contact: &'static str,
    check_updates: bool,
    auto_update: bool,
    checked_at: Option<i64>,
    latest: Option<Release>,
    update_available: bool,
    /// Why the update can't be installed from here, if it can't.
    cannot_install: Option<String>,
    error: Option<String>,
    installing: bool,
    progress: Option<(u64, u64)>,
}

async fn about_info(state: &AppState) -> ApiResult<About> {
    let update = &state.update;
    let checked = update.checked();
    let available = checked.latest.as_ref().filter(|r| is_newer(&r.version));
    Ok(About {
        version: env!("CARGO_PKG_VERSION"),
        commit: env!("DJINN_COMMIT"),
        commit_date: env!("DJINN_COMMIT_DATE"),
        repository: format!("https://github.com/{REPOSITORY}"),
        contact: CONTACT,
        check_updates: crate::jukebox::setting(&state.store, CHECK_UPDATES, 1).await? == 1,
        auto_update: crate::jukebox::setting(&state.store, AUTO_UPDATE, 0).await? == 1,
        checked_at: checked.at,
        update_available: available.is_some(),
        cannot_install: available.and_then(|r| update.blocker(r)),
        latest: checked.latest,
        error: checked.error,
        installing: update.installing.load(Ordering::Acquire),
        progress: checked.progress,
    })
}

async fn about(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<About>> {
    Ok(Json(about_info(&state).await?))
}

async fn check_now(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<About>> {
    check(&state).await;
    Ok(Json(about_info(&state).await?))
}

async fn install_now(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let release = state
        .update
        .checked()
        .latest
        .filter(|r| is_newer(&r.version))
        .ok_or_else(|| ApiError::bad_request("no update to install"))?;
    if let Some(reason) = state.update.blocker(&release) {
        return Err(ApiError::bad_request(reason));
    }
    start_install(&state, release);
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct Settings {
    check_updates: bool,
    auto_update: bool,
}

async fn set_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Settings>,
) -> ApiResult<StatusCode> {
    let flag = |on: bool| if on { "1" } else { "0" };
    state
        .store
        .set_setting(CHECK_UPDATES, flag(body.check_updates))
        .await?;
    state
        .store
        .set_setting(AUTO_UPDATE, flag(body.auto_update))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Cleans up after an update and checks daily in the background.
pub fn start(state: &AppState) {
    if let Some(dir) = state.update.exe.as_deref().and_then(Path::parent) {
        let _ = std::fs::remove_file(dir.join(PARTIAL));
        let _ = std::fs::remove_file(dir.join("djinn.old.exe"));
    }
    let state = state.clone();
    tokio::spawn(async move {
        // Let startup settle first.
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            tick(&state).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick(state: &AppState) {
    let setting = |key, default| crate::jukebox::setting(&state.store, key, default);
    if setting(CHECK_UPDATES, 1).await.ok() != Some(1) {
        return;
    }
    let due = state
        .update
        .checked()
        .at
        .is_none_or(|at| now_ms() - at >= CHECK_EVERY_MS);
    if due {
        check(state).await;
    }
    if setting(AUTO_UPDATE, 0).await.ok() != Some(1) {
        return;
    }
    let Some(release) = state
        .update
        .checked()
        .latest
        .filter(|r| is_newer(&r.version))
    else {
        return;
    };
    // Never in the middle of the music.
    let playing = state.jukebox.lock().await.player_state().state == PlayState::Playing;
    if !playing && state.update.blocker(&release).is_none() {
        tracing::info!(version = %release.version, "installing an update by itself");
        start_install(state, release);
    }
}

/// The files of this version's release, with its SHA256SUMS already
/// checked against the release key.
pub struct ReleaseFiles {
    /// (name, download URL, size).
    pub files: Vec<(String, String, u64)>,
    pub sums: String,
}

/// This version's release on GitHub. `None` if there isn't one (or the
/// repository is private).
pub async fn this_release() -> Result<Option<ReleaseFiles>, String> {
    // For testing against a stand-in server, like DJINN_RELEASES_URL.
    let url = std::env::var("DJINN_THIS_RELEASE_URL").unwrap_or_else(|_| {
        format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/tags/v{}",
            env!("CARGO_PKG_VERSION")
        )
    });
    let client = client()?;
    let response = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("cannot reach GitHub: {e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release: GitHubRelease = response
        .error_for_status()
        .map_err(|e| format!("GitHub said: {e}"))?
        .json()
        .await
        .map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    let find = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
    };
    let (Some(sums_url), Some(sig_url)) = (find("SHA256SUMS"), find("SHA256SUMS.sig")) else {
        return Err("this release has no checksums".into());
    };
    let text = |url: String| {
        let client = client.clone();
        async move {
            client
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("download failed: {e}"))?
                .text()
                .await
                .map_err(|e| format!("download failed: {e}"))
        }
    };
    let sums = text(sums_url).await?;
    if !signed(sums.as_bytes(), &text(sig_url).await?, PUBLIC_KEY) {
        return Err("this release isn't signed with DJINN's release key".into());
    }
    Ok(Some(ReleaseFiles {
        files: release
            .assets
            .into_iter()
            .map(|a| (a.name, a.browser_download_url, a.size))
            .collect(),
        sums,
    }))
}

pub fn client() -> Result<reqwest::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("DJINN/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

/// Asks GitHub for the latest release.
async fn check(state: &AppState) {
    let result = latest().await;
    state.update.update(|s| {
        s.at = Some(now_ms());
        match result {
            Ok(latest) => {
                s.latest = latest;
                s.error = None;
            }
            Err(e) => s.error = Some(e),
        }
    });
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

async fn latest() -> Result<Option<Release>, String> {
    // For testing against a stand-in server; downloads still need the
    // release key's signature.
    let url = std::env::var("DJINN_RELEASES_URL")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"));
    let response = client()?
        .get(url)
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("cannot reach GitHub: {e}"))?;
    // No releases yet, or the repository is private.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let release: GitHubRelease = response
        .error_for_status()
        .map_err(|e| format!("GitHub said: {e}"))?
        .json()
        .await
        .map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    Ok(Some(from_github(release, &asset_name())))
}

fn from_github(release: GitHubRelease, wanted: &str) -> Release {
    let find = |name: &str| release.assets.iter().find(|a| a.name == name);
    let sums = find("SHA256SUMS")
        .zip(find("SHA256SUMS.sig"))
        .map(|(sums, sig)| {
            (
                sums.browser_download_url.clone(),
                sig.browser_download_url.clone(),
            )
        });
    let asset = find(wanted).map(|a| Asset {
        name: a.name.clone(),
        url: a.browser_download_url.clone(),
        size: a.size,
    });
    Release {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        url: release.html_url,
        notes: release
            .body
            .unwrap_or_default()
            .chars()
            .take(4000)
            .collect(),
        published_at: release.published_at,
        asset,
        sums,
    }
}

/// The release file for this computer: `djinn-linux-x86_64`,
/// `djinn-windows-x86_64.exe`, and so on.
fn asset_name() -> String {
    format!(
        "djinn-{}-{}{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::EXE_SUFFIX
    )
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// True if `version` is newer than this copy.
fn is_newer(version: &str) -> bool {
    match (
        parse_version(version),
        parse_version(env!("CARGO_PKG_VERSION")),
    ) {
        (Some(theirs), Some(ours)) => theirs > ours,
        _ => false,
    }
}

/// True if `signature` (hex) is DJINN's release key signing `data`.
fn signed(data: &[u8], signature: &str, key: &str) -> bool {
    let (Some(key), Some(signature)) = (unhex::<32>(key), unhex::<64>(signature.trim())) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key) else {
        return false;
    };
    key.verify_strict(data, &Signature::from_bytes(&signature))
        .is_ok()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The sum for `name` in a SHA256SUMS file ("<hex>  <name>" lines).
pub fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| sum.to_lowercase())
    })
}

fn start_install(state: &AppState, release: Release) {
    if state.update.installing.swap(true, Ordering::AcqRel) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        state.update.update(|s| s.error = None);
        match install(&state, &release).await {
            Ok(()) => {
                tracing::info!(version = %release.version, "update installed; restarting");
                state.update.restart.store(true, Ordering::Release);
                state.shutdown_request.notify_one();
            }
            Err(e) => {
                tracing::error!(error = %e, "installing the update failed");
                state.update.update(|s| s.error = Some(e));
            }
        }
        state.update.update(|s| s.progress = None);
        state.update.installing.store(false, Ordering::Release);
    });
}

async fn install(state: &AppState, release: &Release) -> Result<(), String> {
    let (Some(exe), Some(asset), Some((sums_url, sig_url))) =
        (state.update.exe.clone(), &release.asset, &release.sums)
    else {
        return Err("nothing to install for this computer".into());
    };
    let dir = exe
        .parent()
        .ok_or("DJINN can't tell where it's installed")?;
    let client = client()?;
    let fetch = |url: String| {
        let client = client.clone();
        async move {
            client
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("download failed: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("download failed: {e}"))
        }
    };
    let sums = fetch(sums_url.clone()).await?;
    let signature = fetch(sig_url.clone()).await?;
    if !signed(&sums, &String::from_utf8_lossy(&signature), PUBLIC_KEY) {
        return Err(
            "this release isn't signed with DJINN's release key, so it wasn't installed".into(),
        );
    }
    let expected = expected_sum(&String::from_utf8_lossy(&sums), &asset.name)
        .ok_or("the release's checksums don't list this download")?;

    let partial = dir.join(PARTIAL);
    let mut out = tokio::fs::File::create(&partial)
        .await
        .map_err(|e| format!("cannot write to {}: {e}", dir.display()))?;
    let response = client
        .get(&asset.url)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("download failed: {e}"))?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download broke off: {e}"))?;
        hasher.update(&chunk);
        out.write_all(&chunk).await.map_err(|e| e.to_string())?;
        written += chunk.len() as u64;
        state
            .update
            .update(|s| s.progress = Some((written, asset.size)));
    }
    out.flush().await.map_err(|e| e.to_string())?;
    drop(out);
    let sum: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if written != asset.size || sum != expected {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err("the download came down damaged; try again".into());
    }
    replace(&exe, &partial).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        format!("cannot replace {}: {e}", exe.display())
    })
}

/// Puts `new` in place of the running program at `exe`.
fn replace(exe: &Path, new: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))?;
        // Replacing the file is fine while it runs.
        std::fs::rename(new, exe)
    }
    #[cfg(not(unix))]
    {
        // A running .exe can be renamed but not overwritten; the old one is
        // deleted at the next start.
        let old = exe.with_file_name("djinn.old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old)?;
        std::fs::rename(new, exe).inspect_err(|_| {
            let _ = std::fs::rename(&old, exe);
        })
    }
}

/// Starts the (new) program again with the same arguments. On Unix this
/// replaces the process; elsewhere it starts a new one and this one ends.
pub fn restart(exe: &Path) -> std::io::Result<()> {
    let mut command = std::process::Command::new(exe);
    command
        .args(std::env::args_os().skip(1))
        .env(RESTARTED, "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec())
    }
    #[cfg(not(unix))]
    {
        command.spawn().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10"), Some((0, 10, 0)));
        assert_eq!(parse_version("2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("v99.0.0"));
        assert!(!is_newer(env!("CARGO_PKG_VERSION")));
        assert!(!is_newer("0.0.1"));
    }

    #[test]
    fn checks_the_signature() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let public: String = key
            .verifying_key()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sums = b"abc123  djinn-linux-x86_64\n";
        let signature: String = key
            .sign(sums)
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(signed(sums, &format!("{signature}\n"), &public));
        assert!(!signed(
            b"tampered  djinn-linux-x86_64\n",
            &signature,
            &public
        ));
        assert!(!signed(sums, &signature, PUBLIC_KEY), "a different key");
        assert!(!signed(sums, "not hex", &public));
    }

    #[test]
    fn reads_release_files() {
        let sums = "AA11  djinn-linux-x86_64\nbb22 *djinn-windows-x86_64.exe\n";
        assert_eq!(
            expected_sum(sums, "djinn-windows-x86_64.exe").as_deref(),
            Some("bb22")
        );
        assert_eq!(
            expected_sum(sums, "djinn-linux-x86_64").as_deref(),
            Some("aa11")
        );
        assert_eq!(expected_sum(sums, "djinn-macos-aarch64"), None);

        let asset = |name: &str| GitHubAsset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 10,
        };
        let release = from_github(
            GitHubRelease {
                tag_name: "v0.2.0".into(),
                html_url: "https://github.com/x".into(),
                body: Some("Notes".into()),
                published_at: None,
                assets: vec![
                    asset("djinn-linux-x86_64"),
                    asset("SHA256SUMS"),
                    asset("SHA256SUMS.sig"),
                ],
            },
            "djinn-linux-x86_64",
        );
        assert_eq!(release.version, "0.2.0");
        assert!(release.asset.is_some() && release.sums.is_some());
        let state = UpdateState {
            exe: Some(PathBuf::from("/home/me/.local/bin/djinn")),
            ..UpdateState::new()
        };
        assert_eq!(state.blocker(&release), None);
        let dev = UpdateState {
            exe: Some(PathBuf::from("/src/djinn/target/release/djinn")),
            ..UpdateState::new()
        };
        assert!(dev.blocker(&release).is_some(), "a build folder");
    }
}
