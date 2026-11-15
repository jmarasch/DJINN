//! A node helping with AI lyrics. When Main's admin switches it on, the
//! node writes the lyrics listener from its own DJINN, gets the Python
//! packages and the Whisper model from Main (never the internet), makes a
//! private environment, tests its speed, and runs the worker. The worker
//! talks only to this node (`/api/ai-work/*` here, with a key of its own),
//! which passes its asks on to Main over the pinned connection, and only
//! while the node plays nothing and no screen draws visuals: the worker is
//! paused (stopped in place) the moment music or visuals start, and carries
//! on when they end. Main hands nodes songs whose vocals it has isolated
//! when it can, so the node only listens.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::ai_kit::KitFile;
use crate::ai_listener;
use crate::ai_work::NodeAiReport;
use crate::node_mode::Node;
use crate::nodes::NodeAiWanted;

/// How often the helper looks at what it should be doing.
const TICK: Duration = Duration::from_secs(2);
/// After a failed install, how long before trying again.
const RETRY: Duration = Duration::from_secs(10 * 60);
/// While Main puts the kit together, how often to ask again.
const KIT_WAIT: Duration = Duration::from_secs(15);

/// The helper's state, shared with the node's page and routes.
pub struct Helper {
    dir: PathBuf,
    /// What Main wants, from each check-in.
    wanted: Mutex<Option<NodeAiWanted>>,
    report: Mutex<NodeAiReport>,
    /// The report Main has, so only changes are sent.
    sent: Mutex<Option<NodeAiReport>>,
    child: tokio::sync::Mutex<Option<Child>>,
    paused: AtomicBool,
    /// The worker's key for this node's `/api/ai-work` routes.
    key: String,
    failed_at: Mutex<Option<std::time::Instant>>,
}

impl Helper {
    pub fn new(dir: PathBuf) -> anyhow::Result<Helper> {
        let memory_mb = djinn_platform::system::memory().map(|m| m.total / (1024 * 1024));
        let threads = std::thread::available_parallelism()
            .ok()
            .and_then(|n| u32::try_from(n.get()).ok());
        Ok(Helper {
            dir,
            wanted: Mutex::new(None),
            report: Mutex::new(NodeAiReport {
                phase: "off".into(),
                threads,
                memory_mb,
                ..NodeAiReport::default()
            }),
            sent: Mutex::new(None),
            child: tokio::sync::Mutex::new(None),
            paused: AtomicBool::new(false),
            key: crate::security::random_hex(24)?,
            failed_at: Mutex::new(None),
        })
    }

    /// What Main said in a check-in.
    pub fn set_wanted(&self, wanted: Option<NodeAiWanted>) {
        if let Ok(mut w) = self.wanted.lock() {
            *w = wanted;
        }
    }

    fn wanted(&self) -> Option<NodeAiWanted> {
        self.wanted.lock().ok().and_then(|w| w.clone())
    }

    pub fn report(&self) -> NodeAiReport {
        self.report.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn set(&self, phase: &str, detail: Option<String>) {
        if let Ok(mut r) = self.report.lock() {
            if r.phase != phase || r.detail != detail {
                tracing::info!(phase, detail = detail.as_deref().unwrap_or(""), "AI helper");
            }
            r.phase = phase.to_owned();
            r.detail = detail;
        }
    }

    fn installed_model(&self) -> Option<String> {
        if !ai_listener::installed(&self.dir) {
            return None;
        }
        std::fs::read_to_string(self.dir.join(".model"))
            .ok()
            .map(|m| m.trim().to_owned())
            .filter(|m| {
                self.dir
                    .join("models")
                    .join(format!("faster-whisper-{m}"))
                    .join("model.bin")
                    .is_file()
            })
    }
}

/// Runs the helper until the process ends.
pub async fn run(node: Node, helper: Arc<Helper>) {
    loop {
        tick(&node, &helper).await;
        send_report(&node, &helper).await;
        tokio::time::sleep(TICK).await;
    }
}

async fn tick(node: &Node, helper: &Helper) {
    let Some(wanted) = helper.wanted() else {
        stop(helper).await;
        if helper.report().phase != "off" {
            helper.set("off", None);
        }
        return;
    };
    if helper.installed_model().as_deref() != Some(wanted.model.as_str()) {
        stop(helper).await;
        let recently = helper
            .failed_at
            .lock()
            .ok()
            .and_then(|f| *f)
            .is_some_and(|at| at.elapsed() < RETRY);
        if recently {
            return;
        }
        match install(node, helper, &wanted.model).await {
            Ok(()) => {
                if let Ok(mut f) = helper.failed_at.lock() {
                    *f = None;
                }
            }
            Err(e) => {
                helper.set("failed", Some(e));
                if let Ok(mut f) = helper.failed_at.lock() {
                    *f = Some(std::time::Instant::now());
                }
                return;
            }
        }
    }
    let busy = node.busy();
    let mut child = helper.child.lock().await;
    let running = child
        .as_mut()
        .is_some_and(|c| matches!(c.try_wait(), Ok(None)));
    if !running {
        *child = None;
        helper.paused.store(false, Ordering::Release);
        if busy {
            helper.set("paused", Some("playing or showing visuals".into()));
            return;
        }
        match start(node, helper, &wanted.model) {
            Ok(started) => {
                *child = Some(started);
                helper.set("working", None);
            }
            Err(e) => helper.set("failed", Some(format!("couldn't start the listener: {e}"))),
        }
        return;
    }
    let paused = helper.paused.load(Ordering::Acquire);
    if busy && !paused {
        if pause(child.as_mut(), true).await {
            helper.paused.store(true, Ordering::Release);
            helper.set("paused", Some("playing or showing visuals".into()));
        } else if let Some(mut c) = child.take() {
            // Can't pause it here: stop it, it starts again when free.
            let _ = c.kill().await;
            helper.set("paused", Some("playing or showing visuals".into()));
        }
    } else if !busy && paused && pause(child.as_mut(), false).await {
        helper.paused.store(false, Ordering::Release);
        helper.set("working", None);
    }
}

/// Stops (true) or continues (false) the worker in place.
async fn pause(child: Option<&mut Child>, stop: bool) -> bool {
    #[cfg(unix)]
    {
        let Some(pid) = child.and_then(|c| c.id()) else {
            return false;
        };
        Command::new("kill")
            .arg(if stop { "-STOP" } else { "-CONT" })
            .arg(pid.to_string())
            .status()
            .await
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        let _ = (child, stop);
        false
    }
}

async fn stop(helper: &Helper) {
    let mut child = helper.child.lock().await;
    if let Some(mut c) = child.take() {
        // A stopped process must go on before it can end.
        if helper.paused.swap(false, Ordering::AcqRel) {
            pause(Some(&mut c), false).await;
        }
        let _ = c.kill().await;
    }
}

fn start(node: &Node, helper: &Helper, model: &str) -> std::io::Result<Child> {
    let port = node.port();
    let mut command = Command::new(ai_listener::venv_python(&helper.dir));
    command
        .current_dir(&helper.dir)
        .args(["worker.py", "--server"])
        .arg(format!("http://127.0.0.1:{port}"))
        .args(["--model", model, "--vocals", "off", "--device", "cpu"])
        .env("DJINN_WORKER_KEY", &helper.key)
        .env("PYTHONUNBUFFERED", "1")
        .env("HF_HUB_OFFLINE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    for out in [
        child
            .stdout
            .take()
            .map(|o| Box::new(o) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child
            .stderr
            .take()
            .map(|e| Box::new(e) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::info!(target: "djinn::lyrics_listener", "{line}");
            }
        });
    }
    Ok(child)
}

/// The node's Python version ("3.11") and processor ("x86_64"; Windows and
/// Python call the same processors "AMD64" and "ARM64").
async fn python_kind(py: &Path) -> Result<(String, String), String> {
    let out = Command::new(py)
        .args([
            "-c",
            "import sys, platform; print('%d.%d %s' % (sys.version_info[0], sys.version_info[1], platform.machine()))",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(v), Some(m)) => Ok((
            v.to_owned(),
            match m.to_ascii_lowercase().as_str() {
                "amd64" | "x86_64" => "x86_64".to_owned(),
                "arm64" | "aarch64" => "aarch64".to_owned(),
                other => other.to_owned(),
            },
        )),
        _ => Err("couldn't tell which Python this is".into()),
    }
}

async fn install(node: &Node, helper: &Helper, model: &str) -> Result<(), String> {
    let d = &helper.dir;
    helper.set("installing", Some("writing the listener".into()));
    ai_listener::write_pack(d).await?;
    ai_listener::make_venv(
        d,
        |line| helper.set("installing", Some(line.to_owned())),
        false,
    )
    .await?;
    let py = ai_listener::venv_python(d);
    let (python, arch) = python_kind(&py).await?;
    if let Ok(mut r) = helper.report.lock() {
        r.python = Some(python.clone());
    }
    // Ask Main for the kit until it's put together.
    let started = std::time::Instant::now();
    let files: Vec<KitFile> = loop {
        helper.set(
            "installing",
            Some("Main is getting the listener's packages ready".into()),
        );
        let (main, client, auth) = node
            .main_client(Duration::from_secs(30))
            .await
            .ok_or("this node isn't joined to Main")?;
        let answer = client
            .get(format!("{main}/api/nodes/ai/kit"))
            .header(header::AUTHORIZATION, &auth)
            .query(&[
                ("os", std::env::consts::OS),
                ("python", python.as_str()),
                ("arch", arch.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("can't reach Main: {e}"))?;
        let code = answer.status();
        let body: serde_json::Value = answer.json().await.unwrap_or_default();
        if body["model"].as_str().is_some_and(|m| m != model) {
            return Err("Main changed the model; trying again".into());
        }
        match body["kit"]["state"].as_str() {
            Some("ready") => {
                break serde_json::from_value(body["kit"]["files"].clone())
                    .map_err(|e| e.to_string())?
            }
            Some("failed") => {
                return Err(body["kit"]["error"]
                    .as_str()
                    .unwrap_or("Main couldn't get the listener ready")
                    .to_owned())
            }
            _ if code.is_success() => {}
            _ => return Err(format!("Main said {code}")),
        }
        if started.elapsed() > Duration::from_secs(60 * 60) {
            return Err("Main took too long to get the listener ready".into());
        }
        tokio::time::sleep(KIT_WAIT).await;
    };
    let total: u64 = files.iter().map(|f| f.size).sum();
    let mut got: u64 = 0;
    for file in &files {
        if !crate::ai_kit::safe_path(&file.path) {
            return Err("Main listed a file it shouldn't".into());
        }
        let target = match file.path.strip_prefix("wheels/") {
            Some(wheel) => d.join("kit").join(wheel),
            None => d.join(&file.path),
        };
        helper.set(
            "installing",
            Some(format!(
                "downloading from Main: {} of {} MB",
                got / 1_000_000,
                total / 1_000_000
            )),
        );
        if !already_there(&target, file).await {
            fetch(node, &python, &arch, file, &target).await?;
        }
        got += file.size;
    }
    // An environment made without pip (no python3-venv) gets pip from the
    // kit: a pip wheel runs as it is.
    if !ai_listener::has_pip(d).await {
        helper.set("installing", Some("putting pip in place".into()));
        let wheel = std::fs::read_dir(d.join("kit"))
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with("pip-") && n.ends_with(".whl")
                })
            })
            .ok_or("Main's kit has no pip; update Main and try again")?;
        let ok = Command::new(&py)
            .arg(wheel.join("pip"))
            .args([
                "install",
                "--disable-pip-version-check",
                "--quiet",
                "--no-index",
                "--find-links",
            ])
            .arg(d.join("kit"))
            .arg("pip")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success());
        if !ok {
            return Err("couldn't put pip in place".into());
        }
    }
    helper.set(
        "installing",
        Some("installing the listener's packages".into()),
    );
    let ok = Command::new(&py)
        .current_dir(d)
        .args([
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--quiet",
            "--no-index",
            "--find-links",
        ])
        .arg(d.join("kit"))
        .args(["-r", "requirements.txt"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success());
    if !ok {
        return Err("the listener's packages didn't install".into());
    }
    // Installed: the packages' files aren't needed again.
    let _ = tokio::fs::remove_dir_all(d.join("kit")).await;
    tokio::fs::write(d.join(".model"), model)
        .await
        .map_err(|e| e.to_string())?;
    ai_listener::mark_installed(d).await?;
    helper.set("installing", Some("testing this node's speed".into()));
    let bench = bench(&py, d, model).await;
    if let Ok(mut r) = helper.report.lock() {
        r.model = Some(model.to_owned());
        r.bench_ms = bench;
    }
    helper.set("ready", None);
    Ok(())
}

/// The speed test: how long the model takes to hear 30 seconds.
async fn bench(py: &Path, d: &Path, model: &str) -> Option<u64> {
    let out = Command::new(py)
        .current_dir(d)
        .args(["worker.py", "--bench", "--model", model, "--device", "cpu"])
        .env("HF_HUB_OFFLINE", "1")
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .and_then(|v| v["encode_ms"].as_u64())
}

async fn already_there(target: &Path, file: &KitFile) -> bool {
    if tokio::fs::metadata(target).await.map(|m| m.len()).ok() != Some(file.size) {
        return false;
    }
    let path = target.to_owned();
    tokio::task::spawn_blocking(move || crate::ai_kit::sha256_file(&path))
        .await
        .ok()
        .and_then(Result::ok)
        .is_some_and(|sum| sum == file.sha256)
}

/// Downloads one kit file from Main, checking its size and sum.
async fn fetch(
    node: &Node,
    python: &str,
    arch: &str,
    file: &KitFile,
    target: &Path,
) -> Result<(), String> {
    let (main, client, auth) = node
        .main_client(Duration::from_secs(30 * 60))
        .await
        .ok_or("this node isn't joined to Main")?;
    let mut response = client
        .get(format!("{main}/api/nodes/ai/kit"))
        .header(header::AUTHORIZATION, &auth)
        .query(&[
            ("os", std::env::consts::OS),
            ("python", python),
            ("arch", arch),
            ("path", file.path.as_str()),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("download from Main failed: {e}"))?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let part = target.with_extension("part");
    let mut out = tokio::fs::File::create(&part)
        .await
        .map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("download from Main failed: {e}"))?
    {
        size += chunk.len() as u64;
        if size > file.size {
            break;
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    out.flush().await.map_err(|e| e.to_string())?;
    drop(out);
    if size != file.size || crate::security::hex(&hasher.finalize()) != file.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!("{} didn't arrive whole from Main", file.path));
    }
    tokio::fs::rename(&part, target)
        .await
        .map_err(|e| e.to_string())
}

/// Tells Main how the helper stands, when that changed.
async fn send_report(node: &Node, helper: &Helper) {
    let report = helper.report();
    if helper.sent.lock().ok().and_then(|s| s.clone()).as_ref() == Some(&report) {
        return;
    }
    let Some((main, client, auth)) = node.main_client(Duration::from_secs(15)).await else {
        return;
    };
    let sent = client
        .put(format!("{main}/api/nodes/ai/report"))
        .header(header::AUTHORIZATION, &auth)
        .json(&report)
        .send()
        .await
        .is_ok_and(|r| r.status().is_success());
    if sent {
        if let Ok(mut s) = helper.sent.lock() {
            *s = Some(report);
        }
    }
}

// --- The worker's routes on the node, passed on to Main ---

fn worker_ok(helper: &Helper, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Worker "))
        .is_some_and(|k| crate::security::same_secret(k.trim().as_bytes(), helper.key.as_bytes()))
}

fn refused() -> Response {
    StatusCode::UNAUTHORIZED.into_response()
}

/// Passes Main's answer on as it is.
async fn pass_on(sent: Result<reqwest::Response, reqwest::Error>) -> Response {
    match sent {
        Ok(response) => {
            let code =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let kind = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut out = Response::builder().status(code);
            if let Some(kind) = kind {
                out = out.header(header::CONTENT_TYPE, kind);
            }
            out.body(Body::from_stream(response.bytes_stream()))
                .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
        }
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

pub async fn next(
    State(node): State<Node>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let helper = node.ai();
    if !worker_ok(&helper, &headers) {
        return refused();
    }
    // Playing or drawing visuals: nothing now, ask again soon.
    if node.busy() {
        return axum::Json(serde_json::json!({
            "jobs": [], "wait_secs": 20, "held": { "reason": "music" }
        }))
        .into_response();
    }
    let Some((main, client, auth)) = node.main_client(Duration::from_secs(30)).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    pass_on(
        client
            .post(format!("{main}/api/nodes/ai/next"))
            .header(header::AUTHORIZATION, auth)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await,
    )
    .await
}

pub async fn song(
    State(node): State<Node>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<i64>,
) -> Response {
    if !worker_ok(&node.ai(), &headers) {
        return refused();
    }
    let Some((main, client, auth)) = node.main_client(Duration::from_secs(10 * 60)).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    pass_on(
        client
            .get(format!("{main}/api/nodes/ai/song/{id}"))
            .header(header::AUTHORIZATION, auth)
            .send()
            .await,
    )
    .await
}

pub async fn result(
    State(node): State<Node>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<i64>,
    body: axum::body::Bytes,
) -> Response {
    if !worker_ok(&node.ai(), &headers) {
        return refused();
    }
    let Some((main, client, auth)) = node.main_client(Duration::from_secs(60)).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    pass_on(
        client
            .post(format!("{main}/api/nodes/ai/result/{id}"))
            .header(header::AUTHORIZATION, auth)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await,
    )
    .await
}
