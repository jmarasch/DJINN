//! Whether Windows has the network guests use set to Public. Its firewall
//! then drops phones' connections before DJINN sees them, since the rule
//! people allow on first start only covers private networks.

use std::net::IpAddr;

/// `Some(true)` if the network `ip` belongs to is set to Public. `None`
/// outside Windows or when it can't be told.
pub fn is_public(ip: IpAddr) -> Option<bool> {
    imp::is_public(ip)
}

#[cfg(windows)]
mod imp {
    use std::net::IpAddr;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// No console window for PowerShell.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn is_public(ip: IpAddr) -> Option<bool> {
        // `ip` is a parsed address, so it can't break out of the quotes.
        let script = format!(
            "(Get-NetConnectionProfile -ErrorAction Stop -InterfaceIndex \
             (Get-NetIPAddress -ErrorAction Stop -IPAddress '{ip}').InterfaceIndex).NetworkCategory"
        );
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        match String::from_utf8_lossy(&out.stdout).trim() {
            "" => None,
            category => Some(category == "Public"),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::net::IpAddr;

    pub fn is_public(_: IpAddr) -> Option<bool> {
        None
    }
}
