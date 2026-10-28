//! Operating system integration: the tray icon, finding a LAN device's
//! hardware address, the Windows console, how much memory there is, and
//! whether Windows treats the network as Public.
//! Kept free of C dependencies so the Windows code can be checked from
//! Linux with
//! `cargo check -p djinn-platform --target x86_64-pc-windows-gnu`.

pub mod console;
pub mod neighbors;
pub mod network;
pub mod system;
pub mod tray;
