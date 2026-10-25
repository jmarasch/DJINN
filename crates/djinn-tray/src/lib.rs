//! System tray icon for desktop installs: click to open DJINN, right-click
//! for the join code and shutdown.
//!
//! Linux uses the StatusNotifierItem D-Bus protocol (KDE, and GNOME with
//! the AppIndicator extension); Windows uses the native notification area.
//! The tray lives on its own thread, which Windows needs for its message
//! loop and which keeps the Linux backend's runtime out of the caller's.

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
    Quit,
}

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("tray icon unavailable: {0}")]
    Unavailable(String),
}

const OPEN: &str = "open";
const JOIN: &str = "join";
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
    fn drop(&mut self) {
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

fn install_handlers(on_action: impl Fn(TrayAction) + Send + Sync + 'static) {
    let on_action = std::sync::Arc::new(on_action);
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
