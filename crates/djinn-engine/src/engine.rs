//! The engine thread: owns the output and player, handles commands, and
//! reports events.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::outputs::Outputs;
use crate::player::{Player, Sink};
use crate::{Command, EngineError, Event, OutputConfig, PlayState};

/// How often the thread wakes when no commands arrive.
const TICK: Duration = Duration::from_millis(10);
const POSITION_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Internal mix rate. Every track is resampled to this.
    pub master_rate: u32,
    /// Outputs to start with; replace them with [`Command::SetOutputs`].
    pub outputs: Vec<OutputConfig>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            master_rate: 48_000,
            outputs: vec![OutputConfig::system_default(0)],
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

struct EngineThread {
    rx: mpsc::Receiver<Msg>,
    on_event: Box<dyn FnMut(Event) + Send>,
    player: Player,
    outputs: Outputs,
    paused: bool,
    state: Option<PlayState>,
    next_position: Instant,
}

impl EngineThread {
    fn new(
        config: EngineConfig,
        rx: mpsc::Receiver<Msg>,
        on_event: Box<dyn FnMut(Event) + Send>,
    ) -> Self {
        Self {
            player: Player::new(config.master_rate),
            outputs: Outputs::new(config.master_rate, config.outputs),
            rx,
            on_event,
            paused: false,
            state: None,
            next_position: Instant::now(),
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
            Command::Skip => self.player.skip(&mut self.outputs),
            Command::SetCrossfade(secs) => self.player.set_crossfade(secs),
            Command::SetVolume(volume) => self.outputs.set_volume(volume),
            Command::SetOutputs(configs) => self.outputs.configure(configs),
        }
        self.flush_events();
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.outputs.set_paused(paused);
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.outputs.tick(now);
        self.player.fill(&mut self.outputs);
        let consumed = self.outputs.consumed_frames();
        self.player.update(consumed);
        if now >= self.next_position {
            self.next_position = now + POSITION_INTERVAL;
            if let Some((id, ms)) = self.player.position(consumed) {
                (self.on_event)(Event::Position { id, ms });
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

    fn flush_events(&mut self) {
        for event in self.player.take_events() {
            (self.on_event)(event);
        }
        for event in self.outputs.take_events() {
            (self.on_event)(event);
        }
    }
}
