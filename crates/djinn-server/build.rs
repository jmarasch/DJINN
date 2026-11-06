//! Bakes the git commit and its date into the binary for the About page,
//! and on Windows the icon and version info into the exe.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Cargo needs unpadded numeric parts; releases display a padded date.
/// A nightly keeps its suffix: 2027.1.5-nightly is 2027.01.05-nightly.
fn release_version(version: &str) -> String {
    let (core, suffix) = match version.split_once('-') {
        Some((core, suffix)) => (core, format!("-{suffix}")),
        None => (version, String::new()),
    };
    let parts: Vec<_> = core.split('.').collect();
    if let [year, month, day] = parts.as_slice() {
        if let (Ok(year), Ok(month), Ok(day)) = (
            year.parse::<u32>(),
            month.parse::<u32>(),
            day.parse::<u32>(),
        ) {
            if year >= 1000 && (1..=12).contains(&month) && (1..=31).contains(&day) {
                return format!("{year:04}.{month:02}.{day:02}{suffix}");
            }
        }
    }
    version.to_owned()
}

fn main() {
    // A new commit or checkout changes these.
    for path in [
        "../../.git/HEAD",
        "../../.git/refs/heads",
        "../../.git/packed-refs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_default();
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_default();
    println!("cargo:rustc-env=DJINN_COMMIT={commit}");
    println!("cargo:rustc-env=DJINN_COMMIT_DATE={date}");
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    println!(
        "cargo:rustc-env=DJINN_RELEASE_VERSION={}",
        release_version(&cargo_version)
    );
    // Nightly builds are numbered like 2027.1.5-nightly (see docs/releasing.md).
    let channel = if cargo_version.ends_with("-nightly") {
        "nightly"
    } else {
        "stable"
    };
    println!("cargo:rustc-env=DJINN_CHANNEL={channel}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = "../../packaging/windows/djinn.ico";
        println!("cargo:rerun-if-changed={icon}");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon)
            .set("ProductName", "DJINN")
            .set("FileDescription", "DJINN")
            .set("CompanyName", "New Terra Studios")
            .set("LegalCopyright", "Copyright (c) 2026 New Terra Studios");
        // A missing resource compiler only costs the icon.
        if let Err(e) = res.compile() {
            println!("cargo:warning=no icon in the exe: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::release_version;

    #[test]
    fn public_versions_pad_dates_and_keep_old_versions() {
        assert_eq!(release_version("2027.1.5"), "2027.01.05");
        assert_eq!(release_version("2026.10.31"), "2026.10.31");
        assert_eq!(release_version("0.4.5"), "0.4.5");
        assert_eq!(release_version("2027.1.5-nightly"), "2027.01.05-nightly");
    }
}
