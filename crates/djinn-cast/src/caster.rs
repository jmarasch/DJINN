//! Runs casts: one task per device, each reconnecting on its own if its
//! device drops off, so a TV that reboots mid-party comes back.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, watch};

use crate::discover::{self, Device, DeviceKind};
use crate::{cast, upnp, CastCommand, CastError, CastMedia, CastState};

const RETRY: Duration = Duration::from_secs(10);

/// A device and how its cast is going.
#[derive(Clone, Debug, Serialize)]
pub struct CastStatus {
    #[serde(flatten)]
    pub device: Device,
    /// DJINN is casting to it (or trying to).
    pub active: bool,
    /// Showing the TV page rather than just playing audio.
    pub page: bool,
    pub playing: bool,
    pub buffering: bool,
    pub volume: Option<f32>,
    pub volume_fixed: bool,
    pub error: Option<String>,
}

struct Session {
    commands: mpsc::Sender<CastCommand>,
    state: watch::Receiver<CastState>,
    page: bool,
}

#[derive(Default)]
struct Inner {
    devices: Mutex<HashMap<String, Device>>,
    sessions: Mutex<HashMap<String, Session>>,
}

/// All casts. Cheap to clone.
#[derive(Clone)]
pub struct Caster {
    inner: Arc<Inner>,
    /// Bumped whenever any cast's status changes.
    changed: watch::Sender<u64>,
}

impl Default for Caster {
    fn default() -> Self {
        Self::new()
    }
}

impl Caster {
    pub fn new() -> Self {
        Self {
            inner: Arc::default(),
            changed: watch::Sender::new(0),
        }
    }

    /// Notified on every status change.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// Looks for devices for `wait` and remembers them. Devices being cast
    /// to stay listed even if they didn't answer this time.
    pub async fn discover(&self, wait: Duration) -> Vec<CastStatus> {
        let found = discover::discover(wait).await;
        if let Ok(mut devices) = self.inner.devices.lock() {
            let active: Vec<String> = self
                .inner
                .sessions
                .lock()
                .map(|s| s.keys().cloned().collect())
                .unwrap_or_default();
            devices.retain(|id, _| active.contains(id));
            for device in found {
                devices.insert(device.id.clone(), device);
            }
        }
        self.bump();
        self.status()
    }

    pub fn status(&self) -> Vec<CastStatus> {
        let devices: Vec<Device> = self
            .inner
            .devices
            .lock()
            .map(|d| d.values().cloned().collect())
            .unwrap_or_default();
        let sessions = self.inner.sessions.lock();
        let mut list: Vec<CastStatus> = devices
            .into_iter()
            .map(|device| {
                let session = sessions.as_ref().ok().and_then(|s| s.get(&device.id));
                let page = session.is_some_and(|s| s.page);
                let state = session.map(|s| s.state.borrow().clone());
                let active = state.is_some();
                let state = state.unwrap_or_default();
                CastStatus {
                    device,
                    active,
                    page,
                    playing: state.playing,
                    buffering: state.buffering,
                    volume: state.volume,
                    volume_fixed: state.volume_fixed,
                    error: state.error,
                }
            })
            .collect();
        list.sort_by(|a, b| {
            a.device
                .name
                .to_lowercase()
                .cmp(&b.device.name.to_lowercase())
        });
        list
    }

    /// Starts casting to a known device. Already casting the same kind of
    /// thing is fine; switching between audio and the page restarts it.
    pub async fn start(&self, device_id: &str, media: CastMedia) -> Result<(), CastError> {
        let device = self
            .inner
            .devices
            .lock()
            .ok()
            .and_then(|d| d.get(device_id).cloned())
            .ok_or(CastError::UnknownDevice)?;
        let page = matches!(media, CastMedia::Page { .. });
        if page && !device.screen {
            return Err(CastError::NoScreen);
        }
        let running = self
            .inner
            .sessions
            .lock()
            .ok()
            .and_then(|s| s.get(device_id).map(|s| s.page));
        match running {
            Some(same) if same == page => return Ok(()),
            Some(_) => {
                self.stop(device_id).await;
                // Let the old cast finish before the new one starts.
                for _ in 0..50 {
                    if !self.active().iter().any(|a| a == device_id) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            None => {}
        }
        let Ok(mut sessions) = self.inner.sessions.lock() else {
            return Ok(());
        };
        if sessions.contains_key(device_id) {
            return Ok(());
        }
        let (commands, receiver) = mpsc::channel(8);
        let (state, watcher) = watch::channel(CastState::default());
        sessions.insert(
            device_id.to_owned(),
            Session {
                commands,
                state: watcher,
                page,
            },
        );
        drop(sessions);
        let caster = self.clone();
        let id = device_id.to_owned();
        tokio::spawn(async move {
            caster.run(&id, &media, receiver, state).await;
            if let Ok(mut sessions) = caster.inner.sessions.lock() {
                sessions.remove(&id);
            }
            caster.bump();
        });
        self.bump();
        Ok(())
    }

    pub async fn stop(&self, device_id: &str) {
        if let Some(commands) = self.commands(device_id) {
            let _ = commands.send(CastCommand::Stop).await;
        }
    }

    /// Stops every cast.
    pub async fn stop_all(&self) {
        let all: Vec<mpsc::Sender<CastCommand>> = self
            .inner
            .sessions
            .lock()
            .map(|s| s.values().map(|s| s.commands.clone()).collect())
            .unwrap_or_default();
        for commands in all {
            let _ = commands.send(CastCommand::Stop).await;
        }
    }

    pub async fn set_volume(&self, device_id: &str, level: f32) {
        if let Some(commands) = self.commands(device_id) {
            let _ = commands.send(CastCommand::SetVolume(level)).await;
        }
    }

    /// Ids of devices being cast to.
    pub fn active(&self) -> Vec<String> {
        self.inner
            .sessions
            .lock()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn commands(&self, device_id: &str) -> Option<mpsc::Sender<CastCommand>> {
        self.inner
            .sessions
            .lock()
            .ok()?
            .get(device_id)
            .map(|s| s.commands.clone())
    }

    fn bump(&self) {
        self.changed.send_modify(|n| *n = n.wrapping_add(1));
    }

    async fn run(
        &self,
        id: &str,
        media: &CastMedia,
        mut commands: mpsc::Receiver<CastCommand>,
        state: watch::Sender<CastState>,
    ) {
        // Report every state change.
        let mut watcher = state.subscribe();
        let caster = self.clone();
        let reporter = tokio::spawn(async move {
            while watcher.changed().await.is_ok() {
                caster.bump();
            }
        });
        // The latest address each time, in case discovery found it moved.
        while let Some(device) = self
            .inner
            .devices
            .lock()
            .ok()
            .and_then(|d| d.get(id).cloned())
        {
            state.send_modify(|s| {
                s.playing = false;
                s.buffering = true;
            });
            tracing::info!(device = %device.name, "casting");
            let result = match device.kind {
                DeviceKind::Cast => cast::play(device.addr, media, &mut commands, &state).await,
                DeviceKind::Dlna | DeviceKind::Sonos => match media {
                    CastMedia::Stream { url, meta } => {
                        upnp::play(&device, url, meta, &mut commands, &state).await
                    }
                    CastMedia::Page { .. } => Err("this device can't show pages".to_owned()),
                },
            };
            match result {
                Ok(()) => {
                    tracing::info!(device = %device.name, "cast stopped");
                    break;
                }
                Err(error) => {
                    tracing::warn!(device = %device.name, %error, "cast lost, retrying");
                    state.send_modify(|s| {
                        s.playing = false;
                        s.buffering = false;
                        s.error = Some(error);
                    });
                }
            }
            // Wait to retry, unless told to stop meanwhile.
            let stop = tokio::select! {
                _ = tokio::time::sleep(RETRY) => false,
                command = commands.recv() => matches!(command, Some(CastCommand::Stop) | None),
            };
            if stop {
                break;
            }
        }
        reporter.abort();
    }
}
