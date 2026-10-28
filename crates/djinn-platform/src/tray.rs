//! System tray icon for desktop installs: click to open DJINN, right-click
//! for the join code, the TV page, and shutdown.
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

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Left click or "Open DJINN".
    Open,
    ShowJoinCode,
    /// The full-screen now-playing page, for a TV.
    ShowTv,
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

/// The running tray icon. Dropping it removes the icon.
pub struct Tray {
    quit: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    /// Shows the icon. `on_action` runs on the tray's thread, so it should
    /// not block for long.
    pub fn spawn(
        tooltip: String,
        on_action: impl Fn(TrayAction) + Send + Sync + 'static,
    ) -> Result<Self, TrayError> {
        let (quit, quit_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("djinn-tray".to_owned())
            .spawn(move || {
                let tray = match build(&tooltip) {
                    Ok(tray) => tray,
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
                install_handlers(on_action);
                let _ = ready_tx.send(Ok(()));
                run_until_quit(&quit_rx);
                drop(tray);
            })
            .map_err(|e| TrayError::Unavailable(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                quit,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TrayError::Unavailable("tray thread exited".to_owned())),
        }
    }
}

impl Drop for Tray {
    /// Drop it last when shutting down: on Windows this tells a waiting
    /// sign-out or installer that DJINN is done.
    fn drop(&mut self) {
        #[cfg(windows)]
        session::DONE.store(true, std::sync::atomic::Ordering::Release);
        let _ = self.quit.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn build(tooltip: &str) -> Result<TrayIcon, TrayError> {
    let unavailable = |e: &dyn std::fmt::Display| TrayError::Unavailable(e.to_string());
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id(OPEN, "Open DJINN", true, None),
        &MenuItem::with_id(JOIN, "Show join QR code", true, None),
        &MenuItem::with_id(TV, "Open TV page", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(QUIT, "Shut down DJINN", true, None),
    ])
    .map_err(|e| unavailable(&e))?;
    let icon =
        Icon::from_rgba(icon_rgba(ICON_SIZE), ICON_SIZE, ICON_SIZE).map_err(|e| unavailable(&e))?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(tooltip)
        .with_title("DJINN")
        .with_icon(icon)
        .build()
        .map_err(|e| unavailable(&e))
}

fn install_handlers(on_action: std::sync::Arc<impl Fn(TrayAction) + Send + Sync + 'static>) {
    let on_click = std::sync::Arc::clone(&on_action);
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
            ..
        } = event
        {
            on_click(TrayAction::Open);
        }
    }));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = match event.id().as_ref() {
            OPEN => TrayAction::Open,
            JOIN => TrayAction::ShowJoinCode,
            TV => TrayAction::ShowTv,
            QUIT => TrayAction::Quit,
            _ => return,
        };
        on_action(action);
    }));
}

/// Waits for the quit signal. On Windows this thread must also pump window
/// messages, or the icon never sees clicks.
fn run_until_quit(quit: &mpsc::Receiver<()>) {
    loop {
        #[cfg(windows)]
        pump_messages();
        match quit.recv_timeout(Duration::from_millis(30)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => return,
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WM_CLOSE, WM_ENDSESSION,
        WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
    };

    static ON_END: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    /// Set when DJINN has shut down, so Windows may end the session.
    pub static DONE: AtomicBool = AtomicBool::new(false);
    /// Windows ends the session once WM_ENDSESSION returns; hold it this
    /// long at most while DJINN shuts down (connections get 5 s, audio and
    /// the database 2 s each).
    const WAIT: Duration = Duration::from_secs(12);

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
            // Yes, DJINN can close.
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION if wparam != 0 => {
                end();
                let started = Instant::now();
                while !DONE.load(Ordering::Acquire) && started.elapsed() < WAIT {
                    std::thread::sleep(Duration::from_millis(20));
                }
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

/// The DJINN mark (purple ring and dot on a dark rounded square) as RGBA,
/// drawn here so no image decoder is needed. Edges are antialiased by
/// distance.
fn icon_rgba(size: u32) -> Vec<u8> {
    const BG: [u8; 3] = [0x14, 0x11, 0x1f];
    const FG: [u8; 3] = [0xb4, 0x8c, 0xff];
    let s = size as f32;
    let center = s / 2.0;
    let corner = s * 0.22;
    let ring = (s * 0.27, s * 0.07); // radius, half thickness
    let dot = s * 0.08;
    let coverage = |d: f32| (0.5 - d).clamp(0.0, 1.0);

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded square: distance outside the inset rectangle.
            let qx = ((px - center).abs() - (center - corner)).max(0.0);
            let qy = ((py - center).abs() - (center - corner)).max(0.0);
            let bg = coverage((qx * qx + qy * qy).sqrt() - corner);
            let r = ((px - center).powi(2) + (py - center).powi(2)).sqrt();
            let fg = coverage((r - ring.0).abs() - ring.1).max(coverage(r - dot));
            for c in 0..3 {
                let v = f32::from(BG[c]) * (1.0 - fg) + f32::from(FG[c]) * fg;
                rgba.push(v.round() as u8);
            }
            rgba.push((bg * 255.0).round() as u8);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_ring_dot_and_transparent_corners() {
        let size = 64;
        let px = icon_rgba(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "corner is transparent");
        assert_eq!(at(32, 32), [0xb4, 0x8c, 0xff, 255], "center dot");
        assert_eq!(at(32, 32 - 17), [0xb4, 0x8c, 0xff, 255], "ring");
        assert_eq!(at(32, 32 - 10), [0x14, 0x11, 0x1f, 255], "gap between");
    }
}
