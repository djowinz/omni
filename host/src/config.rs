use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Returns the path to the Omni config file: %APPDATA%\Omni\config.json
pub fn config_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("Omni").join("config.json")
}

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Process names that should never be injected.
    pub exclude: Vec<String>,
    /// How often (in milliseconds) to poll for new game processes.
    pub poll_interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2000,
            exclude: vec![
                "dwm.exe".to_string(),
                "chrome.exe".to_string(),
                "firefox.exe".to_string(),
                "msedge.exe".to_string(),
                "discord.exe".to_string(),
                "steam.exe".to_string(),
                "steamwebhelper.exe".to_string(),
                "explorer.exe".to_string(),
                "code.exe".to_string(),
                "blender.exe".to_string(),
                "svchost.exe".to_string(),
                "lsass.exe".to_string(),
                "csrss.exe".to_string(),
                "winlogon.exe".to_string(),
                "services.exe".to_string(),
                "spoolsv.exe".to_string(),
                "taskmgr.exe".to_string(),
                "conhost.exe".to_string(),
                "cmd.exe".to_string(),
                "powershell.exe".to_string(),
                "pwsh.exe".to_string(),
                "notepad.exe".to_string(),
                "notepad++.exe".to_string(),
                "mspaint.exe".to_string(),
                "calc.exe".to_string(),
                "regedit.exe".to_string(),
                "mmc.exe".to_string(),
                "werfault.exe".to_string(),
                "searchhost.exe".to_string(),
                "runtimebroker.exe".to_string(),
                "sihost.exe".to_string(),
                "fontdrvhost.exe".to_string(),
                "audiodg.exe".to_string(),
                "slack.exe".to_string(),
                "teams.exe".to_string(),
                "outlook.exe".to_string(),
                "winword.exe".to_string(),
                "excel.exe".to_string(),
                "thunderbird.exe".to_string(),
                "spotify.exe".to_string(),
                "zoom.exe".to_string(),
            ],
        }
    }
}

/// Loads config from `path`, returning the default if the file does not exist.
/// If the file is missing it is created with the default config so the user can
/// see and edit it.
pub fn load_config(path: &Path) -> Config {
    if !path.exists() {
        let default = Config::default();
        // Best-effort: create the file so the user knows where to edit it.
        let _ = save_config(path, &default);
        return default;
    }

    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "Failed to read config, using default");
            return Config::default();
        }
    };

    match serde_json::from_str(&text) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "Failed to parse config, using default");
            Config::default()
        }
    }
}

/// Writes `config` as pretty-printed JSON to `path`, creating parent directories
/// if needed.
pub fn save_config(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_has_excludes() {
        let cfg = Config::default();
        assert!(
            !cfg.exclude.is_empty(),
            "Default exclude list should not be empty"
        );
        assert!(
            cfg.exclude.contains(&"chrome.exe".to_string()),
            "Default exclude list should contain chrome.exe"
        );
        assert!(
            cfg.exclude.contains(&"explorer.exe".to_string()),
            "Default exclude list should contain explorer.exe"
        );
    }

    #[test]
    fn default_poll_is_2000ms() {
        let cfg = Config::default();
        assert_eq!(cfg.poll_interval_ms, 2000);
    }

    #[test]
    fn round_trip_through_json() {
        let original = Config::default();
        let json = serde_json::to_string_pretty(&original).expect("serialize");
        let restored: Config = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original.poll_interval_ms, restored.poll_interval_ms);
        assert_eq!(original.exclude, restored.exclude);
    }

    #[test]
    fn deserialize_with_missing_fields_uses_defaults() {
        // Supplying only one field — the other should fall back to its Default.
        let json = r#"{ "poll_interval_ms": 5000 }"#;
        let cfg: Config = serde_json::from_str(json).expect("deserialize partial");
        assert_eq!(cfg.poll_interval_ms, 5000);
        // exclude should be the default list, not empty
        assert!(!cfg.exclude.is_empty());
    }

    #[test]
    fn load_returns_default_for_missing_file() {
        let dir = std::env::temp_dir().join(format!("omni_test_{}", std::process::id()));
        let path = dir.join("config.json");

        // Make sure it does not exist before the test.
        if path.exists() {
            fs::remove_file(&path).ok();
        }
        if dir.exists() {
            fs::remove_dir(&dir).ok();
        }

        let cfg = load_config(&path);
        assert_eq!(cfg.poll_interval_ms, Config::default().poll_interval_ms);
        assert_eq!(cfg.exclude, Config::default().exclude);

        // Cleanup the file that load_config may have created.
        fs::remove_file(&path).ok();
        fs::remove_dir(&dir).ok();
    }
}
