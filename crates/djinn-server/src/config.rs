//! Startup configuration: `config.toml` plus `DJINN_*` environment overrides.
//!
//! Env vars use `__` to separate sections, for example
//! `DJINN_SERVER__LISTEN=127.0.0.1:9000` or `DJINN_LIBRARY__ROOTS='["/a","/b"]'`.
//! Runtime-tunable settings live in the database, not here.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

pub const MAX_CROSSFADE_SECS: u32 = 12;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub library: LibraryConfig,
    pub database: DatabaseConfig,
    pub audio: AudioConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    /// Show a system tray icon when a desktop session is available.
    pub tray: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 8066)),
            tray: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct LibraryConfig {
    pub roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub path: PathBuf,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("djinn.db"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AudioConfig {
    pub master_rate: u32,
    pub crossfade_secs: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            master_rate: 48_000,
            crossfade_secs: 0,
        }
    }
}

/// Where the config came from, for logging.
#[derive(Debug)]
pub enum Source {
    File(PathBuf),
    /// The default location had no file, so built-in defaults were used.
    Defaults {
        looked_at: Option<PathBuf>,
    },
}

impl Config {
    /// Loads config from `explicit` if given (it must exist), otherwise from
    /// the platform default location if a file is there, otherwise defaults.
    /// Env overrides apply in every case.
    pub fn load(explicit: Option<&Path>) -> anyhow::Result<(Self, Source)> {
        let (path, source) = match explicit {
            Some(path) => {
                if !path.is_file() {
                    bail!("config file not found: {}", path.display());
                }
                (Some(path.to_path_buf()), Source::File(path.to_path_buf()))
            }
            None => match default_path() {
                Some(path) if path.is_file() => (Some(path.clone()), Source::File(path)),
                looked_at => (None, Source::Defaults { looked_at }),
            },
        };

        let mut config = Self::from_figment(figment(path.as_deref()))?;
        // Relative paths are relative to the config file, so a service finds
        // its database no matter what directory it was started from.
        if let Some(dir) = path.as_deref().and_then(Path::parent) {
            config.resolve_relative_paths(dir);
        }
        Ok((config, source))
    }

    fn resolve_relative_paths(&mut self, base: &Path) {
        let resolve = |p: &mut PathBuf| {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        };
        resolve(&mut self.database.path);
        self.library.roots.iter_mut().for_each(resolve);
    }

    fn from_figment(figment: Figment) -> anyhow::Result<Self> {
        let config: Self = figment.extract().context("invalid configuration")?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if !(8_000..=192_000).contains(&self.audio.master_rate) {
            bail!(
                "audio.master_rate must be between 8000 and 192000, got {}",
                self.audio.master_rate
            );
        }
        if self.audio.crossfade_secs > MAX_CROSSFADE_SECS {
            bail!(
                "audio.crossfade_secs must be between 0 and {MAX_CROSSFADE_SECS}, got {}",
                self.audio.crossfade_secs
            );
        }
        Ok(())
    }
}

fn figment(path: Option<&Path>) -> Figment {
    let mut figment = Figment::from(Serialized::defaults(Config::default()));
    if let Some(path) = path {
        figment = figment.merge(Toml::file_exact(path));
    }
    figment.merge(Env::prefixed("DJINN_").split("__"))
}

/// Next to the binary on Windows, `~/.config/djinn/config.toml` elsewhere.
fn default_path() -> Option<PathBuf> {
    if cfg!(windows) {
        let exe = std::env::current_exe().ok()?;
        Some(exe.parent()?.join("config.toml"))
    } else {
        Some(dirs::config_dir()?.join("djinn").join("config.toml"))
    }
}

#[cfg(test)]
// figment::Jail closures must return figment's large error type.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use figment::Jail;

    // Every test that reads env goes through Jail, which serializes them so
    // one test's DJINN_* vars can't leak into another.
    #[test]
    fn defaults_are_valid() {
        Jail::expect_with(|_| {
            let config = Config::from_figment(figment(None)).unwrap();
            assert_eq!(config.server.listen.port(), 8066);
            assert_eq!(config.audio.master_rate, 48_000);
            assert_eq!(config.audio.crossfade_secs, 0);
            Ok(())
        });
    }

    #[test]
    fn file_then_env_override() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                r#"
                [server]
                listen = "127.0.0.1:9000"

                [library]
                roots = ["/srv/music"]

                [audio]
                crossfade_secs = 4
                "#,
            )?;
            jail.set_env("DJINN_AUDIO__CROSSFADE_SECS", "6");

            let config = Config::from_figment(figment(Some(Path::new("config.toml")))).unwrap();
            assert_eq!(config.server.listen, "127.0.0.1:9000".parse().unwrap());
            assert_eq!(config.library.roots, vec![PathBuf::from("/srv/music")]);
            assert_eq!(config.audio.crossfade_secs, 6);
            // Untouched sections keep their defaults.
            assert_eq!(config.database.path, PathBuf::from("djinn.db"));
            Ok(())
        });
    }

    #[test]
    fn example_config_is_valid() {
        Jail::expect_with(|jail| {
            jail.create_file("config.toml", include_str!("../../../config.example.toml"))?;
            let config = Config::from_figment(figment(Some(Path::new("config.toml")))).unwrap();
            assert_eq!(config.library.roots, vec![PathBuf::from("/srv/music")]);
            Ok(())
        });
    }

    #[test]
    fn rejects_out_of_range_crossfade() {
        Jail::expect_with(|jail| {
            jail.set_env("DJINN_AUDIO__CROSSFADE_SECS", "13");
            assert!(Config::from_figment(figment(None)).is_err());
            Ok(())
        });
    }

    #[test]
    fn rejects_out_of_range_master_rate() {
        Jail::expect_with(|jail| {
            jail.set_env("DJINN_AUDIO__MASTER_RATE", "1000");
            assert!(Config::from_figment(figment(None)).is_err());
            Ok(())
        });
    }

    #[test]
    fn relative_paths_resolve_against_config_dir() {
        let mut config = Config::default();
        config.library.roots = vec![PathBuf::from("music"), PathBuf::from("/abs/music")];
        config.resolve_relative_paths(Path::new("/etc/djinn"));
        assert_eq!(config.database.path, PathBuf::from("/etc/djinn/djinn.db"));
        assert_eq!(
            config.library.roots,
            vec![
                PathBuf::from("/etc/djinn/music"),
                PathBuf::from("/abs/music")
            ]
        );
    }

    #[test]
    fn missing_explicit_file_is_an_error() {
        assert!(Config::load(Some(Path::new("does/not/exist.toml"))).is_err());
    }
}
