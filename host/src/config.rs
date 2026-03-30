use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Returns the path to the Omni config file: %APPDATA%\Omni\config.json
pub fn config_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("Omni").join("config.json")
}

/// Returns the Omni data directory: %APPDATA%\Omni\
pub fn data_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("Omni")
}

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name of the active overlay folder (under overlays/).
    pub active_overlay: String,
    /// Map game exe names to overlay folder names.
    /// e.g., {"valorant.exe": "Valorant Competitive"}
    pub overlay_by_game: HashMap<String, String>,
    /// Keybinds for overlay control.
    pub keybinds: KeybindConfig,
    /// Process names that should never be injected.
    pub exclude: Vec<String>,
    /// Process names that should always be injected (overrides heuristics).
    pub include: Vec<String>,
    /// Directory path prefixes recognised as game installation roots.
    pub game_directories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    /// Key to toggle overlay visibility.
    pub toggle_overlay: String,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        Self {
            toggle_overlay: "F12".to_string(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_overlay: "Default".to_string(),
            overlay_by_game: HashMap::new(),
            keybinds: KeybindConfig::default(),
            include: Vec::new(),
            game_directories: default_game_directories(),
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
                // NVIDIA / GPU tools
                "nvcontainer.exe".to_string(),
                "nvdisplay.container.exe".to_string(),
                "nvidia overlay.exe".to_string(),
                "nvidia share.exe".to_string(),
                "nvoawrappercache.exe".to_string(),
                "nvsphelper64.exe".to_string(),
                "nvspcaps64.exe".to_string(),
                // AMD
                "amdow.exe".to_string(),
                "amddvr.exe".to_string(),
                "radeonoverlay.exe".to_string(),
                // Windows shell / UWP
                "applicationframehost.exe".to_string(),
                "textinputhost.exe".to_string(),
                "shellexperiencehost.exe".to_string(),
                "startmenuexperiencehost.exe".to_string(),
                "systemsettings.exe".to_string(),
                "widgets.exe".to_string(),
                "windowsterminal.exe".to_string(),
                "lockapp.exe".to_string(),
                "gamebar.exe".to_string(),
                "gamebarpresencewriter.exe".to_string(),
                "gamebarftserver.exe".to_string(),
                // Game launchers / storefronts
                "epicgameslauncher.exe".to_string(),
                "eadesktop.exe".to_string(),
                "origin.exe".to_string(),
                "galaxyclient.exe".to_string(),
                "gogalaxy.exe".to_string(),
                "upc.exe".to_string(),
                "battlenet.exe".to_string(),
                // Overlay / monitoring tools
                "msiafterburner.exe".to_string(),
                "rtss.exe".to_string(),
                "hwinfo64.exe".to_string(),
                "hwinfo32.exe".to_string(),
                // Self
                "omni-host.exe".to_string(),
            ],
        }
    }
}

/// Returns the default list of directories where games are commonly installed.
/// Paths are lowercased for case-insensitive comparison.
fn default_game_directories() -> Vec<String> {
    let mut dirs = vec![
        // Steam
        r"steamapps\common\".to_string(),
        // Epic Games
        r"epic games\".to_string(),
        // GOG
        r"gog galaxy\games\".to_string(),
        r"gog games\".to_string(),
        // EA / Origin
        r"ea games\".to_string(),
        r"origin games\".to_string(),
        // Ubisoft
        r"ubisoft game launcher\games\".to_string(),
        r"ubisoft\".to_string(),
        // Riot
        r"riot games\".to_string(),
        // Battle.net
        r"battle.net\".to_string(),
        // Xbox / Microsoft
        r"xboxgames\".to_string(),
        // General
        r"program files\games\".to_string(),
        r"program files (x86)\games\".to_string(),
    ];

    // Detect Steam library folders from the default Steam install.
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        let libraryfolders = PathBuf::from(&program_files_x86)
            .join(r"Steam\steamapps\libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&libraryfolders) {
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(path) = trimmed.strip_prefix("\"path\"") {
                    let path = path.trim().trim_matches('"').trim();
                    if !path.is_empty() {
                        let mut steam_lib = path.replace("\\\\", "\\").to_lowercase();
                        if !steam_lib.ends_with('\\') {
                            steam_lib.push('\\');
                        }
                        steam_lib.push_str(r"steamapps\common\");
                        dirs.push(steam_lib);
                    }
                }
            }
        }
    }

    dirs
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
    fn default_active_overlay_is_default() {
        let cfg = Config::default();
        assert_eq!(cfg.active_overlay, "Default");
    }

    #[test]
    fn round_trip_through_json() {
        let original = Config::default();
        let json = serde_json::to_string_pretty(&original).expect("serialize");
        let restored: Config = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original.active_overlay, restored.active_overlay);
        assert_eq!(original.exclude, restored.exclude);
        assert_eq!(original.keybinds.toggle_overlay, restored.keybinds.toggle_overlay);
    }

    #[test]
    fn deserialize_with_missing_fields_uses_defaults() {
        // Supplying only one field — the other should fall back to its Default.
        let json = r#"{ "active_overlay": "Custom" }"#;
        let cfg: Config = serde_json::from_str(json).expect("deserialize partial");
        assert_eq!(cfg.active_overlay, "Custom");
        // exclude should be the default list, not empty
        assert!(!cfg.exclude.is_empty());
        // keybinds should default
        assert_eq!(cfg.keybinds.toggle_overlay, "F12");
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
        assert_eq!(cfg.active_overlay, Config::default().active_overlay);
        assert_eq!(cfg.exclude, Config::default().exclude);

        // Cleanup the file that load_config may have created.
        fs::remove_file(&path).ok();
        fs::remove_dir(&dir).ok();
    }
}
