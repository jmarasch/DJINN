//! What a node needs to help with AI lyrics, served by Main so nodes never
//! go to the internet: the Python packages for the node's own Python (Main
//! fetches them once with its listener's pip, from the keepsake when it has
//! one, else from the package index) and the Whisper model the node uses
//! (Main's own copy, downloaded once and checked against the pins). The
//! listener's program itself the node writes from its own DJINN, which is
//! Main's version since nodes update from Main.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::ai_listener;
use crate::state::AppState;

/// One file of a kit, by its path in the listener's folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitFile {
    /// "wheels/<file>.whl" or "models/faster-whisper-<model>/<file>".
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// How a kit stands.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Kit {
    Ready { files: Vec<KitFile> },
    Preparing,
    Failed { error: String },
}

static KITS: Mutex<Option<HashMap<String, Kit>>> = Mutex::new(None);

fn kits<T>(f: impl FnOnce(&mut HashMap<String, Kit>) -> T) -> T {
    let mut guard = KITS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// "3.11" style: digits, a dot, digits.
fn valid_python(version: &str) -> bool {
    let mut parts = version.split('.');
    let ok = |p: Option<&str>| {
        p.is_some_and(|p| !p.is_empty() && p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()))
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// The platforms pip may pick wheels for on a node of this kind.
fn platforms(arch: &str) -> Option<Vec<String>> {
    if !matches!(arch, "x86_64" | "aarch64") {
        return None;
    }
    Some(
        [
            "manylinux2014",
            "manylinux_2_17",
            "manylinux_2_27",
            "manylinux_2_28",
            "linux",
        ]
        .iter()
        .map(|p| format!("{p}_{arch}"))
        .collect(),
    )
}

/// The kit for a node with this Python and processor, using `model`: ready,
/// being put together (ask again), or why it can't be.
pub fn kit(state: &AppState, python: &str, arch: &str, model: &str) -> Kit {
    if !valid_python(python) || platforms(arch).is_none() {
        return Kit::Failed {
            error: format!("nodes with Python {python} on {arch} can't help yet"),
        };
    }
    if !ai_listener::MODELS.contains(&model) {
        return Kit::Failed {
            error: "unknown model".into(),
        };
    }
    let key = format!("{python}-{arch}-{model}");
    let known = kits(|k| k.get(&key).cloned());
    match &known {
        Some(Kit::Failed { .. }) | None => {}
        Some(kit) => return kit.clone(),
    }
    // A failure is tried again when asked again, a few minutes on.
    if let Some(Kit::Failed { error }) = known {
        if !retry_due(&key) {
            return Kit::Failed { error };
        }
    }
    kits(|k| k.insert(key.clone(), Kit::Preparing));
    let d = ai_listener::dir(state);
    let (python, arch, model) = (python.to_owned(), arch.to_owned(), model.to_owned());
    tokio::spawn(async move {
        let done = match build(&d, &python, &arch, &model).await {
            Ok(files) => Kit::Ready { files },
            Err(error) => {
                tracing::warn!(%error, "couldn't put together the AI kit for a node");
                note_failure(&key);
                Kit::Failed { error }
            }
        };
        kits(|k| k.insert(key, done));
    });
    Kit::Preparing
}

static FAILED_AT: Mutex<Option<HashMap<String, std::time::Instant>>> = Mutex::new(None);
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(5 * 60);

fn note_failure(key: &str) {
    let mut guard = FAILED_AT.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(HashMap::new)
        .insert(key.to_owned(), std::time::Instant::now());
}

fn retry_due(key: &str) -> bool {
    let guard = FAILED_AT.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|m| m.get(key))
        .is_none_or(|at| at.elapsed() >= RETRY_AFTER)
}

/// Where a kit's file is on Main, if `path` is one of the kit's.
pub fn file_of(
    state: &AppState,
    python: &str,
    arch: &str,
    model: &str,
    path: &str,
) -> Option<PathBuf> {
    let key = format!("{python}-{arch}-{model}");
    let listed = kits(|k| match k.get(&key) {
        Some(Kit::Ready { files }) => files.iter().any(|f| f.path == path),
        _ => false,
    });
    if !listed {
        return None;
    }
    let d = ai_listener::dir(state);
    Some(match path.strip_prefix("wheels/") {
        Some(wheel) => wheels_dir(&d, python, arch).join(wheel),
        None => d.join(path),
    })
}

fn wheels_dir(d: &Path, python: &str, arch: &str) -> PathBuf {
    d.join("node-kit").join(format!("py{python}-{arch}"))
}

async fn build(d: &Path, python: &str, arch: &str, model: &str) -> Result<Vec<KitFile>, String> {
    if !ai_listener::installed(d) {
        return Err(
            "install the lyrics listener on Main first (Admin > AI); nodes get theirs from it"
                .into(),
        );
    }
    let wheels = wheels_dir(d, python, arch);
    // Kits made before they carried pip are made again.
    let has_pip_wheel = std::fs::read_dir(&wheels).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("pip-"))
    });
    if !wheels.join(".done").is_file() || !has_pip_wheel {
        fetch_wheels(d, &wheels, python, arch).await?;
    }
    ensure_model(d, model).await?;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&wheels).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".whl") {
            files.push(describe(&entry.path(), format!("wheels/{name}")).await?);
        }
    }
    let model_dir = format!("models/faster-whisper-{model}");
    for entry in std::fs::read_dir(d.join(&model_dir)).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map_err(|e| e.to_string())?.is_file() && !name.starts_with('.') {
            files.push(describe(&entry.path(), format!("{model_dir}/{name}")).await?);
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Downloads the packages a node's Python needs: from the keepsake when
/// it has them all, else from the package index (once).
async fn fetch_wheels(d: &Path, wheels: &Path, python: &str, arch: &str) -> Result<(), String> {
    let _ = tokio::fs::remove_dir_all(wheels).await;
    tokio::fs::create_dir_all(wheels)
        .await
        .map_err(|e| e.to_string())?;
    let platforms = platforms(arch).unwrap_or_default();
    let download = |offline: Option<PathBuf>| {
        let mut c = Command::new(ai_listener::venv_python(d));
        c.current_dir(d)
            .args([
                "-m",
                "pip",
                "download",
                "--disable-pip-version-check",
                "--quiet",
                "--only-binary=:all:",
                "--implementation",
                "cp",
                "--python-version",
                python,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for p in &platforms {
            c.args(["--platform", p]);
        }
        if let Some(keep) = offline {
            c.arg("--no-index").arg("--find-links").arg(keep);
        }
        // pip itself too, for a node whose Python came without it.
        c.args(["-r", "requirements.txt", "pip", "-d"]).arg(wheels);
        c
    };
    let from_keep = ai_listener::keepsake(d)
        .map(|k| k.join("wheelhouse").join("core"))
        .filter(|w| w.is_dir());
    let mut ok = false;
    if let Some(keep) = from_keep {
        ok = download(Some(keep))
            .status()
            .await
            .is_ok_and(|s| s.success());
    }
    if !ok {
        tracing::info!(python, arch, "fetching the AI packages for a node (once)");
        ok = download(None).status().await.is_ok_and(|s| s.success());
    }
    if !ok {
        return Err(format!(
            "couldn't get the listener's packages for Python {python} on {arch}"
        ));
    }
    tokio::fs::write(wheels.join(".done"), "1")
        .await
        .map_err(|e| e.to_string())
}

/// Main's copy of `model`: from the keepsake, else downloaded once (the
/// worker checks it against the pins).
async fn ensure_model(d: &Path, model: &str) -> Result<(), String> {
    let name = format!("faster-whisper-{model}");
    let target = d.join("models").join(&name);
    if target.join("model.bin").is_file() && target.join("config.json").is_file() {
        return Ok(());
    }
    if let Some(kept) = ai_listener::keepsake(d).map(|k| k.join("models").join(&name)) {
        if kept.is_dir() {
            ai_listener::copy_dir(&kept, &target).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    tracing::info!(model, "downloading a Whisper model for a node (once)");
    let ok = Command::new(ai_listener::venv_python(d))
        .current_dir(d)
        .args(["worker.py", "--setup", "--model", model, "--vocals", "off"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        Err(format!("couldn't download the {model} model"))
    }
}

/// A file's size, change time and SHA-256, as last worked out.
type Known = (u64, std::time::SystemTime, String);

/// A file's size and SHA-256, worked out once per size and change time.
async fn describe(path: &Path, as_path: String) -> Result<KitFile, String> {
    static SUMS: Mutex<Option<HashMap<PathBuf, Known>>> = Mutex::new(None);
    let meta = tokio::fs::metadata(path).await.map_err(|e| e.to_string())?;
    let (size, modified) = (meta.len(), meta.modified().map_err(|e| e.to_string())?);
    let known = SUMS
        .lock()
        .ok()
        .and_then(|g| g.as_ref()?.get(path).cloned())
        .filter(|(s, m, _)| *s == size && *m == modified)
        .map(|(_, _, sum)| sum);
    let sha256 = match known {
        Some(sum) => sum,
        None => {
            let owned = path.to_owned();
            let sum = tokio::task::spawn_blocking(move || sha256_file(&owned))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            if let Ok(mut guard) = SUMS.lock() {
                guard
                    .get_or_insert_with(HashMap::new)
                    .insert(path.to_owned(), (size, modified, sum.clone()));
            }
            sum
        }
    };
    Ok(KitFile {
        path: as_path,
        size,
        sha256,
    })
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::security::hex(&hasher.finalize()))
}

/// Whether a kit path is one a node may write: a wheel, or a model file,
/// with no way out of the listener's folder.
pub fn safe_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && !p.starts_with('.')
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    };
    match parts.as_slice() {
        ["wheels", file] => plain(file) && file.ends_with(".whl"),
        ["models", folder, file] => {
            folder.starts_with("faster-whisper-") && plain(folder) && plain(file)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_versions_and_known_processors() {
        assert!(valid_python("3.11"));
        assert!(valid_python("3.13"));
        assert!(!valid_python("3"));
        assert!(!valid_python("3.11; rm"));
        assert!(!valid_python("3.11.2"));
        assert!(platforms("x86_64").is_some());
        assert!(platforms("../x").is_none());
    }

    #[test]
    fn kit_paths_stay_inside_the_listener() {
        assert!(safe_path(
            "wheels/numpy-2.0-cp311-cp311-manylinux_2_17_x86_64.whl"
        ));
        assert!(safe_path("models/faster-whisper-small/model.bin"));
        assert!(!safe_path("wheels/../../etc/passwd"));
        assert!(!safe_path("models/../worker.py"));
        assert!(!safe_path("models/faster-whisper-small/.secret"));
        assert!(!safe_path("worker.py"));
        assert!(!safe_path("/etc/passwd"));
    }
}

#[cfg(test)]
mod real {
    /// Builds a node's kit in `DJINN_KIT_TEST_DIR` (a folder with the
    /// listener installed): `DJINN_KIT_TEST_DIR=... cargo test -p djinn-server
    /// kit_for_a_node -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "needs an installed listener and the network or a keepsake"]
    async fn kit_for_a_node() {
        let d = std::path::PathBuf::from(std::env::var("DJINN_KIT_TEST_DIR").unwrap());
        crate::ai_listener::write_pack(&d).await.unwrap();
        let files = super::build(&d, "3.13", "x86_64", "tiny").await.unwrap();
        let total: u64 = files.iter().map(|f| f.size).sum();
        println!("{} files, {} MB", files.len(), total / 1_000_000);
        assert!(files
            .iter()
            .any(|f| f.path.starts_with("wheels/ctranslate2")));
        assert!(files
            .iter()
            .any(|f| f.path == "models/faster-whisper-tiny/model.bin"));
        assert!(files.iter().all(|f| super::safe_path(&f.path)));
    }
}
