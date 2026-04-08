//! Resolves which overlay to load based on running games and config.
//!
//! Resolution chain:
//! 1. overlay_by_game — if the running game's exe matches a mapping
//! 2. active_overlay — the user's selected default
//! 3. "Default" — built-in fallback

use std::collections::HashMap;
use std::path::Path;

use super::structure;

/// Resolve the overlay folder name to use.
///
/// # Arguments
/// * `active_game_exe` - The exe name of the currently injected game (if any)
/// * `overlay_by_game` - Map of exe names → overlay folder names
/// * `active_overlay` - The user's configured default overlay
/// * `data_dir` - The Omni data directory
pub fn resolve_overlay_name(
    active_game_exe: Option<&str>,
    overlay_by_game: &HashMap<String, String>,
    active_overlay: &str,
    data_dir: &Path,
) -> String {
    // 1. Check game-specific mapping
    if let Some(exe) = active_game_exe {
        let exe_lower = exe.to_lowercase();
        for (game_exe, overlay_name) in overlay_by_game {
            if game_exe.to_lowercase() == exe_lower {
                // Verify the overlay folder exists
                if structure::overlay_dir(data_dir, overlay_name).exists() {
                    return overlay_name.clone();
                }
            }
        }
    }

    // 2. Use active_overlay if its folder exists
    if structure::overlay_dir(data_dir, active_overlay).exists() {
        return active_overlay.to_string();
    }

    // 3. Fall back to "Default"
    "Default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("omni_test_or_{}_{}", std::process::id(), id));
        fs::create_dir_all(dir.join("overlays/Default")).ok();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn game_specific_overlay() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/Valorant")).ok();

        let mut by_game = HashMap::new();
        by_game.insert("valorant.exe".to_string(), "Valorant".to_string());

        let result = resolve_overlay_name(Some("VALORANT.exe"), &by_game, "Default", &dir);
        assert_eq!(result, "Valorant");

        cleanup(&dir);
    }

    #[test]
    fn falls_back_to_active() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/MySetup")).ok();

        let result =
            resolve_overlay_name(Some("unknowngame.exe"), &HashMap::new(), "MySetup", &dir);
        assert_eq!(result, "MySetup");

        cleanup(&dir);
    }

    #[test]
    fn falls_back_to_default() {
        let dir = temp_dir();

        let result = resolve_overlay_name(None, &HashMap::new(), "NonExistent", &dir);
        assert_eq!(result, "Default");

        cleanup(&dir);
    }

    #[test]
    fn game_mapping_case_insensitive() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/CS2")).ok();

        let mut by_game = HashMap::new();
        by_game.insert("cs2.exe".to_string(), "CS2".to_string());

        let result = resolve_overlay_name(Some("CS2.EXE"), &by_game, "Default", &dir);
        assert_eq!(result, "CS2");

        cleanup(&dir);
    }
}
