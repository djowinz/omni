//! Workspace folder structure management.
//!
//! Handles creation of the overlay workspace at %APPDATA%\Omni/,
//! migration from the old flat layout, and path resolution.

use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Initialize the workspace folder structure.
/// Creates overlays/, themes/, and Default overlay if they don't exist.
/// Migrates old flat overlay.omni if found.
pub fn init_workspace(data_dir: &Path) {
    let overlays_dir = data_dir.join("overlays");
    let themes_dir = data_dir.join("themes");
    let default_dir = overlays_dir.join("Default");

    // Create directories
    fs::create_dir_all(&default_dir).ok();
    fs::create_dir_all(&themes_dir).ok();

    // Migrate old flat overlay.omni → overlays/Default/overlay.omni
    let old_flat = data_dir.join("overlay.omni");
    let new_default = default_dir.join("overlay.omni");

    if old_flat.exists() && !new_default.exists() {
        match fs::rename(&old_flat, &new_default) {
            Ok(()) => info!(
                from = %old_flat.display(),
                to = %new_default.display(),
                "Migrated overlay.omni to overlays/Default/"
            ),
            Err(e) => {
                warn!(error = %e, "Failed to migrate overlay.omni, copying instead");
                let _ = fs::copy(&old_flat, &new_default);
            }
        }
    }

    // Create default overlay.omni if it doesn't exist
    if !new_default.exists() {
        let default_content = crate::omni::default::DEFAULT_OMNI;
        if let Err(e) = fs::write(&new_default, default_content) {
            warn!(error = %e, "Failed to write default overlay.omni");
        } else {
            info!("Created default overlay at {}", new_default.display());
        }
    }

    // Create default theme if it doesn't exist
    let default_theme = themes_dir.join("dark.css");
    if !default_theme.exists() {
        let theme_content = crate::omni::default::DEFAULT_THEME_CSS;
        if let Err(e) = fs::write(&default_theme, theme_content) {
            warn!(error = %e, "Failed to write default theme");
        } else {
            info!("Created default theme at {}", default_theme.display());
        }
    }
}

/// Get the path to an overlay folder.
pub fn overlay_dir(data_dir: &Path, overlay_name: &str) -> PathBuf {
    data_dir.join("overlays").join(overlay_name)
}

/// Get the path to the .omni file for an overlay.
pub fn overlay_omni_path(data_dir: &Path, overlay_name: &str) -> PathBuf {
    overlay_dir(data_dir, overlay_name).join("overlay.omni")
}

/// Resolve a theme file path.
/// Looks in the overlay's folder first, then the shared themes/ folder.
pub fn resolve_theme_path(data_dir: &Path, overlay_name: &str, theme_src: &str) -> Option<PathBuf> {
    // 1. Check overlay's own folder
    let local = overlay_dir(data_dir, overlay_name).join(theme_src);
    if local.exists() {
        return Some(local);
    }

    // 2. Check shared themes/ folder
    let shared = data_dir.join("themes").join(theme_src);
    if shared.exists() {
        return Some(shared);
    }

    None
}

/// List all overlay folder names.
pub fn list_overlays(data_dir: &Path) -> Vec<String> {
    let overlays_dir = data_dir.join("overlays");
    let mut names = Vec::new();

    if let Ok(entries) = fs::read_dir(&overlays_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
    }

    names.sort();
    names
}

/// List all shared theme file names.
pub fn list_themes(data_dir: &Path) -> Vec<String> {
    let themes_dir = data_dir.join("themes");
    let mut names = Vec::new();

    if let Ok(entries) = fs::read_dir(&themes_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".css") {
                    names.push(name.to_string());
                }
            }
        }
    }

    names.sort();
    names
}

/// List font filenames in the overlay's `fonts/` subdirectory.
/// Returns only files with known font extensions (ttf/otf/woff/woff2), sorted.
pub fn list_overlay_fonts(data_dir: &Path, overlay_name: &str) -> Vec<String> {
    list_overlay_assets(
        data_dir,
        overlay_name,
        "fonts",
        &["ttf", "otf", "woff", "woff2"],
    )
}

/// List image filenames in the overlay's `images/` subdirectory.
/// Returns only files with known image extensions (png/jpg/jpeg/webp/gif/svg), sorted.
pub fn list_overlay_images(data_dir: &Path, overlay_name: &str) -> Vec<String> {
    list_overlay_assets(
        data_dir,
        overlay_name,
        "images",
        &["png", "jpg", "jpeg", "webp", "gif", "svg"],
    )
}

fn list_overlay_assets(
    data_dir: &Path,
    overlay_name: &str,
    subdir: &str,
    exts: &[&str],
) -> Vec<String> {
    let dir = overlay_dir(data_dir, overlay_name).join(subdir);
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            let ext_ok = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .is_some_and(|e| exts.iter().any(|x| *x == e));
            if ext_ok {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names
}

/// Validate that a relative path doesn't escape the data directory.
/// Rejects traversal, absolute paths, null bytes, and Windows alternate data streams.
pub fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Path must not be empty".to_string());
    }
    if path.contains('\0') {
        return Err("Path contains null byte".to_string());
    }
    if path.contains(':') {
        return Err("Path contains invalid character ':'".to_string());
    }
    if path.contains("..") {
        return Err("Path traversal not allowed".to_string());
    }
    if Path::new(path).is_absolute() {
        return Err("Absolute paths not allowed".to_string());
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err("Path must be relative".to_string());
    }
    Ok(())
}

/// Validate a name for use as an overlay folder or theme file name.
/// Rejects names containing path separators, traversal, null bytes, and colons.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name must not be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Name must not contain path separators".to_string());
    }
    if name.contains("..") {
        return Err("Name must not contain '..'".to_string());
    }
    if name.contains('\0') {
        return Err("Name contains null byte".to_string());
    }
    if name.contains(':') {
        return Err("Name contains invalid character ':'".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("omni_test_ws_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).ok();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn init_workspace_creates_dirs() {
        let dir = temp_dir();
        init_workspace(&dir);

        assert!(dir.join("overlays").exists());
        assert!(dir.join("overlays/Default").exists());
        assert!(dir.join("themes").exists());
        assert!(dir.join("overlays/Default/overlay.omni").exists());
        assert!(dir.join("themes/dark.css").exists());

        cleanup(&dir);
    }

    #[test]
    fn migrate_flat_overlay() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("overlay.omni"), "old content").ok();

        init_workspace(&dir);

        // Old file should be moved
        assert!(!dir.join("overlay.omni").exists());
        assert!(dir.join("overlays/Default/overlay.omni").exists());
        let content = fs::read_to_string(dir.join("overlays/Default/overlay.omni")).unwrap();
        assert_eq!(content, "old content");

        cleanup(&dir);
    }

    #[test]
    fn resolve_theme_local_first() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/Test")).ok();
        fs::create_dir_all(dir.join("themes")).ok();
        fs::write(dir.join("overlays/Test/dark.css"), "local").ok();
        fs::write(dir.join("themes/dark.css"), "shared").ok();

        let path = resolve_theme_path(&dir, "Test", "dark.css");
        assert_eq!(path.unwrap(), dir.join("overlays/Test/dark.css"));

        cleanup(&dir);
    }

    #[test]
    fn resolve_theme_falls_back_to_shared() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/Test")).ok();
        fs::create_dir_all(dir.join("themes")).ok();
        fs::write(dir.join("themes/dark.css"), "shared").ok();

        let path = resolve_theme_path(&dir, "Test", "dark.css");
        assert_eq!(path.unwrap(), dir.join("themes/dark.css"));

        cleanup(&dir);
    }

    #[test]
    fn list_overlays_returns_folders() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/Alpha")).ok();
        fs::create_dir_all(dir.join("overlays/Beta")).ok();
        // Create a file (should be ignored, only folders)
        fs::write(dir.join("overlays/not-a-folder.txt"), "").ok();

        let names = list_overlays(&dir);
        assert_eq!(names, vec!["Alpha", "Beta"]);

        cleanup(&dir);
    }

    #[test]
    fn list_overlay_fonts_returns_supported_extensions() {
        let dir = temp_dir();
        let overlay = dir.join("overlays/Default");
        fs::create_dir_all(overlay.join("fonts")).ok();
        fs::write(overlay.join("fonts/SpaceMono.ttf"), b"").ok();
        fs::write(overlay.join("fonts/Body.woff2"), b"").ok();
        fs::write(overlay.join("fonts/readme.txt"), b"").ok();

        let fonts = list_overlay_fonts(&dir, "Default");
        assert_eq!(fonts, vec!["Body.woff2", "SpaceMono.ttf"]);
        cleanup(&dir);
    }

    #[test]
    fn list_overlay_images_returns_supported_extensions() {
        let dir = temp_dir();
        let overlay = dir.join("overlays/Default");
        fs::create_dir_all(overlay.join("images")).ok();
        fs::write(overlay.join("images/logo.png"), b"").ok();
        fs::write(overlay.join("images/bg.jpg"), b"").ok();
        fs::write(overlay.join("images/icon.webp"), b"").ok();
        fs::write(overlay.join("images/notes.txt"), b"").ok();

        let images = list_overlay_images(&dir, "Default");
        assert_eq!(images, vec!["bg.jpg", "icon.webp", "logo.png"]);
        cleanup(&dir);
    }

    #[test]
    fn list_overlay_fonts_empty_when_missing() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/Default")).ok();
        assert!(list_overlay_fonts(&dir, "Default").is_empty());
        cleanup(&dir);
    }

    #[test]
    fn validate_path_rejects_traversal() {
        assert!(validate_relative_path("overlays/Default/overlay.omni").is_ok());
        assert!(validate_relative_path("themes/dark.css").is_ok());
        assert!(validate_relative_path("../../../etc/passwd").is_err());
        assert!(validate_relative_path("C:\\Windows\\System32").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
    }
}
