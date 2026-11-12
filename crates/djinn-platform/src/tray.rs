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

/// The DJINN icon on a dark rounded square, as 64 x 64 RGBA
/// (the same picture as `web/public/icon-512.png`), stored as raw pixels so
/// no image decoder is needed. Made with ImageMagick:
/// `convert icon.png -depth 8 rgba:tray-icon-64.rgba`.
static ICON_PIXELS: &[u8; 64 * 64 * 4] = include_bytes!("tray-icon-64.rgba");

fn icon_rgba(size: u32) -> Vec<u8> {
    debug_assert_eq!(size, ICON_SIZE);
    ICON_PIXELS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_square_with_transparent_corners_and_a_solid_middle() {
        let size = ICON_SIZE;
        let px = icon_rgba(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0, "corner is transparent");
        assert_eq!(alpha(size / 2, size / 2), 255, "middle is solid");
    }
}
