//! Files that ship beside the program instead of inside it: the user guide
//! (`guide/`) and the language files (`lang/`), so they can be read, fixed
//! and added to without a new build.
//!
//! They live next to the program (`%LOCALAPPDATA%\Programs\DJINN\guide`,
//! `/opt/djinn/guide`), or in `../share/djinn/` when the program sits in a
//! `bin` folder (`~/.local/bin/djinn` keeps them in `~/.local/share/djinn/`).
//! Each folder has a `VERSION` file naming the DJINN it belongs to, and a
//! folder from another version is ignored: leftovers from an older install
//! never show old wording or an outdated guide. The web app keeps its own
//! copy of the language files, so DJINN works without the folder.
//! Debug builds read both straight from the source tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::Path as UrlPath;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::auth::AdminUser;
use crate::state::AppState;

/// The user guide's folder.
pub const GUIDE: &str = "guide";
/// The language files' folder.
pub const LANG: &str = "lang";
/// The file in each folder that names the version it belongs to.
pub const STAMP: &str = "VERSION";
/// Largest language file read; the English one is about 200 KB.
const MAX_LANGUAGE_FILE: u64 = 2 * 1024 * 1024;
/// Largest guide file served.
const MAX_GUIDE_FILE: u64 = 8 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/language/files", get(language_files))
        .route("/guide", get(guide_status))
        .route("/guide/{page}", get(guide_page))
        .route("/guide/images/{file}", get(guide_image))
}

/// Where this program keeps its content folders.
pub fn home() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    home_for(&exe)
}

fn home_for(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    if dir.file_name().is_some_and(|name| name == "bin") {
        Some(dir.parent()?.join("share").join("djinn"))
    } else {
        Some(dir.to_path_buf())
    }
}

/// The folder `name` that belongs to this version, if there is one.
pub fn folder(name: &str) -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(dir) = source_folder(name) {
            return Some(dir);
        }
    }
    let dir = home()?.join(name);
    belongs_here(&dir, env!("CARGO_PKG_VERSION")).then_some(dir)
}

/// A debug build's folder in the source tree.
fn source_folder(name: &str) -> Option<PathBuf> {
    let source = match name {
        GUIDE => "docs/guide",
        LANG => "web/src/i18n",
        _ => return None,
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(source);
    dir.is_dir().then_some(dir)
}

/// Whether `dir` is there and stamped with `version`.
fn belongs_here(dir: &Path, version: &str) -> bool {
    std::fs::read_to_string(dir.join(STAMP)).is_ok_and(|stamp| stamp.trim() == version)
}

/// The release file holding a version's `guide/` and `lang/`.
pub const RELEASE_FILE: &str = "djinn-content.zip";
/// Most a content zip may unpack to.
const MAX_UNPACKED: u64 = 256 * 1024 * 1024;

/// Unpacks a release's content zip into `home`: its `guide/` and `lang/`
/// take the place of the ones there, each swapped in whole. Anything else
/// in the zip is left out.
pub fn unpack(zip: &Path, home: &Path) -> anyhow::Result<()> {
    use std::io::Read;

    let staging = home.join(".djinn-content-new");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
    let mut budget = MAX_UNPACKED;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let top = name
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str());
        if !matches!(top, Some(GUIDE | LANG)) {
            continue;
        }
        let out = staging.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(&out)?;
        let copied = std::io::copy(&mut (&mut entry).take(budget + 1), &mut file)?;
        if copied > budget {
            let _ = std::fs::remove_dir_all(&staging);
            anyhow::bail!("the content download unpacks to more than it should");
        }
        budget -= copied;
    }
    for name in [GUIDE, LANG] {
        let new = staging.join(name);
        if !new.is_dir() {
            continue;
        }
        let target = home.join(name);
        let old = home.join(format!(".djinn-old-{name}"));
        let _ = std::fs::remove_dir_all(&old);
        if target.exists() {
            std::fs::rename(&target, &old)?;
        }
        if let Err(e) = std::fs::rename(&new, &target) {
            let _ = std::fs::rename(&old, &target);
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&old);
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
}

/// A language file's name: two or three lowercase letters, maybe a region
/// ("pt-br"), then `.json`.
fn language_code(file: &str) -> Option<&str> {
    let code = file.strip_suffix(".json")?;
    crate::language::looks_like_a_language(code).then_some(code)
}

type Catalog = serde_json::Map<String, serde_json::Value>;

/// Every language file in `dir` that reads as a catalog: text, or plural
/// forms ({ "one": ..., "other": ... }) of text.
fn read_catalogs(dir: &Path) -> BTreeMap<String, Catalog> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(code) = name.to_str().and_then(language_code) else {
            continue;
        };
        if entry
            .metadata()
            .map_or(true, |m| !m.is_file() || m.len() > MAX_LANGUAGE_FILE)
        {
            continue;
        }
        let parsed = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Catalog>(&bytes).ok());
        match parsed {
            Some(catalog) if catalog.values().all(is_text_or_plural) => {
                out.insert(code.to_owned(), catalog);
            }
            _ => {
                tracing::warn!(file = %entry.path().display(), "language file isn't a translation catalog; skipped")
            }
        }
    }
    out
}

fn is_text_or_plural(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(_) => true,
        serde_json::Value::Object(forms) => forms.values().all(serde_json::Value::is_string),
        _ => false,
    }
}

/// The language files beside the program, which the apps lay over their own
/// copies. Public, on both ports: pages ask before anyone signs in.
async fn language_files() -> Response {
    let catalogs = match folder(LANG) {
        Some(dir) => tokio::task::spawn_blocking(move || read_catalogs(&dir))
            .await
            .unwrap_or_default(),
        None => BTreeMap::new(),
    };
    ([(header::CACHE_CONTROL, "no-cache")], Json(catalogs)).into_response()
}

#[derive(Serialize)]
struct GuideStatus {
    /// The guide for this version is installed.
    installed: bool,
}

async fn guide_status(_: AdminUser) -> Json<GuideStatus> {
    Json(GuideStatus {
        installed: folder(GUIDE).is_some_and(|dir| dir.join("README.md").is_file()),
    })
}

/// A guide page: `getting-started.md`. Only plain names, so nothing outside
/// the guide's folder can be asked for.
fn page_name(page: &str) -> bool {
    page.strip_suffix(".md").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// A picture in the guide's `images/` folder: `rooms.webp`, or its
/// `CREDITS.md`.
fn image_type(file: &str) -> Option<&'static str> {
    let (stem, ext) = file.rsplit_once('.')?;
    if stem.is_empty()
        || !stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    match ext {
        "webp" => Some("image/webp"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "md" => Some("text/markdown; charset=utf-8"),
        _ => None,
    }
}

async fn guide_page(_: AdminUser, UrlPath(page): UrlPath<String>) -> Response {
    if !page_name(&page) {
        return StatusCode::NOT_FOUND.into_response();
    }
    send(
        folder(GUIDE).map(|dir| dir.join(page)),
        "text/markdown; charset=utf-8",
    )
    .await
}

async fn guide_image(_: AdminUser, UrlPath(file): UrlPath<String>) -> Response {
    let Some(kind) = image_type(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    send(folder(GUIDE).map(|dir| dir.join("images").join(file)), kind).await
}

async fn send(path: Option<PathBuf>, kind: &'static str) -> Response {
    let Some(path) = path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_GUIDE_FILE => {}
        _ => return StatusCode::NOT_FOUND.into_response(),
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, kind),
                (header::CACHE_CONTROL, "no-cache"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("djinn-content-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_program_in_a_bin_folder_keeps_its_content_in_share() {
        assert_eq!(
            home_for(Path::new("/home/a/.local/bin/djinn")).unwrap(),
            Path::new("/home/a/.local/share/djinn")
        );
        assert_eq!(
            home_for(Path::new("/opt/djinn/djinn")).unwrap(),
            Path::new("/opt/djinn")
        );
    }

    #[test]
    fn only_a_folder_stamped_with_this_version_counts() {
        let dir = temp();
        assert!(!belongs_here(&dir, "2026.11.18"), "no stamp");
        std::fs::write(dir.join(STAMP), "2026.11.4\n").unwrap();
        assert!(
            !belongs_here(&dir, "2026.11.18"),
            "an older install's leftovers"
        );
        std::fs::write(dir.join(STAMP), "2026.11.18\n").unwrap();
        assert!(belongs_here(&dir, "2026.11.18"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn language_files_are_read_only_when_they_are_catalogs() {
        let dir = temp();
        std::fs::write(
            dir.join("es.json"),
            r#"{"a":"uno","n":{"one":"{n} cosa","other":"{n} cosas"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("fr.json"), r#"{"a":["not text"]}"#).unwrap();
        std::fs::write(dir.join("de.json"), "not json").unwrap();
        std::fs::write(dir.join("notes.json"), r#"{"a":"b"}"#).unwrap();
        std::fs::write(dir.join("VERSION"), "x").unwrap();
        let catalogs = read_catalogs(&dir);
        assert_eq!(catalogs.keys().collect::<Vec<_>>(), ["es"]);
        assert_eq!(catalogs["es"]["a"], "uno");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn guide_names_stay_inside_the_guide() {
        assert!(page_name("getting-started.md"));
        assert!(!page_name("../secret.md"));
        assert!(!page_name("a/b.md"));
        assert!(!page_name(".md"));
        assert!(!page_name("README.txt"));
        assert_eq!(image_type("rooms.webp"), Some("image/webp"));
        assert_eq!(
            image_type("CREDITS.md"),
            Some("text/markdown; charset=utf-8")
        );
        assert_eq!(
            image_type("x.svg"),
            None,
            "pictures that can hold scripts aren't served"
        );
        assert_eq!(image_type("..webp"), None);
        assert_eq!(image_type("a/b.png"), None);
    }

    #[test]
    fn an_update_swaps_in_the_new_folders_and_nothing_else() {
        use std::io::Write;
        let home = temp();
        std::fs::create_dir_all(home.join("guide")).unwrap();
        std::fs::write(home.join("guide/old.md"), "old").unwrap();
        std::fs::write(home.join("djinn.toml"), "kept").unwrap();

        let zip_path = home.join("content.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("guide/VERSION", "2026.11.18"),
            ("guide/README.md", "# DJINN guide"),
            ("guide/images/a.webp", "picture"),
            ("lang/es.json", "{}"),
            ("other/evil.sh", "nope"),
            ("../escape.txt", "nope"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        unpack(&zip_path, &home).unwrap();
        assert!(home.join("guide/README.md").is_file());
        assert!(home.join("guide/images/a.webp").is_file());
        assert!(
            !home.join("guide/old.md").exists(),
            "the old guide is replaced whole"
        );
        assert!(home.join("lang/es.json").is_file());
        assert!(!home.join("other").exists());
        assert!(!home.parent().unwrap().join("escape.txt").exists());
        assert_eq!(
            std::fs::read_to_string(home.join("djinn.toml")).unwrap(),
            "kept"
        );
        assert!(!home.join(".djinn-content-new").exists());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn the_source_tree_has_the_guide_and_languages() {
        let guide = source_folder(GUIDE).unwrap();
        assert!(guide.join("README.md").is_file());
        let catalogs = read_catalogs(&source_folder(LANG).unwrap());
        assert!(catalogs.contains_key("en") && catalogs.contains_key("es"));
    }
}
