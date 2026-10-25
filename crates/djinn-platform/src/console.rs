//! Release builds on Windows have no console window. When started from a
//! terminal they borrow that terminal's console so logs still show.

/// Attaches to the parent process's console if there is one. Returns true
/// when standard output goes somewhere visible.
#[cfg(windows)]
pub fn attach_parent_console() -> bool {
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetConsoleWindow, ATTACH_PARENT_PROCESS,
    };
    // SAFETY: plain Win32 calls with no pointers.
    unsafe { !GetConsoleWindow().is_null() || AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

#[cfg(not(windows))]
pub fn attach_parent_console() -> bool {
    true
}
