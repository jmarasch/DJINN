//! Restarting the whole computer, for when its sound is stuck and starting
//! DJINN again doesn't help (a sound card or driver that hung).
//!
//! On Linux DJINN runs as its own user, which may not restart the computer
//! by itself. The install images allow exactly one command for it in
//! `/etc/sudoers.d/djinn-reboot`, which this uses through `sudo -n` (never
//! asking for a password). On Windows a signed-in user may restart.

use std::process::{Command, Stdio};

/// The one command the install images let DJINN's user run as root.
#[cfg(target_os = "linux")]
const SYSTEMCTL: &str = "/usr/bin/systemctl";

/// Whether this computer lets DJINN restart it.
pub fn can_reboot() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Running as root (a hand-made service), or allowed by sudoers.
        if is_root() {
            return true;
        }
        quiet(Command::new("sudo").args(["-n", "-l", SYSTEMCTL, "reboot"]))
    }
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        false
    }
}

/// Restarts the computer now. Err says why it can't.
pub fn reboot() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let ok = if is_root() {
        quiet(Command::new(SYSTEMCTL).arg("reboot"))
    } else {
        quiet(Command::new("sudo").args(["-n", SYSTEMCTL, "reboot"]))
    };
    #[cfg(windows)]
    let ok = quiet(Command::new("shutdown").args(["/r", "/t", "0"]));
    #[cfg(not(any(target_os = "linux", windows)))]
    let ok = false;
    if ok {
        Ok(())
    } else {
        Err("this computer doesn't let DJINN restart it".to_owned())
    }
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    // The real user id, from the kernel's own account of this process.
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().next().map(|id| id == "0"))
        })
        .unwrap_or(false)
}

/// Runs a command with nothing shown, true if it succeeded.
fn quiet(command: &mut Command) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
