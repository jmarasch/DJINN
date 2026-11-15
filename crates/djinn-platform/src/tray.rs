//! System tray icon for desktop installs: click to open DJINN, right-click
//! for the join code, the TV page, and shutdown. A node's menu has its
//! status page, its screen, Stop, its room and the room's join code.
//!
//! Linux uses the StatusNotifierItem D-Bus protocol (KDE, and GNOME with
//! the AppIndicator extension); Windows uses the native notification area.
//! The tray lives on its own thread, which Windows needs for its message
//! loop and which keeps the Linux backend's runtime out of the caller's.
//!
//! On Windows the same thread also answers sign-out, shutdown, and
//! installers asking DJINN to close (`session`), as if Quit was picked.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Left click, "Open DJINN" (a node's "Node status").
    Open,
    ShowJoinCode,
    /// The full-screen now-playing page, for a TV (a node's "Open DJINN").
    ShowTv,
    /// A node's "Stop" tick box: stop (`true`) or play again its music.
    StopMusic(bool),
    /// A node's room picked from the Room menu.
    PickRoom(i64),
    /// A node's "Open main DJINN": the main DJINN's own page.
    OpenMain,
    Quit,
    /// Windows is signing out or shutting down, or an installer asked
    /// DJINN to close. Shut down quickly.
    SessionEnding,
}

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("tray icon unavailable: {0}")]
    Unavailable(String),
}

const OPEN: &str = "open";
const JOIN: &str = "join";
const TV: &str = "tv";
const QUIT: &str = "quit";
const STOP: &str = "stop";
const ROOM: &str = "room:";
const MAIN: &str = "main";

/// Which menu the icon has.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrayMenu {
    /// The jukebox: open, join code, TV page, shut down.
    Main,
    /// A node: its status page, its screen, stop its music, its room, the
    /// room's join code, shut down.
    Node,
}

/// What the tray's thread is told.
enum Update {
    Quit,
    /// A menu item was picked (its id).
    Picked(String),
    /// The rooms to offer and the one the node is in.
    Rooms(Vec<(i64, String)>, Option<i64>),
}

/// The running tray icon. Dropping it removes the icon.
pub struct Tray {
    updates: mpsc::Sender<Update>,
    thread: Option<JoinHandle<()>>,
}

/// Keeps a node's Room menu current, from any thread.
#[derive(Clone)]
pub struct RoomMenu(mpsc::Sender<Update>);

impl RoomMenu {
    /// Offers `rooms` (id, name), ticking `current`.
    pub fn set(&self, rooms: Vec<(i64, String)>, current: Option<i64>) {
        let _ = self.0.send(Update::Rooms(rooms, current));
    }
}

impl Tray {
    /// Shows the icon. `on_action` runs on the tray's thread, so it should
    /// not block for long.
    pub fn spawn(
        tooltip: String,
        kind: TrayMenu,
        on_action: impl Fn(TrayAction) + Send + Sync + 'static,
    ) -> Result<Self, TrayError> {
        let (updates, updates_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let picked = updates.clone();
        let thread = std::thread::Builder::new()
            .name("djinn-tray".to_owned())
            .spawn(move || {
                let (tray, mut menu) = match build(&tooltip, kind) {
                    Ok(built) => built,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let on_action = std::sync::Arc::new(on_action);
                #[cfg(windows)]
                {
                    let on_action = std::sync::Arc::clone(&on_action);
                    session::watch(move || on_action(TrayAction::SessionEnding));
                }
                install_handlers(std::sync::Arc::clone(&on_action), picked);
                let _ = ready_tx.send(Ok(()));
                run_until_quit(&updates_rx, &mut menu, &*on_action);
                drop(tray);
                drop(menu);
            })
            .map_err(|e| TrayError::Unavailable(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                updates,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TrayError::Unavailable("tray thread exited".to_owned())),
        }
    }

    /// For keeping a node's Room menu current.
    pub fn room_menu(&self) -> RoomMenu {
        RoomMenu(self.updates.clone())
    }
}

impl Drop for Tray {
    /// Drop it last when shutting down: on Windows this tells a waiting
    /// sign-out or installer that DJINN is done.
    fn drop(&mut self) {
        #[cfg(windows)]
        session::DONE.store(true, std::sync::atomic::Ordering::Release);
        let _ = self.updates.send(Update::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The menu items whose state changes, kept on the tray's thread (they
/// can't leave it).
struct LiveMenu {
    stop: CheckMenuItem,
    stopped: bool,
    rooms: Submenu,
    /// What `rooms` lists, and its items.
    offered: Vec<(i64, String)>,
    room_items: Vec<(i64, CheckMenuItem)>,
}

impl LiveMenu {
    fn tick_room(&self, current: Option<i64>) {
        for (id, item) in &self.room_items {
            item.set_checked(Some(*id) == current);
        }
    }

    fn set_rooms(&mut self, rooms: Vec<(i64, String)>, current: Option<i64>) {
        if rooms != self.offered {
            for (_, item) in self.room_items.drain(..) {
                let _ = self.rooms.remove(&item);
            }
            for (id, name) in &rooms {
                let item = CheckMenuItem::with_id(format!("{ROOM}{id}"), name, true, false, None);
                if self.rooms.append(&item).is_ok() {
                    self.room_items.push((*id, item));
                }
            }
            self.rooms.set_enabled(!rooms.is_empty());
            self.offered = rooms;
        }
        self.tick_room(current);
    }

    /// What picking `id` means, setting the tick marks to match (the menu
    /// may or may not have flipped them itself).
    fn picked(&mut self, id: &str) -> Option<TrayAction> {
        Some(match id {
            OPEN => TrayAction::Open,
            JOIN => TrayAction::ShowJoinCode,
            TV => TrayAction::ShowTv,
            QUIT => TrayAction::Quit,
            MAIN => TrayAction::OpenMain,
            STOP => {
                self.stopped = !self.stopped;
                self.stop.set_checked(self.stopped);
                TrayAction::StopMusic(self.stopped)
            }
            _ => {
                let room = id.strip_prefix(ROOM)?.parse().ok()?;
                self.tick_room(Some(room));
                TrayAction::PickRoom(room)
            }
        })
    }
}

fn build(tooltip: &str, kind: TrayMenu) -> Result<(TrayIcon, LiveMenu), TrayError> {
    let unavailable = |e: &dyn std::fmt::Display| TrayError::Unavailable(e.to_string());
    let menu = Menu::new();
    let live = LiveMenu {
        stop: CheckMenuItem::with_id(STOP, "Stop", true, false, None),
        stopped: false,
        rooms: Submenu::new("Room", false),
        offered: Vec::new(),
        room_items: Vec::new(),
    };
    let added = match kind {
        TrayMenu::Main => menu.append_items(&[
            &MenuItem::with_id(OPEN, "Open DJINN", true, None),
            &MenuItem::with_id(JOIN, "Show join QR code", true, None),
            &MenuItem::with_id(TV, "Open TV page", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(QUIT, "Shut down DJINN", true, None),
        ]),
        TrayMenu::Node => menu.append_items(&[
            &MenuItem::with_id(OPEN, "Node status", true, None),
            &MenuItem::with_id(TV, "Node TV page", true, None),
            &MenuItem::with_id(MAIN, "Open main DJINN", true, None),
            &PredefinedMenuItem::separator(),
            &live.stop,
            &live.rooms,
            &MenuItem::with_id(JOIN, "Show code", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(QUIT, "Shut down", true, None),
        ]),
    };
    added.map_err(|e| unavailable(&e))?;
    let icon =
        Icon::from_rgba(icon_rgba(ICON_SIZE), ICON_SIZE, ICON_SIZE).map_err(|e| unavailable(&e))?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(tooltip)
        .with_title("DJINN")
        .with_icon(icon)
        .build()
        .map(|tray| (tray, live))
        .map_err(|e| unavailable(&e))
}

/// Clicks on the icon go straight to `on_action`; menu picks go to the
/// tray's thread, which owns the items they may change.
fn install_handlers(
    on_action: std::sync::Arc<impl Fn(TrayAction) + Send + Sync + 'static>,
    picked: mpsc::Sender<Update>,
) {
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
            ..
        } = event
        {
            on_action(TrayAction::Open);
        }
    }));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = picked.send(Update::Picked(event.id().as_ref().to_owned()));
    }));
}

/// Handles updates until told to quit. On Windows this thread must also
/// pump window messages, or the icon never sees clicks.
fn run_until_quit(
    updates: &mpsc::Receiver<Update>,
    menu: &mut LiveMenu,
    on_action: &dyn Fn(TrayAction),
) {
    loop {
        #[cfg(windows)]
        pump_messages();
        match updates.recv_timeout(Duration::from_millis(30)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(Update::Picked(id)) => {
                if let Some(action) = menu.picked(&id) {
                    on_action(action);
                }
            }
            Ok(Update::Rooms(rooms, current)) => menu.set_rooms(rooms, current),
            Ok(Update::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(windows)]
fn pump_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    // SAFETY: MSG is plain data; PeekMessageW fills it before it is read,
    // and a null window handle means "any window on this thread".
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Windows asks a program to close by sending its top-level windows
/// WM_QUERYENDSESSION and WM_ENDSESSION (sign-out, shutdown, and installers
/// through the Restart Manager) or WM_CLOSE (`taskkill` without `/F`).
/// tray-icon's own window ignores them, so DJINN would be killed after a
/// timeout; this hidden window turns them into Quit instead.
#[cfg(windows)]
mod session {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Shutdown::{
        ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WM_CLOSE, WM_ENDSESSION,
        WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
    };

    static ON_END: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    /// Set when DJINN has shut down, so Windows may end the session.
    pub static DONE: AtomicBool = AtomicBool::new(false);
    /// Windows ends the session once WM_ENDSESSION returns; hold it this
    /// long at most while DJINN shuts down (1 s each for connections,
    /// audio, and the database). Windows shows "preventing sign-out" after
    /// a few seconds, so keep it short.
    const WAIT: Duration = Duration::from_secs(4);

    /// Creates the window on the calling thread, which must pump messages.
    pub fn watch(on_end: impl Fn() + Send + Sync + 'static) {
        if ON_END.set(Box::new(on_end)).is_err() {
            return;
        }
        let class: Vec<u16> = "DJINN session\0".encode_utf16().collect();
        // SAFETY: the class name outlives both calls (Windows copies it),
        // the window procedure is a plain function, and a zeroed WNDCLASSW
        // is valid (null handles and no extra bytes).
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            // Top level (not message-only), since that's what Windows asks;
            // never shown.
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            // Yes, DJINN can close. If Windows has to wait, its screen
            // says why instead of "an app".
            WM_QUERYENDSESSION => {
                let reason: Vec<u16> = "DJINN is saving and closing...\0".encode_utf16().collect();
                // SAFETY: a live window of this thread and a terminated
                // string Windows copies.
                unsafe { ShutdownBlockReasonCreate(hwnd, reason.as_ptr()) };
                1
            }
            WM_ENDSESSION => {
                if wparam != 0 {
                    end();
                    let started = Instant::now();
                    while !DONE.load(Ordering::Acquire) && started.elapsed() < WAIT {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                // SAFETY: as above; harmless if no reason was set.
                unsafe { ShutdownBlockReasonDestroy(hwnd) };
                0
            }
            WM_CLOSE => {
                end();
                0
            }
            // SAFETY: the arguments are the ones Windows passed in.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    fn end() {
        if let Some(on_end) = ON_END.get() {
            on_end();
        }
    }
}

const ICON_SIZE: u32 = 64;

/// The DJINN mark as RGBA (the web app's small favicon: two grooves of a
/// record opening into a waveform, on a dark rounded square), drawn here so
/// no image decoder is needed. Shapes are distances in the favicon's 80-unit
/// box (-8..72), so edges are antialiased at any size.
fn icon_rgba(size: u32) -> Vec<u8> {
    const BG: [u8; 3] = [0x14, 0x11, 0x1f];
    const RING: [u8; 3] = [0xb4, 0x8c, 0xff];
    const BAR: [u8; 3] = [0x3f, 0xe0, 0xcf];
    const CENTER: (f32, f32) = (25.0, 32.0);
    /// Groove radius and the half-angle (degrees) left open on the right.
    const GROOVES: [(f32, f32); 2] = [(12.0, 58.0), (20.0, 40.0)];
    const GROOVE_HALF: f32 = 2.5;
    const HUB: f32 = 5.5;
    /// Bar x and height, centered on y = 32.
    const BARS: [(f32, f32); 4] = [(34.0, 30.0), (42.0, 46.0), (50.0, 26.0), (58.0, 12.0)];
    const BAR_HALF: f32 = 2.75;
    /// Space kept clear around the bars, so they stand apart from the grooves.
    const PARTING: f32 = 1.5;

    let unit = size as f32 / 80.0;
    // Coverage of a shape whose edge is `d` units away (negative inside).
    let coverage = |d: f32| (0.5 - d * unit).clamp(0.0, 1.0);
    let groove = |x: f32, y: f32, (r, open): (f32, f32)| {
        let (dx, dy) = (x - CENTER.0, CENTER.1 - y);
        let open = open.to_radians();
        if dy.atan2(dx).abs() <= open {
            // In the opening: distance to the nearer end of the arc.
            let ex = CENTER.0 + r * open.cos();
            let ey = r * open.sin();
            ((x - ex).powi(2) + (dy.abs() - ey).powi(2)).sqrt() - GROOVE_HALF
        } else {
            ((dx * dx + dy * dy).sqrt() - r).abs() - GROOVE_HALF
        }
    };
    let bars = |x: f32, y: f32| {
        BARS.iter()
            .map(|&(bx, h)| {
                let dy = ((y - 32.0).abs() - h / 2.0).max(0.0);
                ((x - bx).powi(2) + dy * dy).sqrt() - BAR_HALF
            })
            .fold(f32::MAX, f32::min)
    };
    let blend = |under: [u8; 3], over: [u8; 3], amount: f32| {
        let mut out = [0u8; 3];
        for c in 0..3 {
            let v = f32::from(under[c]) * (1.0 - amount) + f32::from(over[c]) * amount;
            out[c] = v.round() as u8;
        }
        out
    };

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let x = (px as f32 + 0.5) / unit - 8.0;
            let y = (py as f32 + 0.5) / unit - 8.0;
            // Rounded square, 18 units at the corners.
            let qx = ((x - 32.0).abs() - 22.0).max(0.0);
            let qy = ((y - 32.0).abs() - 22.0).max(0.0);
            let square = coverage((qx * qx + qy * qy).sqrt() - 18.0);
            let to_bars = bars(x, y);
            let hub = ((x - CENTER.0).powi(2) + (y - CENTER.1).powi(2)).sqrt() - HUB;
            let rings = GROOVES.iter().map(|&g| groove(x, y, g)).fold(hub, f32::min);
            let ring = coverage(rings) * (1.0 - coverage(to_bars - PARTING));
            let color = blend(blend(BG, RING, ring), BAR, coverage(to_bars));
            rgba.extend_from_slice(&color);
            rgba.push((square * 255.0).round() as u8);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_hub_grooves_bars_and_transparent_corners() {
        let size = 64;
        let px = icon_rgba(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // Units map to pixels as (u + 8) * 0.8.
        assert_eq!(at(0, 0)[3], 0, "corner is transparent");
        assert_eq!(at(26, 32), [0xb4, 0x8c, 0xff, 255], "hub");
        assert_eq!(at(20, 32), [0x14, 0x11, 0x1f, 255], "gap around the hub");
        assert_eq!(at(16, 32), [0xb4, 0x8c, 0xff, 255], "first groove");
        assert_eq!(at(39, 32), [0x3f, 0xe0, 0xcf, 255], "tallest bar");
    }
}
