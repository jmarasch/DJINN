//! Node mode: this box plays a room for a main DJINN instead of being one.
//!
//! A node keeps no library and serves no party app; phones go to Main. It
//! has its own small page (`/node`) to enter Main's address and to see how
//! pairing stands. It connects to Main over Main's encrypted connection,
//! pins Main's certificate the first time (both screens show Main's security
//! code, so a wrong server shows a wrong code), and asks to join with a
//! secret only it knows. It then checks in now and then: Main answers
//! waiting, accepted (with the room to play), or "unknown node", which after
//! being accepted means kicked: the node wipes its cache at once. Once
//! accepted, the node's page only shows status, so nobody on the network can
//! repoint it; leaving goes through Main's Kick.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_store::Store;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::error::{ApiError, ApiResult};

/// Setting key: "node" makes this box a node at the next start.
pub const ROLE: &str = "djinn_role";
const NODE_ID: &str = "node_id";
const NODE_SECRET: &str = "node_secret";
const NODE_MAIN: &str = "node_main";
const NODE_MAIN_FP: &str = "node_main_fingerprint";
const NODE_NAME: &str = "node_name";
const NODE_STATE: &str = "node_state";
/// Main's port when an address doesn't say.
const MAIN_PORT: u16 = 8066;
const CHECK_WAITING: Duration = Duration::from_secs(5);
/// Joined nodes check in often, so a setting changed on Main arrives soon.
const CHECK_JOINED: Duration = Duration::from_secs(5);

/// True if this box is set up as a node.
pub async fn is_node(store: &Store) -> anyhow::Result<bool> {
    Ok(store.get_setting(ROLE).await?.as_deref() == Some("node"))
}

/// Where a pairing stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pairing {
    /// No Main yet.
    Setup,
    /// Asking Main, or waiting for its admin.
    Waiting,
    Joined,
    /// Main's admin said no.
    Declined,
    /// Main's admin kicked this node; its cache is wiped.
    Kicked,
    /// Main answered with a different certificate than before.
    Changed,
}

impl Pairing {
    fn as_str(self) -> &'static str {
        match self {
            Pairing::Setup => "setup",
            Pairing::Waiting => "waiting",
            Pairing::Joined => "joined",
            Pairing::Declined => "declined",
            Pairing::Kicked => "kicked",
            Pairing::Changed => "changed",
        }
    }

    fn parse(text: &str) -> Pairing {
        match text {
            "waiting" => Pairing::Waiting,
            "joined" => Pairing::Joined,
            "declined" => Pairing::Declined,
            "kicked" => Pairing::Kicked,
            "changed" => Pairing::Changed,
            _ => Pairing::Setup,
        }
    }
}

/// What the node's page shows.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub pairing: Pairing,
    /// This node's name, as Main lists it.
    pub name: String,
    /// Main's address, like "https://192.168.1.10:8066".
    pub main: Option<String>,
    pub main_name: Option<String>,
    /// Main's security code (from its certificate), to compare with Main's
    /// Admin > Nodes.
    pub main_code: Option<String>,
    /// This request's code, which Main's admin sees beside it.
    pub code: Option<String>,
    pub room_id: Option<i64>,
    /// Why the last try didn't work, in words.
    pub problem: Option<String>,
    /// Playing Main's music right now.
    pub playing: bool,
    /// How closely this node's clock is known to match Main's, in ms (half
    /// the quickest round trip lately); None until matched.
    pub sync_ms: Option<f64>,
    /// Why the music isn't playing, in words.
    pub audio_problem: Option<String>,
    /// How far this node's speaker runs from Main's, in ms, as measured
    /// (late is positive).
    pub speaker_ms: Option<f64>,
    /// Speaker settings, as Main keeps them.
    pub audio: Option<crate::nodes::AudioSettings>,
    /// Main can't be reached; playing cached songs (or quiet, if there are
    /// none or the setting says stop).
    pub offline: bool,
    /// Songs in this node's cache.
    pub cached: usize,
    /// The room whose music Main sends it now.
    pub audio_room: Option<i64>,
    /// The page a plugged-in screen shows once joined (Main's TV page for
    /// this node's room).
    pub screen: Option<String>,
    /// Main DJINNs heard on the network letting nodes join, while this node
    /// hasn't got one. Other nodes don't announce this.
    pub mains: Vec<FoundMain>,
}

/// A main DJINN heard on the network.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FoundMain {
    pub name: String,
    /// What to enter as Main's address, like "192.168.1.10:8066".
    pub address: String,
    /// Its "Allow nodes to ask to join" is on.
    pub joining: bool,
}

/// The node's running parts.
#[derive(Clone)]
pub struct Node {
    store: Store,
    cache_dir: Arc<PathBuf>,
    status: Arc<RwLock<Status>>,
    wake: Arc<Notify>,
    /// Main's clock minus this one (µs), and whether that's known yet.
    offset: Arc<AtomicI64>,
    clock_ready: Arc<AtomicBool>,
    /// Speaker settings from Main, for the player to follow.
    audio: Arc<tokio::sync::watch::Sender<Option<crate::nodes::AudioSettings>>>,
    /// What's coming up in Main's room, to cache.
    upcoming: Arc<Mutex<Vec<crate::nodes::Upcoming>>>,
    cache: Arc<crate::node_offline::Cache>,
    /// Main's last volume (f32 bits), for playing offline at the same level.
    volume: Arc<AtomicU32>,
    /// Browsers let in to manage this node (by a link from Main's Admin >
    /// Nodes > Manage), and until when.
    managers: Arc<Mutex<std::collections::HashMap<String, std::time::Instant>>>,
}

/// The cookie that says a browser may manage this node.
const MANAGE_COOKIE: &str = "djinn_node_manage";
/// How long a browser stays let in after opening a manage link.
const MANAGE_FOR: Duration = Duration::from_secs(12 * 60 * 60);

impl Node {
    pub async fn new(store: Store, cache_dir: PathBuf) -> anyhow::Result<Node> {
        let name = match store.get_setting(NODE_NAME).await? {
            Some(name) if !name.is_empty() => name,
            _ => crate::go_announce::computer_name(),
        };
        let pairing = Pairing::parse(&store.get_setting(NODE_STATE).await?.unwrap_or_default());
        let main = store
            .get_setting(NODE_MAIN)
            .await?
            .filter(|m| !m.is_empty());
        let main_code = store
            .get_setting(NODE_MAIN_FP)
            .await?
            .filter(|f| !f.is_empty())
            .map(|f| crate::tls::security_code(&f));
        Ok(Node {
            store,
            cache_dir: Arc::new(cache_dir.clone()),
            status: Arc::new(RwLock::new(Status {
                pairing,
                name,
                main,
                main_name: None,
                main_code,
                code: None,
                room_id: None,
                problem: None,
                playing: false,
                sync_ms: None,
                audio_problem: None,
                speaker_ms: None,
                audio: None,
                offline: false,
                cached: 0,
                audio_room: None,
                screen: None,
                mains: Vec::new(),
            })),
            wake: Arc::new(Notify::new()),
            offset: Arc::new(AtomicI64::new(0)),
            clock_ready: Arc::new(AtomicBool::new(false)),
            audio: Arc::new(tokio::sync::watch::Sender::new(None)),
            upcoming: Arc::default(),
            cache: Arc::new(crate::node_offline::Cache::new(
                cache_dir.clone(),
                crate::node_offline::CACHE_LIMIT_BYTES,
            )),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            managers: Arc::default(),
        })
    }

    pub fn status(&self) -> Status {
        self.status
            .read()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    fn update(&self, change: impl FnOnce(&mut Status)) {
        match self.status.write() {
            Ok(mut s) => change(&mut s),
            Err(e) => change(&mut e.into_inner()),
        }
    }

    async fn set_pairing(&self, pairing: Pairing) -> anyhow::Result<()> {
        self.store.set_setting(NODE_STATE, pairing.as_str()).await?;
        self.update(|s| s.pairing = pairing);
        Ok(())
    }

    /// This node's id and secret, made the first time.
    async fn identity(&self) -> anyhow::Result<(String, String)> {
        let id = self
            .store
            .get_setting(NODE_ID)
            .await?
            .filter(|v| !v.is_empty());
        let secret = self
            .store
            .get_setting(NODE_SECRET)
            .await?
            .filter(|v| !v.is_empty());
        if let (Some(id), Some(secret)) = (id, secret) {
            return Ok((id, secret));
        }
        let id = format!("node-{}", random_hex(8)?);
        let secret = random_hex(32)?;
        self.store.set_setting(NODE_ID, &id).await?;
        self.store.set_setting(NODE_SECRET, &secret).await?;
        Ok((id, secret))
    }

    /// Points this node at a Main and asks to join. Not once joined: that
    /// goes through Main's Kick.
    pub async fn connect(&self, address: &str, name: &str) -> Result<(), String> {
        if self.status().pairing == Pairing::Joined {
            return Err("this node has joined; kick it on Main to change that".into());
        }
        let main = main_url(address).ok_or("that doesn't look like an address")?;
        let name = name.trim();
        let name = if name.is_empty() {
            crate::go_announce::computer_name()
        } else {
            name.chars().take(60).collect()
        };
        let save = async {
            self.store.set_setting(NODE_MAIN, &main).await?;
            self.store.set_setting(NODE_NAME, &name).await?;
            self.store.set_setting(NODE_MAIN_FP, "").await?;
            self.set_pairing(Pairing::Waiting).await
        };
        save.await.map_err(|e| e.to_string())?;
        self.update(|s| {
            s.main = Some(main);
            s.name = name;
            s.main_code = None;
            s.main_name = None;
            s.code = None;
            s.problem = None;
        });
        self.wake.notify_one();
        Ok(())
    }

    /// After being declined or kicked, or if Main's certificate changed:
    /// a fresh id and secret, Main's certificate learned again, and a new
    /// request.
    pub async fn ask_again(&self) -> Result<(), String> {
        let status = self.status();
        if matches!(status.pairing, Pairing::Joined | Pairing::Setup) {
            return Err("there's nothing to ask again".into());
        }
        let reset = async {
            self.store.set_setting(NODE_ID, "").await?;
            self.store.set_setting(NODE_SECRET, "").await?;
            self.store.set_setting(NODE_MAIN_FP, "").await?;
            self.set_pairing(Pairing::Waiting).await
        };
        reset.await.map_err(|e| e.to_string())?;
        self.update(|s| {
            s.main_code = None;
            s.code = None;
            s.problem = None;
        });
        self.wake.notify_one();
        Ok(())
    }

    /// Deletes every cached song.
    pub fn wipe_cache(&self) {
        wipe(&self.cache_dir);
    }

    /// Keeps the pairing going: asks, waits, checks in, and wipes the cache
    /// if kicked. Runs until the process ends.
    pub async fn run(self) {
        loop {
            let wait = match self.check().await {
                Ok(wait) => wait,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "node check failed");
                    Some(CHECK_WAITING)
                }
            };
            match wait {
                Some(wait) => {
                    let _ = tokio::time::timeout(wait, self.wake.notified()).await;
                }
                None => self.wake.notified().await,
            }
        }
    }

    /// One look at Main. Returns how long until the next, or None to wait
    /// until someone acts on the node's page.
    async fn check(&self) -> anyhow::Result<Option<Duration>> {
        let status = self.status();
        let Some(main) = status.main.clone() else {
            return Ok(None);
        };
        if !matches!(status.pairing, Pairing::Waiting | Pairing::Joined) {
            return Ok(None);
        }
        let (id, secret) = self.identity().await?;
        let pinned = self
            .store
            .get_setting(NODE_MAIN_FP)
            .await?
            .filter(|f| !f.is_empty());
        let seen = Arc::new(Mutex::new(None));
        let client = pinned_client(pinned.clone(), Arc::clone(&seen))?;
        let auth = format!("Node {id}:{secret}");

        // Already asked (and Main's certificate known): check in.
        let checked = if pinned.is_some() {
            Some(
                client
                    .get(format!("{main}/api/nodes/me"))
                    .header("authorization", &auth)
                    .send()
                    .await,
            )
        } else {
            None
        };
        let response = match checked {
            Some(sent) => sent,
            None => {
                let body = serde_json::json!({
                    "node_id": id,
                    "name": status.name,
                    "secret": secret,
                    "version": env!("DJINN_RELEASE_VERSION"),
                    "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                });
                client
                    .post(format!("{main}/api/nodes/join"))
                    .json(&body)
                    .send()
                    .await
            }
        };
        let response = match response {
            Ok(response) => response,
            Err(e) => {
                let changed = matches!((&pinned, seen.lock().ok().and_then(|s| s.clone())), (Some(p), Some(s)) if *p != s);
                if changed {
                    self.set_pairing(Pairing::Changed).await?;
                    self.update(|s| {
                        s.problem = Some(
                            "Main answered with a different identity than before. If you reinstalled Main, \
                             ask to join again; otherwise something else may be answering at that address."
                                .into(),
                        )
                    });
                    return Ok(None);
                }
                self.update(|s| s.problem = Some(format!("Can't reach Main: {}", plain_error(&e))));
                return Ok(Some(CHECK_WAITING));
            }
        };
        // The certificate this connection saw; pinned from the first answer.
        if pinned.is_none() {
            if let Some(fingerprint) = seen.lock().ok().and_then(|s| s.clone()) {
                self.store.set_setting(NODE_MAIN_FP, &fingerprint).await?;
                self.update(|s| s.main_code = Some(crate::tls::security_code(&fingerprint)));
            }
        }
        let code = crate::nodes::pairing_code(&id, &crate::nodes::hash_secret(&secret));
        let http = response.status();
        if http == reqwest::StatusCode::UNAUTHORIZED {
            // Main doesn't know this node: declined while waiting, or kicked.
            if status.pairing == Pairing::Joined {
                self.wipe_cache();
                self.set_pairing(Pairing::Kicked).await?;
                self.update(|s| s.problem = None);
                tracing::warn!("kicked by Main; cache wiped");
            } else {
                self.set_pairing(Pairing::Declined).await?;
                self.update(|s| s.problem = None);
            }
            return Ok(None);
        }
        let answer: serde_json::Value = response.json().await.unwrap_or_default();
        if !http.is_success() {
            let message = answer["error"]
                .as_str()
                .unwrap_or("Main said no")
                .to_owned();
            self.update(|s| {
                s.code = Some(code.clone());
                s.problem = Some(message);
            });
            // Asking again later (joining may be turned on meanwhile) is
            // only a fresh join if nothing was pinned yet.
            if http != reqwest::StatusCode::UPGRADE_REQUIRED {
                self.store.set_setting(NODE_MAIN_FP, "").await?;
            }
            return Ok(Some(CHECK_WAITING));
        }
        let joined = answer["status"].as_str() == Some("approved");
        if joined && status.pairing != Pairing::Joined {
            self.set_pairing(Pairing::Joined).await?;
            tracing::info!(main = %main, "joined Main");
        }
        self.update(|s| {
            s.code = Some(code);
            s.problem = None;
            if let Some(name) = answer["server_name"].as_str() {
                s.main_name = Some(name.to_owned());
            }
            if let Some(room) = answer["room_id"].as_i64() {
                s.room_id = Some(room);
            }
            s.audio_room = answer["audio_room"].as_i64();
            s.screen = answer["screen"].as_str().map(str::to_owned);
        });
        if let Ok(upcoming) =
            serde_json::from_value::<Vec<crate::nodes::Upcoming>>(answer["upcoming"].clone())
        {
            if let Ok(mut list) = self.upcoming.lock() {
                *list = upcoming;
            }
        }
        if let Ok(audio) =
            serde_json::from_value::<crate::nodes::AudioSettings>(answer["audio"].clone())
        {
            if self.status().audio.as_ref() != Some(&audio) {
                self.update(|s| s.audio = Some(audio.clone()));
                self.audio.send_replace(Some(audio));
            }
        }
        Ok(Some(if joined { CHECK_JOINED } else { CHECK_WAITING }))
    }
}

/// How long Main's music may be gone before playing from the cache (a
/// shorter blip just waits).
const OFFLINE_AFTER: Duration = Duration::from_secs(6);
/// Clock samples kept: the quickest round trip among them gives the offset.
const CLOCK_SAMPLES: usize = 10;
const CLOCK_EVERY: Duration = Duration::from_secs(1);
/// Samples before the clock counts as matched.
const CLOCK_READY: usize = 4;

impl Node {
    /// Main's address and its pinned certificate, once joined.
    async fn joined_main(&self) -> Option<(String, String)> {
        if self.status().pairing != Pairing::Joined {
            return None;
        }
        let main = self.status().main?;
        let pin = self
            .store
            .get_setting(NODE_MAIN_FP)
            .await
            .ok()
            .flatten()
            .filter(|f| !f.is_empty())?;
        Some((main, pin))
    }

    /// Keeps `offset` (Main's clock minus this one) current while joined:
    /// asks Main the time every second and believes the quickest answers,
    /// whose network delay is smallest and most even.
    async fn run_clock(self) {
        let mut samples: VecDeque<(i64, i64)> = VecDeque::new();
        loop {
            let Some((main, pin)) = self.joined_main().await else {
                samples.clear();
                self.clock_ready.store(false, Ordering::Release);
                self.update(|s| s.sync_ms = None);
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            };
            let client = match pinned_client(Some(pin), Arc::new(Mutex::new(None))) {
                Ok(client) => client,
                Err(_) => {
                    tokio::time::sleep(CLOCK_EVERY).await;
                    continue;
                }
            };
            // Quick ones while starting, then steady.
            let wait = if samples.len() < CLOCK_READY {
                Duration::from_millis(150)
            } else {
                CLOCK_EVERY
            };
            let t0 = djinn_engine::clock::now_micros();
            let answer = client.get(format!("{main}/api/nodes/clock")).send().await;
            let t1 = djinn_engine::clock::now_micros();
            let main_now = match answer {
                Ok(r) if r.status().is_success() => r
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|v| v["now_us"].as_i64()),
                _ => None,
            };
            if let Some(main_now) = main_now {
                let rtt = t1 - t0;
                samples.push_back((main_now - (t0 + t1) / 2, rtt));
                while samples.len() > CLOCK_SAMPLES {
                    samples.pop_front();
                }
                if let Some(&(offset, rtt)) = samples.iter().min_by_key(|s| s.1) {
                    self.offset.store(offset, Ordering::Release);
                    if samples.len() >= CLOCK_READY {
                        self.clock_ready.store(true, Ordering::Release);
                    }
                    self.update(|s| s.sync_ms = Some(rtt as f64 / 2000.0));
                }
            }
            tokio::time::sleep(wait).await;
        }
    }

    /// Plays Main's music while joined, reconnecting when the feed drops.
    async fn run_audio(self) {
        let mut player: Option<(djinn_engine::NodePlayer, u32, Option<String>)> = None;
        let mut offline: Option<crate::node_offline::OfflinePlayer> = None;
        let mut lost_since: Option<std::time::Instant> = None;
        loop {
            let Some((main, pin)) = self.joined_main().await else {
                player = None;
                offline = None;
                lost_since = None;
                self.update(|s| {
                    s.playing = false;
                    s.offline = false;
                });
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            };
            if offline.is_some() || self.status().offline {
                // Back to Main's music as soon as it answers.
                if !self.main_answers(&main, &pin).await {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
                offline = None;
                lost_since = None;
                tracing::info!("Main is back; following it again");
                self.update(|s| s.offline = false);
            }
            if !self.clock_ready.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            match self.listen(&main, &pin, &mut player).await {
                Ok(()) => lost_since = None,
                Err(e) => {
                    tracing::debug!(error = %format!("{e:#}"), "node audio stopped");
                    lost_since.get_or_insert_with(std::time::Instant::now);
                    self.update(|s| {
                        s.playing = false;
                        s.audio_problem = Some(format!("{e:#}"));
                    });
                }
            }
            // Gone a while (not just a blip): play from the cache.
            if lost_since.is_some_and(|since| since.elapsed() >= OFFLINE_AFTER) {
                player = None;
                offline = self.go_offline();
                self.update(|s| s.offline = true);
                tracing::warn!(playing = offline.is_some(), "Main can't be reached");
                continue;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// True if Main answers right now.
    async fn main_answers(&self, main: &str, pin: &str) -> bool {
        let Ok(client) = pinned_client_with(
            Some(pin.to_owned()),
            Arc::new(Mutex::new(None)),
            Some(Duration::from_secs(3)),
        ) else {
            return false;
        };
        client
            .get(format!("{main}/api/nodes/clock"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }

    /// Starts playing what was coming up from the cache, then stops or
    /// keeps going as Main's setting for this node says. None if there's
    /// nothing to play.
    fn go_offline(&self) -> Option<crate::node_offline::OfflinePlayer> {
        let upcoming = self.upcoming.lock().map(|u| u.clone()).unwrap_or_default();
        let first = crate::node_offline::coming_up(&self.cache, &upcoming);
        let audio = self.status().audio;
        let keep_going = audio.as_ref().is_some_and(|a| a.offline == "cached");
        if first.is_empty() && !keep_going {
            return None;
        }
        let volume = f32::from_bits(self.volume.load(Ordering::Acquire));
        match crate::node_offline::OfflinePlayer::start(
            48_000,
            audio.as_ref(),
            volume,
            first,
            keep_going,
            self.cache.files(),
        ) {
            Ok(player) => Some(player),
            Err(e) => {
                self.update(|s| s.audio_problem = Some(format!("can't play cached songs: {e:#}")));
                None
            }
        }
    }

    /// Keeps copies of what's coming up in Main's room, while joined.
    async fn run_cache(self) {
        loop {
            if let Some((main, pin)) = self.joined_main().await {
                let upcoming = self.upcoming.lock().map(|u| u.clone()).unwrap_or_default();
                for song in &upcoming {
                    if !self.cache.has(song) {
                        if let Err(e) = self.fetch(&main, &pin, song).await {
                            tracing::debug!(error = %format!("{e:#}"), song = song.id, "couldn't cache a song");
                            break;
                        }
                    }
                    self.cache.touch(song);
                }
                let keep: Vec<PathBuf> = upcoming.iter().map(|s| self.cache.path(s)).collect();
                let cache = Arc::clone(&self.cache);
                let count = tokio::task::spawn_blocking(move || {
                    cache.trim(&keep);
                    cache.files().len()
                })
                .await
                .unwrap_or(0);
                self.update(|s| s.cached = count);
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    /// Downloads one song into the cache.
    async fn fetch(
        &self,
        main: &str,
        pin: &str,
        song: &crate::nodes::Upcoming,
    ) -> anyhow::Result<()> {
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let (id, secret) = self.identity().await?;
        let client = pinned_client_with(Some(pin.to_owned()), Arc::new(Mutex::new(None)), None)?;
        let response = client
            .get(format!("{main}/api/nodes/song/{}", song.id))
            .header("authorization", format!("Node {id}:{secret}"))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Main answered {}", response.status());
        }
        let partial = self.cache.partial(song);
        let mut file = tokio::fs::File::create(&partial).await?;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            file.write_all(&chunk?).await?;
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&partial, self.cache.path(song)).await?;
        Ok(())
    }

    /// One connection to Main's feed, until it ends.
    async fn listen(
        &self,
        main: &str,
        pin: &str,
        player: &mut Option<(djinn_engine::NodePlayer, u32, Option<String>)>,
    ) -> anyhow::Result<()> {
        use futures_util::StreamExt;
        let (id, secret) = self.identity().await?;
        let client = pinned_client_with(Some(pin.to_owned()), Arc::new(Mutex::new(None)), None)?;
        let response = client
            .get(format!("{main}/api/nodes/stream"))
            .header("authorization", format!("Node {id}:{secret}"))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("can't reach Main: {}", plain_error(&e)))?;
        if !response.status().is_success() {
            anyhow::bail!("Main refused the music ({})", response.status());
        }
        let rate: u32 = response
            .headers()
            .get("x-djinn-rate")
            .and_then(|v| v.to_str().ok()?.parse().ok())
            .unwrap_or(48_000);
        let mut settings_rx = self.audio.subscribe();
        let audio = settings_rx.borrow_and_update().clone();
        let device = audio.as_ref().and_then(|a| a.device.clone());
        if player
            .as_ref()
            .is_none_or(|(_, r, d)| *r != rate || *d != device)
        {
            *player = None;
            let offset = Arc::clone(&self.offset);
            let settings = node_settings(audio.as_ref());
            let wanted = device.clone();
            let started = tokio::task::spawn_blocking(move || {
                djinn_engine::NodePlayer::start(wanted, rate, settings, offset)
            })
            .await?;
            self.report_speakers(main, pin).await;
            match started {
                Ok(p) => *player = Some((p, rate, device)),
                Err(e) => anyhow::bail!("can't open this computer's speakers: {e}"),
            }
        }
        self.update(|s| {
            s.playing = true;
            s.audio_problem = None;
        });
        let room = (self.status().room_id, self.status().audio_room);
        let mut buffer = Vec::new();
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk
                .map_err(|e| anyhow::anyhow!("the music from Main stopped: {}", plain_error(&e)))?;
            buffer.extend_from_slice(&chunk);
            let pieces = crate::node_audio::decode(&mut buffer)
                .ok_or_else(|| anyhow::anyhow!("Main sent something odd"))?;
            if settings_rx.has_changed().unwrap_or(false) {
                let audio = settings_rx.borrow_and_update().clone();
                let device = audio.as_ref().and_then(|a| a.device.clone());
                if player.as_ref().is_some_and(|(_, _, d)| *d != device) {
                    // Another speaker: open it on the next connection.
                    *player = None;
                    break;
                }
                if let Some((p, _, _)) = player.as_ref() {
                    p.set(node_settings(audio.as_ref()))?;
                }
            }
            if let Some((p, _, _)) = player.as_ref() {
                for piece in pieces {
                    if let djinn_engine::Mark::Volume(v) = piece.mark {
                        self.volume.store(v.to_bits(), Ordering::Release);
                    }
                    p.piece(piece)?;
                }
                let error = p.error_ms();
                self.update(|s| s.speaker_ms = Some(error));
            }
            if self.status().pairing != Pairing::Joined {
                *player = None;
                break;
            }
            // Moved to another room on Main: take that room's music.
            if (self.status().room_id, self.status().audio_room) != room {
                break;
            }
        }
        self.update(|s| s.playing = false);
        Ok(())
    }
}

impl Node {
    /// Tells Main which speakers this box has, so Admin can offer them.
    async fn report_speakers(&self, main: &str, pin: &str) {
        let Ok(devices) = tokio::task::spawn_blocking(djinn_engine::output_devices).await else {
            return;
        };
        let devices: Vec<serde_json::Value> = devices
            .unwrap_or_default()
            .into_iter()
            .map(|d| serde_json::json!({ "id": d.id, "name": d.name, "is_default": d.is_default }))
            .collect();
        let Ok((id, secret)) = self.identity().await else {
            return;
        };
        let Ok(client) = pinned_client(Some(pin.to_owned()), Arc::new(Mutex::new(None))) else {
            return;
        };
        let _ = client
            .post(format!("{main}/api/nodes/me/devices"))
            .header("authorization", format!("Node {id}:{secret}"))
            .json(&serde_json::json!({ "devices": devices }))
            .send()
            .await;
    }

    /// Sends settings changed on this node's page to Main, which keeps them.
    /// Asks Main whether a manage link is real and for this node; if so,
    /// lets the browser in and gives back its cookie value.
    async fn let_in(&self, token: &str) -> Result<String, String> {
        let (main, pin) = self
            .joined_main()
            .await
            .ok_or("this node hasn't joined a main DJINN")?;
        let (id, secret) = self.identity().await.map_err(|e| e.to_string())?;
        let client =
            pinned_client(Some(pin), Arc::new(Mutex::new(None))).map_err(|e| e.to_string())?;
        let response = client
            .post(format!("{main}/api/nodes/me/manage-check"))
            .header("authorization", format!("Node {id}:{secret}"))
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await
            .map_err(|e| format!("can't reach Main: {}", plain_error(&e)))?;
        if !response.status().is_success() {
            return Err("that link has expired or was already used: open Manage again".into());
        }
        let cookie = random_hex(16).map_err(|e| e.to_string())?;
        if let Ok(mut managers) = self.managers.lock() {
            let now = std::time::Instant::now();
            managers.retain(|_, until| *until > now);
            managers.insert(cookie.clone(), now + MANAGE_FOR);
        }
        Ok(cookie)
    }

    /// Whether this request comes from a browser let in to manage the node.
    fn managing(&self, headers: &axum::http::HeaderMap) -> bool {
        let Some(cookie) = cookie_value(headers, MANAGE_COOKIE) else {
            return false;
        };
        let now = std::time::Instant::now();
        self.managers
            .lock()
            .is_ok_and(|managers| managers.get(&cookie).is_some_and(|until| *until > now))
    }

    async fn save_audio(&self, audio: &crate::nodes::AudioSettings) -> Result<(), String> {
        let (main, pin) = self
            .joined_main()
            .await
            .ok_or("this node hasn't joined a main DJINN")?;
        let (id, secret) = self.identity().await.map_err(|e| e.to_string())?;
        let client =
            pinned_client(Some(pin), Arc::new(Mutex::new(None))).map_err(|e| e.to_string())?;
        let response = client
            .put(format!("{main}/api/nodes/me/audio"))
            .header("authorization", format!("Node {id}:{secret}"))
            .json(audio)
            .send()
            .await
            .map_err(|e| format!("can't reach Main: {}", plain_error(&e)))?;
        if !response.status().is_success() {
            let answer: serde_json::Value = response.json().await.unwrap_or_default();
            return Err(answer["error"]
                .as_str()
                .unwrap_or("Main refused")
                .to_owned());
        }
        self.update(|s| s.audio = Some(audio.clone()));
        self.audio.send_replace(Some(audio.clone()));
        Ok(())
    }
}

/// Listens for main DJINNs on the network whenever `wanted` says so, telling
/// `publish` the list as it changes. Runs on its own thread, for good.
fn browse_mains(wanted: impl Fn() -> bool, publish: impl Fn(Vec<FoundMain>)) {
    loop {
        if !wanted() {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        let browsing = mdns_sd::ServiceDaemon::new().and_then(|daemon| {
            let events = daemon.browse(crate::go_announce::MAIN_SERVICE)?;
            Ok((daemon, events))
        });
        let (daemon, events) = match browsing {
            Ok(browsing) => browsing,
            Err(e) => {
                tracing::debug!(error = %e, "can't listen for main DJINNs");
                std::thread::sleep(Duration::from_secs(30));
                continue;
            }
        };
        let mut found = std::collections::BTreeMap::new();
        while wanted() && !events.is_disconnected() {
            match events.recv_timeout(Duration::from_secs(1)) {
                Ok(mdns_sd::ServiceEvent::ServiceResolved(service)) => {
                    let addresses: Vec<std::net::Ipv4Addr> = service
                        .get_addresses_v4()
                        .into_iter()
                        .filter_map(|ip| ip.to_string().parse().ok())
                        .collect();
                    let Some(ip) = best_address(&addresses) else {
                        continue;
                    };
                    let name = service
                        .get_property_val_str("name")
                        .map_or_else(|| ip.to_string(), str::to_owned);
                    let address = format!("{ip}:{}", service.get_port());
                    let joining = service.get_property_val_str("join") == Some("1");
                    found.insert(
                        service.get_fullname().to_owned(),
                        FoundMain {
                            name,
                            address,
                            joining,
                        },
                    );
                }
                Ok(mdns_sd::ServiceEvent::ServiceRemoved(_, fullname)) => {
                    found.remove(&fullname);
                }
                _ => continue,
            }
            publish(found.values().cloned().collect());
        }
        let _ = daemon.shutdown();
        publish(Vec::new());
    }
}

impl Node {
    /// Listens for main DJINNs while this node has none.
    fn find_mains(self) {
        let looking = self.clone();
        browse_mains(
            move || !matches!(looking.status().pairing, Pairing::Joined | Pairing::Waiting),
            move |list| self.update(|s| s.mains = list),
        );
    }
}

/// While nobody has set up this DJINN, listens for main DJINNs, so the
/// first-start page can ask whether this box is meant as a node for one.
pub fn find_mains_during_setup(state: crate::state::AppState) {
    let handle = tokio::runtime::Handle::current();
    let store = state.store.clone();
    let spawned = std::thread::Builder::new()
        .name("djinn-find-mains".to_owned())
        .spawn(move || {
            browse_mains(
                move || !handle.block_on(store.admin_exists()).unwrap_or(true),
                move |list| {
                    if let Ok(mut found) = state.found_mains.lock() {
                        *found = list;
                    }
                },
            );
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "can't listen for main DJINNs");
    }
}

/// The address a server is most likely reachable at: a home network's
/// before a container's or VPN's.
fn best_address(addresses: &[std::net::Ipv4Addr]) -> Option<std::net::Ipv4Addr> {
    let rank = |ip: &std::net::Ipv4Addr| match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        [172, ..] => 2,
        _ => 3,
    };
    addresses
        .iter()
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local())
        .min_by_key(|ip| rank(ip))
        .copied()
}

/// The player's settings from Main's.
fn node_settings(audio: Option<&crate::nodes::AudioSettings>) -> djinn_engine::NodeSettings {
    match audio {
        Some(a) => djinn_engine::NodeSettings {
            delay_ms: a.delay_ms.clamp(0, 2000) as u32,
            trim: a.trim.clamp(0.0, 1.0) as f32,
            equalizer: a.equalizer.engine_config(),
            free_run: a.free_run,
        },
        None => djinn_engine::NodeSettings {
            trim: 1.0,
            ..djinn_engine::NodeSettings::default()
        },
    }
}

fn random_hex(bytes: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// The error's own words, without reqwest's long chain.
fn plain_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "it didn't answer in time".into()
    } else if e.is_connect() {
        "nothing answered at that address".into()
    } else {
        e.to_string()
    }
}

/// Main's base URL from what was typed: "192.168.1.10", "djinn.local:8066",
/// "http://..." all become "https://host:port" (nodes always encrypt).
pub fn main_url(typed: &str) -> Option<String> {
    let rest = typed.trim().trim_end_matches('/');
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))
        .unwrap_or(rest);
    let host_port = rest.split('/').next()?;
    if host_port.is_empty() || host_port.contains(char::is_whitespace) {
        return None;
    }
    let has_port = match host_port.rsplit_once(':') {
        // An IPv6 address in brackets has colons of its own.
        Some((host, port)) => {
            !host.ends_with(':')
                && port.parse::<u16>().is_ok()
                && (!host.contains(':') || host.ends_with(']'))
        }
        None => false,
    };
    Some(if has_port {
        format!("https://{host_port}")
    } else {
        format!("https://{host_port}:{MAIN_PORT}")
    })
}

fn wipe(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(e) = removed {
                tracing::error!(path = %path.display(), error = %e, "couldn't wipe a cached file");
            }
        }
    }
}

/// Accepts Main's certificate if it's the pinned one, or (the first time,
/// nothing pinned) whatever it is, noting what it saw. The handshake's
/// signatures are still checked, so whoever answers must hold the
/// certificate's key.
#[derive(Debug)]
struct Pin {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint: String = Sha256::digest(end_entity.as_ref())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(fingerprint.clone());
        }
        match &self.expected {
            Some(expected) if *expected != fingerprint => {
                Err(rustls::Error::General("Main's certificate changed".into()))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn pinned_client(
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
) -> anyhow::Result<reqwest::Client> {
    pinned_client_with(expected, seen, Some(Duration::from_secs(15)))
}

/// A pinned client; `timeout` None for the live audio, which never ends
/// (a stalled read still fails after a while).
fn pinned_client_with(
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    timeout: Option<Duration>,
) -> anyhow::Result<reqwest::Client> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pin {
            expected,
            seen,
            provider,
        }))
        .with_no_client_auth();
    let builder = reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("DJINN-node/", env!("CARGO_PKG_VERSION")));
    let builder = match timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder.read_timeout(Duration::from_secs(10)),
    };
    Ok(builder.build()?)
}

// --- The node's page ---

pub fn router(node: Node) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/node") }))
        // A plugged-in screen: the node's page (pairing codes) until it
        // joins, then Main's TV page for its room.
        .route("/screen", get(|| async { Redirect::to("/node?screen=1") }))
        .route("/api/node/status", get(status))
        .route("/api/node/connect", post(connect))
        .route("/api/node/ask-again", post(ask_again))
        .route("/api/node/audio", get(get_audio).put(put_audio))
        .route("/api/node/manage", get(manage_state).post(manage))
        .fallback(crate::web::static_handler)
        .with_state(node)
}

#[derive(Serialize)]
struct NodeAudioView {
    audio: Option<crate::nodes::AudioSettings>,
    speakers: Vec<serde_json::Value>,
}

/// This node's speaker settings and its speakers, for its page.
async fn get_audio(State(node): State<Node>) -> Json<NodeAudioView> {
    let speakers = tokio::task::spawn_blocking(djinn_engine::output_devices)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
        .into_iter()
        .map(|d| serde_json::json!({ "id": d.id, "name": d.name, "is_default": d.is_default }))
        .collect();
    Json(NodeAudioView {
        audio: node.status().audio,
        speakers,
    })
}

/// A cookie's value from a request.
fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
}

#[derive(Serialize)]
struct ManageState {
    managing: bool,
}

async fn manage_state(
    State(node): State<Node>,
    headers: axum::http::HeaderMap,
) -> Json<ManageState> {
    Json(ManageState {
        managing: node.managing(&headers),
    })
}

#[derive(Deserialize)]
struct ManageBody {
    token: String,
}

/// Opening a manage link from Main: checked with Main, then this browser
/// may manage the node for a while.
async fn manage(
    State(node): State<Node>,
    Json(body): Json<ManageBody>,
) -> ApiResult<axum::response::Response> {
    let cookie = node
        .let_in(&body.token)
        .await
        .map_err(|e| ApiError::new(StatusCode::FORBIDDEN, e))?;
    let set = format!(
        "{MANAGE_COOKIE}={cookie}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        MANAGE_FOR.as_secs()
    );
    Ok((
        StatusCode::NO_CONTENT,
        [(axum::http::header::SET_COOKIE, set)],
    )
        .into_response())
}

async fn put_audio(
    State(node): State<Node>,
    headers: axum::http::HeaderMap,
    Json(body): Json<crate::nodes::AudioSettings>,
) -> ApiResult<StatusCode> {
    if !node.managing(&headers) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "open this page from the main DJINN's Admin > Nodes > Manage to change settings",
        ));
    }
    node.save_audio(&body)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn status(State(node): State<Node>) -> Json<Status> {
    Json(node.status())
}

#[derive(Deserialize)]
struct Connect {
    address: String,
    #[serde(default)]
    name: String,
}

async fn connect(State(node): State<Node>, Json(body): Json<Connect>) -> ApiResult<Json<Status>> {
    node.connect(&body.address, &body.name)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(node.status()))
}

async fn ask_again(State(node): State<Node>) -> ApiResult<Json<Status>> {
    node.ask_again()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(node.status()))
}

/// Runs this box as a node until asked to stop.
pub async fn serve(
    store: Store,
    db_path: &Path,
    listen: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let cache_dir = db_path
        .parent()
        .map_or_else(|| PathBuf::from("node-cache"), |dir| dir.join("node-cache"));
    std::fs::create_dir_all(&cache_dir)?;
    let node = Node::new(store.clone(), cache_dir).await?;
    tokio::spawn(node.clone().run());
    tokio::spawn(node.clone().run_clock());
    tokio::spawn(node.clone().run_audio());
    tokio::spawn(node.clone().run_cache());
    let browsing = node.clone();
    std::thread::Builder::new()
        .name("djinn-find-mains".to_owned())
        .spawn(move || browsing.find_mains())?;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "running as a node; its page is at /node");
    axum::serve(listener, router(node))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    store.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn the_manage_cookie_is_found_among_others() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "a=1; djinn_node_manage=abc123; b=2".parse().unwrap(),
        );
        assert_eq!(
            cookie_value(&headers, MANAGE_COOKIE).as_deref(),
            Some("abc123")
        );
        assert_eq!(cookie_value(&headers, "missing"), None);
    }

    #[test]
    fn the_home_network_address_is_preferred() {
        let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap();
        assert_eq!(
            best_address(&[
                ip("172.17.0.1"),
                ip("127.0.0.1"),
                ip("192.168.0.81"),
                ip("10.0.0.2")
            ]),
            Some(ip("192.168.0.81"))
        );
        assert_eq!(best_address(&[ip("127.0.0.1")]), None);
    }

    #[test]
    fn addresses_become_encrypted_urls() {
        assert_eq!(
            main_url("192.168.1.10").as_deref(),
            Some("https://192.168.1.10:8066")
        );
        assert_eq!(
            main_url("http://djinn.local:9000/").as_deref(),
            Some("https://djinn.local:9000")
        );
        assert_eq!(
            main_url(" djinn.local ").as_deref(),
            Some("https://djinn.local:8066")
        );
        assert_eq!(
            main_url("https://host:8066/admin").as_deref(),
            Some("https://host:8066")
        );
        assert_eq!(
            main_url("[::1]:8066").as_deref(),
            Some("https://[::1]:8066")
        );
        assert_eq!(main_url("").as_deref(), None);
        assert_eq!(main_url("two words").as_deref(), None);
    }

    /// A real Main over its encrypted port and a real node pairing with it.
    #[tokio::test]
    async fn a_node_pairs_with_main_and_wipes_its_cache_when_kicked() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let main = AppState::for_tests().await;
        main.store
            .set_setting(crate::nodes::NODES_JOIN, "1")
            .await
            .unwrap();
        crate::go::apply_encryption(&main).await.unwrap();
        let main_fp = main.go.encryption.fingerprint().unwrap();
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = tcp.local_addr().unwrap().port();
        let listener = crate::tls::listen(tcp, Arc::clone(&main.go.encryption)).unwrap();
        let app = crate::app(main.clone());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<crate::tls::Peer>(),
            )
            .await
        });

        let dir = std::env::temp_dir().join(format!("djinn-node-test-{}", std::process::id()));
        let cache = dir.join("node-cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("song.flac"), b"cached").unwrap();
        let node = Node::new(Store::open_in_memory().await.unwrap(), cache.clone())
            .await
            .unwrap();
        assert_eq!(node.status().pairing, Pairing::Setup);
        node.connect(&format!("127.0.0.1:{port}"), "Kitchen Pi")
            .await
            .unwrap();

        // Asks: Main pinned, request waiting with matching codes.
        node.check().await.unwrap();
        let status = node.status();
        assert_eq!(status.pairing, Pairing::Waiting, "{:?}", status.problem);
        assert_eq!(status.main_code, Some(crate::tls::security_code(&main_fp)));
        let waiting = main.store.nodes().await.unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(
            status.code,
            Some(crate::nodes::pairing_code(
                &waiting[0].node_id,
                &waiting[0].secret_hash
            ))
        );

        // Accepted on Main: the node sees it.
        let admin = main
            .store
            .create_user("boss", "h", djinn_store::Role::Admin)
            .await
            .unwrap();
        main.store
            .approve_node(waiting[0].id, admin.id)
            .await
            .unwrap();
        node.check().await.unwrap();
        assert_eq!(node.status().pairing, Pairing::Joined);
        assert!(
            node.connect("elsewhere", "x").await.is_err(),
            "can't be repointed once joined"
        );
        assert!(cache.join("song.flac").exists());

        // Kicked: cache gone.
        main.store.remove_node(waiting[0].id).await.unwrap();
        node.check().await.unwrap();
        assert_eq!(node.status().pairing, Pairing::Kicked);
        assert!(!cache.join("song.flac").exists(), "cache wiped");

        // Asking again is a new request.
        node.ask_again().await.unwrap();
        node.check().await.unwrap();
        assert_eq!(node.status().pairing, Pairing::Waiting);
        assert_eq!(main.store.nodes().await.unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_different_main_certificate_stops_the_node() {
        let node = Node::new(Store::open_in_memory().await.unwrap(), std::env::temp_dir())
            .await
            .unwrap();
        let seen = Arc::new(Mutex::new(None));
        let client = pinned_client(Some("00".repeat(32)), Arc::clone(&seen)).unwrap();
        // Nothing to connect to here; the pin itself is what's tested.
        drop(client);
        let pin = Pin {
            expected: Some("00".repeat(32)),
            seen,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        };
        let cert = CertificateDer::from(vec![1u8, 2, 3]);
        let name = ServerName::try_from("main").unwrap();
        assert!(pin
            .verify_server_cert(&cert, &[], &name, &[], UnixTime::now())
            .is_err());
        let open = Pin {
            expected: None,
            ..pin
        };
        assert!(open
            .verify_server_cert(&cert, &[], &name, &[], UnixTime::now())
            .is_ok());
        assert!(
            open.seen.lock().unwrap().is_some(),
            "the first look is noted"
        );
        assert_eq!(node.status().pairing, Pairing::Setup);
    }
}
