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

/// The number Android orders releases by, from a date version: 2026.11.02 is 20261102.
fn version_code(version: &str) -> Option<u32> {
    let mut parts = version.split('.').map(str::parse::<u32>);
    let (year, month, day) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    Some(year * 10_000 + month * 100 + day)
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
        assert_eq!(version_code("2026.11.02"), Some(20_261_102));
        assert!(version_code("2026.11.03") > version_code("2026.11.02"));
        assert!(version_code("2026.12.01") > version_code("2026.11.30"));
        assert!(version_code("2027.01.01") > version_code("2026.12.31"));
        assert_eq!(version_code("not a date"), None);
        assert_eq!(version_code("2026.11"), None);
    }

    #[test]
    fn a_release_matches_its_file() {
        if let Some(release) = release() {
            assert_eq!(release.sha256.len(), 64);
            assert!(release.size > 0);
        }
    }
}
