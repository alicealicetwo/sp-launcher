//! Persistent launcher settings.
//!
//! Stored as JSON next to the app's config directory, written atomically
//! (temp file + rename) so a crash mid-write cannot leave a truncated file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{LauncherError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where the game is installed. Empty until the user picks a folder.
    pub install_dir: String,
    /// Base URL the launcher downloads game files from.
    pub manifest_url: String,
    /// URL the launcher fetches the news feed (`Vec<news::NewsItem>` JSON) from.
    pub news_url: String,
    /// Extra command-line arguments, one per line as the user typed them.
    pub launch_args: String,

    /// Point the game's hostnames at `backend_ip` while the launcher runs.
    pub hosts_redirect: bool,
    /// What the hostnames below should resolve to.
    pub backend_ip: String,
    /// Hostnames the game talks to.
    pub hosts_domains: Vec<String>,
    /// Send the launcher to the tray when the game launches, instead of
    /// staying visible with the "Close Game" button.
    pub close_on_launch: bool,
    pub auto_update: bool,
    pub verify_before_launch: bool,
    pub debug_logging: bool,
    /// Files transferred at once. 1-16.
    pub download_threads: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            install_dir: String::new(),
            manifest_url: "https://example.invalid/manifest.json".into(),
            news_url: "https://example.invalid/news.json".into(),
            launch_args: "-IgnoreCatalogue".into(),
            hosts_redirect: true,
            backend_ip: "127.0.0.1".into(),
            hosts_domains: vec![
                "game.bravohotel.io".into(),
                "ui-lobby.bravohotel.io".into(),
                "game-public-dev2-ap-northeast-2.bravohotel.io".into(),
                "game-private-dev.bravohotel.io".into(),
            ],
            close_on_launch: false,
            auto_update: true,
            verify_before_launch: false,
            debug_logging: false,
            download_threads: 4,
        }
    }
}

pub fn config_path(base: &Path) -> PathBuf {
    base.join("config.v1.json")
}

pub fn load(base: &Path) -> Config {
    let path = config_path(base);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            // A corrupt file must not brick the launcher: keep a copy and
            // start fresh rather than refusing to open.
            let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
            eprintln!("[config] unreadable ({e}), starting from defaults");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

pub fn save(base: &Path, cfg: &Config) -> Result<()> {
    std::fs::create_dir_all(base)?;
    let path = config_path(base);
    let tmp = path.with_extension("json.tmp");

    let text = serde_json::to_string_pretty(cfg)
        .map_err(|e| LauncherError::Config(e.to_string()))?;

    std::fs::write(&tmp, text)?;
    // rename is atomic on the same volume, so readers never see a partial file
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
