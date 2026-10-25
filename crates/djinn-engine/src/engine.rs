//! The engine thread: owns the output and player, handles commands, and
//! reports events.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::output::Output;
use crate::player::{Player, Sink};
use crate::{Command, EngineError, Event, PlayState};

/// How often the thread wakes when no commands arrive.
const TICK: Duration = Duration::from_millis(10);
const POSITION_INTERVAL: Duration = Duration::from_millis(500);
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(3);
const OUTPUT_RETRY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Internal mix rate. Every track is resampled to this.
    pub master_rate: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            master_rate: 48_000,
        }
    }
}

enum Msg {
    Command(Command),
    Shutdown,
}

/// Handle to the engine thread. Dropping it stops playback.
pub struct Engine {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Starts the engine thread. `on_event` runs on that thread, so it
    /// should hand events off quickly (for example, into a channel).
    pub fn start(
        config: EngineConfig,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> Result<Self, EngineError> {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("djinn-engine".to_owned())
            .spawn(move || EngineThread::new(config, rx, Box::new(on_event)).run())
            .map_err(|e| EngineError::Output(format!("failed to start engine thread: {e}")))?;
        Ok(Self {
            tx,
            thread: Some(thread),
        })
    }

    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.tx
            .send(Msg::Command(command))
            .map_err(|_| EngineError::Stopped)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Stand-in sink while no output device is open: nothing plays.
struct Detached {
    frames: u64,
}

impl Sink for Detached {
    fn free_frames(&mut self) -> usize {
        0
    }
    fn write(&mut self, _: &[f32]) {}
    fn consumed_frames(&self) -> u64 {
        self.frames
    }
    fn flush(&mut self) {}
}

struct EngineThread {
    config: EngineConfig,
    rx: mpsc::Receiver<Msg>,
    on_event: Box<dyn FnMut(Event) + Send>,
    player: Player,
    output: Option<Output>,
    retry_at: Instant,
    last_output_error: Option<String>,
    paused: bool,
    volume: f32,
    state: Option<PlayState>,
    next_position: Instant,
    next_device_check: Instant,
}

impl EngineThread {
    fn new(
        config: EngineConfig,
        rx: mpsc::Receiver<Msg>,
        on_event: Box<dyn FnMut(Event) + Send>,
    ) -> Self {
        let now = Instant::now();
        Self {
            player: Player::new(config.master_rate),
            config,
            rx,
            on_event,
            output: None,
            retry_at: now,
            last_output_error: None,
            paused: false,
            volume: 1.0,
            state: None,
            next_position: now,
            next_device_check: now + DEVICE_CHECK_INTERVAL,
        }
    }

    fn run(mut self) {
        (self.on_event)(Event::NextNeeded { after: None });
        loop {
            match self.rx.recv_timeout(TICK) {
                Ok(Msg::Command(cmd)) => self.handle(cmd),
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            loop {
                match self.rx.try_recv() {
                    Ok(Msg::Command(cmd)) => self.handle(cmd),
                    Ok(Msg::Shutdown) => return,
                    Err(_) => break,
                }
            }
            self.tick();
        }
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::SetNext(track) => self.player.set_next(track),
            Command::Play => self.set_paused(false),
            Command::Pause => self.set_paused(true),
            Command::Skip => match self.output.as_mut() {
                Some(output) => self.player.skip(output),
                None => {
                    let mut sink = Detached {
                        frames: self.player.written(),
                    };
                    self.player.skip(&mut sink);
                }
            },
            Command::SetVolume(volume) => {
                self.volume = volume.clamp(0.0, 1.0);
                if let Some(output) = &self.output {
                    output.set_volume(self.volume);
                }
            }
        }
        self.flush_events();
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if let Some(output) = &self.output {
            output.set_paused(paused);
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.manage_output(now);

        if let Some(output) = self.output.as_mut() {
            self.player.fill(output);
            let consumed = output.consumed_frames();
            self.player.update(consumed);
            if now >= self.next_position {
                self.next_position = now + POSITION_INTERVAL;
                if let Some((id, ms)) = self.player.position(consumed) {
                    (self.on_event)(Event::Position { id, ms });
                }
            }
        }
        self.flush_events();

        let state = if self.paused {
            PlayState::Paused
        } else if self.player.is_active() {
            PlayState::Playing
        } else {
            PlayState::Idle
        };
        if self.state != Some(state) {
            self.state = Some(state);
            (self.on_event)(Event::StateChanged(state));
        }
    }

    fn manage_output(&mut self, now: Instant) {
        if let Some(output) = &self.output {
            let mut lost = None;
            match output.poll_errors() {
                Ok(minor) => {
                    for message in minor {
                        tracing::debug!(%message, "audio output");
                    }
                }
                Err(message) => lost = Some(message),
            }
            if lost.is_none() && now >= self.next_device_check {
                self.next_device_check = now + DEVICE_CHECK_INTERVAL;
                if output.default_device_changed() {
                    tracing::info!("default output device changed, reopening");
                    self.output = None;
                    self.retry_at = now;
                }
            }
            if let Some(message) = lost {
                tracing::warn!(%message, "audio output lost");
                (self.on_event)(Event::OutputError {
                    error: message.clone(),
                });
                self.last_output_error = Some(message);
                self.output = None;
                self.retry_at = now + OUTPUT_RETRY;
            }
        }

        if self.output.is_none() && now >= self.retry_at {
            match Output::open_default(self.config.master_rate, self.player.written()) {
                Ok(output) => {
                    output.set_volume(self.volume);
                    output.set_paused(self.paused);
                    tracing::info!(
                        device = output.name(),
                        rate = output.sample_rate(),
                        channels = output.channels(),
                        "audio output opened"
                    );
                    (self.on_event)(Event::OutputOpened {
                        device: output.name().to_owned(),
                        sample_rate: output.sample_rate(),
                        channels: output.channels(),
                    });
                    self.last_output_error = None;
                    self.output = Some(output);
                }
                Err(e) => {
                    let message = e.to_string();
                    // Report each distinct failure once, not every retry.
                    if self.last_output_error.as_deref() != Some(message.as_str()) {
                        tracing::warn!(%message, "cannot open audio output, retrying");
                        (self.on_event)(Event::OutputError {
                            error: message.clone(),
                        });
                        self.last_output_error = Some(message);
                    }
                    self.retry_at = now + OUTPUT_RETRY;
                }
            }
        }
    }

    fn flush_events(&mut self) {
        for event in self.player.take_events() {
            (self.on_event)(event);
        }
    }
}
