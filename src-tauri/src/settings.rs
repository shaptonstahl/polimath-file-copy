//! User settings, stored as JSON in the OS's per-user app config folder:
//! Windows `%APPDATA%\com.polimath.filecopy`, macOS
//! `~/Library/Application Support/com.polimath.filecopy`, Linux
//! `~/.config/com.polimath.filecopy`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Write a log for each Preview and Run.
    pub log_enabled: bool,
    /// Folder for logs. `None` means the OS default (see `lib.rs`).
    pub log_dir: Option<PathBuf>,
}

impl Settings {
    /// Missing or unreadable settings fall back to defaults.
    pub fn load(file: &Path) -> Settings {
        fs::read_to_string(file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Write atomically: a temp file, then rename over the old one.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, file)
    }

    pub fn effective_log_dir(&self, default: &Path) -> PathBuf {
        self.log_dir.clone().unwrap_or_else(|| default.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults_and_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("cfg/settings.json");
        assert_eq!(Settings::load(&f), Settings::default());
        let s = Settings { log_enabled: true, log_dir: Some(d.path().join("logs")) };
        s.save(&f).unwrap();
        assert_eq!(Settings::load(&f), s);
        assert_eq!(s.effective_log_dir(Path::new("/default")), d.path().join("logs"));
        assert_eq!(Settings::default().effective_log_dir(Path::new("/default")), PathBuf::from("/default"));
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("settings.json");
        fs::write(&f, b"{not json").unwrap();
        assert_eq!(Settings::load(&f), Settings::default());
    }
}
