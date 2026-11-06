//! The Android app, carried inside the server: whoever can reach GO can get
//! it, with no store and no internet, and its version always matches. The
//! release workflow builds the APK first and leaves it in `android-dist/` for
//! the server build; builds without one simply don't offer it.

use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rust_embed::RustEmbed;
use serde::Serialize;

#[derive(RustEmbed)]
#[folder = "../../android-dist/"]
#[allow_missing = true]
struct Dist;

const APK: &str = "djinn-go-android.apk";
/// Where GO serves it, public like the sign-in page: people need it to get started.
pub const APK_PATH: &str = "/go/app/djinn-go.apk";
const APK_TYPE: &str = "application/vnd.android.package-archive";

/// The app this server carries.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AppRelease {
    pub version: String,
    /// What the app compares with its own, to tell whether this one is newer.
    pub version_code: u32,
    /// For checking a download, by the app itself or by someone comparing by eye.
    pub sha256: String,
    pub size: u64,
}

/// The number Android orders releases by, from a date version: 2026.11.02 is
/// 2026110250 and that day's nightly, 2026.11.02-nightly, is 2026110200, so a
/// day's nightly comes before its stable release and both after the day
/// before. Same as `versionCode` in android/app/build.gradle.kts.
fn version_code(version: &str) -> Option<u32> {
    let (date, nightly) = match version.strip_suffix("-nightly") {
        Some(date) => (date, true),
        None => (version, false),
    };
    let mut parts = date.split('.').map(str::parse::<u32>);
    let (year, month, day) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if parts.next().is_some() {
        return None;
    }
    let day_code = year
        .checked_mul(10_000)?
        .checked_add(month * 100 + day)?
        .checked_mul(100)?;
    day_code.checked_add(if nightly { 0 } else { 50 })
}

pub fn release() -> Option<AppRelease> {
    let file = Dist::get(APK)?;
    let version = env!("DJINN_RELEASE_VERSION");
    Some(AppRelease {
        version: version.to_owned(),
        version_code: version_code(version)?,
        sha256: file
            .metadata
            .sha256_hash()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        size: file.data.len() as u64,
    })
}

/// Which app this server carries, or null.
pub async fn latest() -> Json<Option<AppRelease>> {
    Json(release())
}

pub async fn download() -> Response {
    let Some(file) = Dist::get(APK) else {
        return (StatusCode::NOT_FOUND, "This server doesn't carry the app.").into_response();
    };
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(APK_TYPE)),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"djinn-go.apk\""),
            ),
        ],
        Body::from(file.data),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_codes_go_up_with_the_date() {
        assert_eq!(version_code("2026.11.02"), Some(2_026_110_250));
        assert!(version_code("2026.11.03") > version_code("2026.11.02"));
        assert!(version_code("2026.12.01") > version_code("2026.11.30"));
        assert!(version_code("2027.01.01") > version_code("2026.12.31"));
        assert_eq!(version_code("not a date"), None);
        assert_eq!(version_code("2026.11"), None);
        // Every new code is above the old numbering (2026.11.04 was 20261104).
        assert!(version_code("2026.11.05-nightly") > Some(20_261_104));
    }

    #[test]
    fn nightlies_come_between_stable_releases() {
        assert_eq!(version_code("2026.11.05-nightly"), Some(2_026_110_500));
        assert!(version_code("2026.11.05-nightly") > version_code("2026.11.04"));
        assert!(version_code("2026.11.05-nightly") < version_code("2026.11.05"));
        assert!(version_code("2026.11.06-nightly") > version_code("2026.11.05"));
        assert_eq!(version_code("2026.11.05-beta"), None);
        // Android's limit is 2,100,000,000.
        assert!(version_code("2099.12.31").is_some_and(|c| c < 2_100_000_000));
    }

    #[test]
    fn a_release_matches_its_file() {
        if let Some(release) = release() {
            assert_eq!(release.sha256.len(), 64);
            assert!(release.size > 0);
        }
    }
}
