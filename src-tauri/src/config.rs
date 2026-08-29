//! IDIN configuration: categories, download directory, per-task settings.
//!
//! Config is persisted as JSON in the app's config directory.
//! Categories map file extensions → sub-folders inside the download root.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// A single download category (e.g. "Videos", "Music", "Archives").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Category {
    pub name: String,
    /// Sub-folder name inside the download root (e.g. "Videos").
    pub folder: String,
    /// File extensions this category matches (e.g. [".mp4", ".mkv"]).
    /// Extensions are lowercased and include the dot.
    pub extensions: Vec<String>,
}

/// Full application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Root download directory (e.g. `C:\Users\X\Downloads\IDIN`).
    pub download_dir: PathBuf,
    /// Ordered list of categories. First match wins.
    pub categories: Vec<Category>,
    /// Global download speed limit in bytes/sec (0 = unlimited).
    pub global_speed_limit: u64,
    /// When true, clicking X hides the window to tray instead of quitting.
    #[serde(default = "default_close_to_tray")]
    pub close_to_tray: bool,
    /// Unix timestamp (seconds) when queued downloads should start.
    /// `None` means no schedule — downloads start immediately.
    #[serde(default)]
    pub scheduled_start: Option<u64>,
    /// Action to perform after ALL downloads complete.
    /// Values: `null` (none), `"shutdown"`, `"sleep"`, `"hibernate"`.
    #[serde(default)]
    pub post_download_action: Option<String>,
    /// Max simultaneous downloads (0 = unlimited).
    #[serde(default)]
    pub max_concurrent: u64,
    /// Global proxy URL (http:// or socks5://); empty = no proxy.
    #[serde(default)]
    pub proxy_url: String,
}

fn default_close_to_tray() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            categories: default_categories(),
            global_speed_limit: 0,
            close_to_tray: true,
            scheduled_start: None,
            post_download_action: None,
            max_concurrent: 0,
            proxy_url: String::new(),
        }
    }
}

/// Thread-safe shared config handle.
pub type SharedConfig = Arc<RwLock<AppConfig>>;

/// Tauri managed state wrapper for the config.
pub struct ConfigState(pub SharedConfig);

/// Create a new shared config, loading from disk or using defaults.
pub fn load_or_create(config_dir: &std::path::Path) -> SharedConfig {
    let path = config_dir.join("config.json");
    let cfg = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
            .unwrap_or_default()
    } else {
        AppConfig::default()
    };
    // Ensure the download directory exists.
    let _ = std::fs::create_dir_all(&cfg.download_dir);
    Arc::new(RwLock::new(cfg))
}

/// Persist the current config to disk.
pub fn save(config_dir: &std::path::Path, cfg: &AppConfig) -> Result<(), String> {
    let path = config_dir.join("config.json");
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("write config: {e}"))?;
    Ok(())
}

/// Given a filename, return the sub-folder it belongs to based on categories.
/// Returns `None` if no category matches (caller should put it in the root).
pub fn classify_file(cfg: &AppConfig, filename: &str) -> Option<String> {
    let ext = std::path::Path::new(filename)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    let ext = format!(".{ext}");
    for cat in &cfg.categories {
        if cat.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
            return Some(cat.folder.clone());
        }
    }
    None
}

/// Default download dir: `~/Downloads/IDIN`.
fn default_download_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
        .join("IDIN")
}

/// The six built-in categories with sensible extension lists.
pub fn default_categories() -> Vec<Category> {
    vec![
        Category {
            name: "Documents".into(),
            folder: "Docs".into(),
            extensions: vec![
                ".pdf", ".doc", ".docx", ".txt", ".epub", ".mobi", ".odt", ".rtf", ".tex", ".pptx",
                ".xlsx", ".csv", ".md", ".rst",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        Category {
            name: "Videos".into(),
            folder: "Videos".into(),
            extensions: vec![
                ".mp4", ".mkv", ".avi", ".mov", ".wmv", ".flv", ".webm", ".m4v", ".mpg", ".mpeg",
                ".3gp",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        Category {
            name: "Music".into(),
            folder: "Music".into(),
            extensions: vec![
                ".mp3", ".flac", ".wav", ".aac", ".ogg", ".m4a", ".wma", ".opus", ".ape",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        Category {
            name: "Images".into(),
            folder: "Images".into(),
            extensions: vec![
                ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".svg", ".webp", ".ico", ".tiff", ".psd",
                ".raw",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        Category {
            name: "Archives".into(),
            folder: "Archives".into(),
            extensions: vec![
                ".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz", ".iso", ".dmg",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        Category {
            name: "Code".into(),
            folder: "Code".into(),
            extensions: vec![
                ".js", ".ts", ".py", ".rs", ".go", ".java", ".c", ".cpp", ".h", ".css", ".html",
                ".json", ".xml", ".yaml", ".yml", ".toml", ".sh", ".bat",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_video() {
        let cfg = AppConfig::default();
        assert_eq!(classify_file(&cfg, "movie.mp4"), Some("Videos".into()));
    }

    #[test]
    fn classify_pdf() {
        let cfg = AppConfig::default();
        assert_eq!(classify_file(&cfg, "report.pdf"), Some("Docs".into()));
    }

    #[test]
    fn classify_unknown() {
        let cfg = AppConfig::default();
        assert_eq!(classify_file(&cfg, "file.xyz"), None);
    }

    #[test]
    fn classify_case_insensitive() {
        let cfg = AppConfig::default();
        assert_eq!(classify_file(&cfg, "IMAGE.JPG"), Some("Images".into()));
    }

    #[test]
    fn default_categories_count() {
        assert_eq!(default_categories().len(), 6);
    }
}
