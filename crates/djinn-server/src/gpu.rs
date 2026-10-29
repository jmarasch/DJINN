//! The optional GPU pack (admin): the AI reader built for NVIDIA graphics
//! cards (`djinn-gpu`, see djinn-ai) and the NVIDIA CUDA libraries it
//! needs, downloaded from this version's GitHub release and checked
//! against its signed SHA256SUMS. DJINN itself stays one download that
//! runs on the processor everywhere; with the pack it tries the graphics
//! card first and falls back to the processor.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use djinn_ai::Reading;
use futures_util::StreamExt;
use serde::Deserialize;

use crate::ai::Fetch;
use crate::download::CheckedFile;

/// Roughly how much the pack downloads, before the release says exactly.
pub const PACK_ESTIMATE: u64 = 300_000_000;
/// Written last, so a half-finished pack doesn't count as installed.
const COMPLETE: &str = "complete";
/// Loading compiles the GPU code the first time, which takes a while.
const LOAD_WAIT: Duration = Duration::from_secs(180);
const READ_WAIT: Duration = Duration::from_secs(30);

/// Where this version's pack lives under DJINN's models folder.
pub fn pack_dir(models_dir: &Path) -> PathBuf {
    models_dir.join(format!("gpu-{}", env!("CARGO_PKG_VERSION")))
}

pub fn installed(dir: &Path) -> bool {
    dir.join(COMPLETE).exists() && dir.join(helper_name()).exists()
}

fn helper_name() -> String {
    format!("djinn-gpu{}", std::env::consts::EXE_SUFFIX)
}

/// Release files of the pack for this computer start with this, then the
/// file's own name, then `.zst`.
fn asset_prefix() -> String {
    format!("gpu-{}-{}.", std::env::consts::OS, std::env::consts::ARCH)
}

/// True if an NVIDIA graphics driver is installed (the pack can only help
/// then).
pub fn nvidia_present() -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc/driver/nvidia/version").exists()
    }
    #[cfg(windows)]
    {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .is_some_and(|root| root.join("System32").join("nvcuda.dll").exists())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        false
    }
}

/// Downloads the pack into `dir`, reporting progress through `fetch` and
/// calling `progressed` now and then.
pub async fn download(
    dir: &Path,
    fetch: &Fetch,
    progressed: impl Fn() -> futures_util::future::BoxFuture<'static, ()>,
) -> Result<(), String> {
    let release = crate::update::this_release().await?.ok_or(
        "GPU packs come with published releases, and there isn't one for this version yet",
    )?;
    let prefix = asset_prefix();
    let files: Vec<&(String, String, u64)> = release
        .files
        .iter()
        .filter(|(name, _, _)| name.starts_with(&prefix) && name.ends_with(".zst"))
        .collect();
    if !files
        .iter()
        .any(|(name, _, _)| name.contains(&helper_name()))
    {
        return Err(format!(
            "this release has no GPU pack for {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }
    let total: u64 = files.iter().map(|(_, _, size)| size).sum();
    fetch.set_progress(Some((0, total)));
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let _ = tokio::fs::remove_file(dir.join(COMPLETE)).await;
    let client = crate::update::client()?;
    let mut done = 0u64;
    for (name, url, size) in files {
        let expected = crate::update::expected_sum(&release.sums, name)
            .ok_or_else(|| format!("{name} isn't in the release's checksums"))?;
        let file = name
            .strip_prefix(&prefix)
            .and_then(|n| n.strip_suffix(".zst"))
            .filter(|n| !n.is_empty() && !n.contains(['/', '\\']) && *n != COMPLETE)
            .ok_or_else(|| format!("unexpected file {name}"))?;
        let packed = dir.join(format!("{file}.zst"));
        let mut out = CheckedFile::create(&packed)
            .await
            .map_err(|e| e.to_string())?;
        let response = client
            .get(url)
            .timeout(Duration::from_secs(1800))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("cannot download {name}: {e}"))?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if fetch.cancelled() {
                out.discard().await;
                return Err("stopped".into());
            }
            let chunk = chunk.map_err(|e| format!("download of {name} broke off: {e}"))?;
            let written = out.write(&chunk).await?;
            fetch.set_progress(Some((done + written, total)));
            if written % (4 << 20) < chunk.len() as u64 {
                progressed().await;
            }
        }
        out.finish(*size, &expected, name).await?;
        let target = dir.join(file);
        let unpacked = packed.clone();
        tokio::task::spawn_blocking(move || unpack(&unpacked, &target))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("cannot unpack {name}: {e}"))?;
        let _ = tokio::fs::remove_file(&packed).await;
        done += size;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.join(helper_name()),
            std::fs::Permissions::from_mode(0o755),
        )
        .map_err(|e| e.to_string())?;
    }
    tokio::fs::write(dir.join(COMPLETE), env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Decompresses a zstd file (one or more frames).
fn unpack(packed: &Path, target: &Path) -> std::io::Result<()> {
    let mut input = BufReader::new(std::fs::File::open(packed)?);
    let mut output = std::io::BufWriter::new(std::fs::File::create(target)?);
    while !input.fill_buf()?.is_empty() {
        let mut frame = ruzstd::decoding::StreamingDecoder::new(&mut input)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::io::copy(&mut frame, &mut output)?;
    }
    output.flush()
}

/// The `djinn-gpu` helper, running.
pub struct Helper {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

#[derive(Deserialize)]
struct Ready {
    #[serde(default)]
    ready: bool,
    #[serde(default)]
    gpu: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    reading: Option<Reading>,
    #[serde(default)]
    error: Option<String>,
}

impl Helper {
    /// Starts the helper from `pack` with the model in `model`, and waits
    /// until it has loaded onto the graphics card.
    pub fn start(pack: &Path, model: &Path) -> Result<Self, String> {
        let mut command = Command::new(pack.join(helper_name()));
        command
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // The CUDA libraries sit next to it.
        #[cfg(target_os = "linux")]
        {
            let mut path = std::ffi::OsString::from(pack.as_os_str());
            if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
                path.push(":");
                path.push(old);
            }
            command.env("LD_LIBRARY_PATH", path);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window for the helper.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start the GPU helper: {e}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err("cannot talk to the GPU helper".into());
        };
        let (send, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            while reader.by_ref().read_line(&mut line).is_ok_and(|n| n > 0) {
                if send.send(std::mem::take(&mut line)).is_err() {
                    break;
                }
            }
        });
        let mut helper = Self {
            child,
            stdin,
            lines,
        };
        let first = helper.next_line(LOAD_WAIT)?;
        let ready: Ready = serde_json::from_str(&first)
            .map_err(|_| "the GPU helper said something unexpected".to_owned())?;
        if let Some(error) = ready.error {
            return Err(format!("the GPU helper couldn't load: {error}"));
        }
        if !ready.ready || !ready.gpu {
            return Err("the graphics card couldn't be used".into());
        }
        Ok(helper)
    }

    fn next_line(&mut self, wait: Duration) -> Result<String, String> {
        self.lines.recv_timeout(wait).map_err(|e| match e {
            RecvTimeoutError::Timeout => "the GPU helper stopped answering".to_owned(),
            RecvTimeoutError::Disconnected => "the GPU helper stopped".to_owned(),
        })
    }

    pub fn read(&mut self, query: &str) -> Result<Reading, String> {
        let line = serde_json::json!({ "read": query }).to_string();
        writeln!(self.stdin, "{line}")
            .and_then(|()| self.stdin.flush())
            .map_err(|_| "the GPU helper stopped".to_owned())?;
        let answer: Answer = serde_json::from_str(&self.next_line(READ_WAIT)?)
            .map_err(|_| "the GPU helper said something unexpected".to_owned())?;
        match (answer.reading, answer.error) {
            (Some(reading), _) => Ok(reading),
            (None, error) => Err(error.unwrap_or_else(|| "no answer".into())),
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
