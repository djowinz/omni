# Phase 9a-2a: Workspace + File Management + Config

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure the overlay system into an organized workspace with overlay folders, shared themes, game-specific overlay mapping, and a file management WebSocket API for Electron.

**Architecture:** The `%APPDATA%\Omni/` directory gains an `overlays/` folder (each overlay is a self-contained subfolder) and a `themes/` folder (shared CSS). `config.json` gains `active_overlay`, `overlay_by_game`, and `keybinds` fields. A new `workspace` module manages the folder structure, migration, overlay resolution, and theme loading. The WebSocket server gains file CRUD endpoints and a `widget.apply` endpoint for live preview.

**Tech Stack:** Rust, `serde_json` (config), `tungstenite` (WebSocket), existing `omni` parser.

**Testing notes:** Workspace operations testable with temp directories. Config serialization unit-testable. File API handlers testable without real WebSocket connections. Full pipeline tested manually.

**Depends on:** Phase 9a-1 complete (.omni parser, OmniResolver, WebSocket server).

---

## File Map

```
host/
  src/
    config.rs                        # Update: new fields (active_overlay, overlay_by_game, keybinds)
    main.rs                          # Update: use workspace module for overlay loading
    ws_server.rs                     # Update: add file.* and widget.apply handlers
    workspace/
      mod.rs                         # Public API: init, load_active_overlay, resolve_theme
      structure.rs                   # Folder creation, migration, path resolution
      overlay_resolver.rs            # Game exe → overlay folder resolution chain
      file_api.rs                    # File CRUD operations (list, read, write, create, delete)
    omni/
      default.rs                     # Update: also provide DEFAULT_THEME_CSS constant
```

---

### Task 1: Update Config.json Structure

**Files:**
- Modify: `host/src/config.rs`

- [ ] **Step 1: Add new fields to Config struct**

Update the `Config` struct to include overlay management and keybind fields:

```rust
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
```

Update the `Default` impl for `Config`:

```rust
impl Default for Config {
    fn default() -> Self {
        Self {
            active_overlay: "Default".to_string(),
            overlay_by_game: HashMap::new(),
            keybinds: KeybindConfig::default(),
            include: Vec::new(),
            game_directories: default_game_directories(),
            exclude: vec![
                // ... existing exclude list unchanged ...
            ],
        }
    }
}
```

Remove `poll_interval_ms` from the struct and Default impl (it moves to .omni `<config>` in Phase 9a-2b).

- [ ] **Step 2: Update tests**

Update existing tests to reflect the new fields. The `default_poll_is_2000ms` test should be removed. Add a test for the new fields:

```rust
    #[test]
    fn default_active_overlay_is_default() {
        let cfg = Config::default();
        assert_eq!(cfg.active_overlay, "Default");
        assert!(cfg.overlay_by_game.is_empty());
        assert_eq!(cfg.keybinds.toggle_overlay, "F12");
    }
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- config`
Expected: Tests pass (with the poll_interval_ms test removed).

Note: main.rs will need updating since it references `config.poll_interval_ms`. Temporarily hardcode `Duration::from_millis(2000)` in `run_host` until this is removed. This will be done in the main.rs task.

- [ ] **Step 4: Commit**

```bash
git add host/src/config.rs
git commit -m "feat(host): update config with active_overlay, overlay_by_game, keybinds"
```

---

### Task 2: Workspace Structure Module

**Files:**
- Create: `host/src/workspace/mod.rs`
- Create: `host/src/workspace/structure.rs`
- Modify: `host/src/main.rs` (add `mod workspace;`)

This module handles folder creation, migration from flat layout, and path resolution.

- [ ] **Step 1: Create host/src/workspace/structure.rs**

```rust
//! Workspace folder structure management.
//!
//! Handles creation of the overlay workspace at %APPDATA%\Omni/,
//! migration from the old flat layout, and path resolution.

use std::path::{Path, PathBuf};
use std::fs;
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

/// Validate that a relative path doesn't escape the data directory.
/// Rejects paths with "..", absolute paths, and paths starting with "/" or "\".
pub fn validate_relative_path(path: &str) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omni_test_ws_{}", std::process::id()));
        fs::create_dir_all(&dir).ok();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn init_workspace_creates_dirs() {
        let dir = temp_dir();
        // Provide DEFAULT_OMNI and DEFAULT_THEME_CSS stubs via the module
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
    fn validate_path_rejects_traversal() {
        assert!(validate_relative_path("overlays/Default/overlay.omni").is_ok());
        assert!(validate_relative_path("themes/dark.css").is_ok());
        assert!(validate_relative_path("../../../etc/passwd").is_err());
        assert!(validate_relative_path("C:\\Windows\\System32").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
    }
}
```

- [ ] **Step 2: Create host/src/workspace/mod.rs**

```rust
pub mod structure;
pub mod overlay_resolver;
pub mod file_api;
```

- [ ] **Step 3: Add mod declaration to main.rs**

Add `mod workspace;` after `mod omni;`:

```rust
mod omni;
mod workspace;
```

- [ ] **Step 4: Add DEFAULT_THEME_CSS to omni/default.rs**

Add a default dark theme CSS constant:

```rust
pub const DEFAULT_THEME_CSS: &str = r#":root {
  --bg: rgba(20, 20, 20, 0.7);
  --bg-light: rgba(40, 40, 40, 0.6);
  --text: #ffffff;
  --text-dim: #aaaaaa;
  --accent: #44ff88;
  --warning: #ff8844;
  --critical: #ff4444;
  --font: 'Segoe UI';
  --font-size: 14px;
}
"#;
```

- [ ] **Step 5: Create empty placeholder files**

```bash
echo "" > host/src/workspace/overlay_resolver.rs
echo "" > host/src/workspace/file_api.rs
```

- [ ] **Step 6: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- workspace::structure`
Expected: 6 tests pass.

- [ ] **Step 7: Commit**

```bash
git add host/src/workspace/ host/src/omni/default.rs host/src/main.rs
git commit -m "feat(host): add workspace structure module with folder management and migration"
```

---

### Task 3: Overlay Resolver — Game-Specific Overlay Selection

**Files:**
- Rewrite: `host/src/workspace/overlay_resolver.rs`

Determines which overlay to load based on the running game and config.

- [ ] **Step 1: Create host/src/workspace/overlay_resolver.rs**

```rust
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

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("omni_test_or_{}", std::process::id()));
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

        let result = resolve_overlay_name(
            Some("VALORANT.exe"),
            &by_game,
            "Default",
            &dir,
        );
        assert_eq!(result, "Valorant");

        cleanup(&dir);
    }

    #[test]
    fn falls_back_to_active() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/MySetup")).ok();

        let result = resolve_overlay_name(
            Some("unknowngame.exe"),
            &HashMap::new(),
            "MySetup",
            &dir,
        );
        assert_eq!(result, "MySetup");

        cleanup(&dir);
    }

    #[test]
    fn falls_back_to_default() {
        let dir = temp_dir();

        let result = resolve_overlay_name(
            None,
            &HashMap::new(),
            "NonExistent",
            &dir,
        );
        assert_eq!(result, "Default");

        cleanup(&dir);
    }

    #[test]
    fn game_mapping_case_insensitive() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/CS2")).ok();

        let mut by_game = HashMap::new();
        by_game.insert("cs2.exe".to_string(), "CS2".to_string());

        let result = resolve_overlay_name(
            Some("CS2.EXE"),
            &by_game,
            "Default",
            &dir,
        );
        assert_eq!(result, "CS2");

        cleanup(&dir);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- workspace::overlay_resolver`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/workspace/overlay_resolver.rs
git commit -m "feat(host): add overlay resolver with game-specific mapping"
```

---

### Task 4: File API Module

**Files:**
- Rewrite: `host/src/workspace/file_api.rs`

Handles file CRUD operations for the WebSocket API.

- [ ] **Step 1: Create host/src/workspace/file_api.rs**

```rust
//! File management API for the Electron workspace.
//!
//! All paths are relative to the Omni data directory.
//! Path traversal is rejected for security.

use std::path::Path;
use std::fs;

use serde_json::{json, Value};
use tracing::{info, warn};

use super::structure;

/// Handle a file.list request.
/// Returns: { overlays: ["Default", ...], themes: ["dark.css", ...] }
pub fn handle_list(data_dir: &Path) -> Value {
    let overlays = structure::list_overlays(data_dir);
    let themes = structure::list_themes(data_dir);

    json!({
        "type": "file.list",
        "overlays": overlays,
        "themes": themes,
    })
}

/// Handle a file.read request.
/// Input: { path: "overlays/Default/overlay.omni" }
/// Returns: { content: "..." } or error
pub fn handle_read(data_dir: &Path, relative_path: &str) -> Value {
    if let Err(e) = structure::validate_relative_path(relative_path) {
        return json!({ "type": "error", "message": e });
    }

    let full_path = data_dir.join(relative_path);
    match fs::read_to_string(&full_path) {
        Ok(content) => json!({
            "type": "file.content",
            "path": relative_path,
            "content": content,
        }),
        Err(e) => json!({
            "type": "error",
            "message": format!("Failed to read {}: {}", relative_path, e),
        }),
    }
}

/// Handle a file.write request.
/// Input: { path: "overlays/Default/overlay.omni", content: "..." }
pub fn handle_write(data_dir: &Path, relative_path: &str, content: &str) -> Value {
    if let Err(e) = structure::validate_relative_path(relative_path) {
        return json!({ "type": "error", "message": e });
    }

    let full_path = data_dir.join(relative_path);

    // Ensure parent directory exists
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).ok();
    }

    match fs::write(&full_path, content) {
        Ok(()) => {
            info!(path = relative_path, "File written");
            json!({ "type": "file.written", "path": relative_path })
        }
        Err(e) => json!({
            "type": "error",
            "message": format!("Failed to write {}: {}", relative_path, e),
        }),
    }
}

/// Handle a file.create request.
/// Input: { type: "overlay", name: "My New Overlay" } or { type: "theme", name: "neon.css" }
pub fn handle_create(data_dir: &Path, create_type: &str, name: &str) -> Value {
    match create_type {
        "overlay" => {
            let overlay_dir = data_dir.join("overlays").join(name);
            if overlay_dir.exists() {
                return json!({
                    "type": "error",
                    "message": format!("Overlay '{}' already exists", name),
                });
            }

            if let Err(e) = fs::create_dir_all(&overlay_dir) {
                return json!({
                    "type": "error",
                    "message": format!("Failed to create overlay directory: {}", e),
                });
            }

            // Write starter .omni file
            let starter = format!(r#"<widget id="main" name="{}" enabled="true">
  <template>
    <div style="position: fixed; top: 20px; left: 20px;">
      <span style="color: white; font-size: 16px;">{{fps}} FPS</span>
    </div>
  </template>
  <style></style>
</widget>
"#, name);

            let omni_path = overlay_dir.join("overlay.omni");
            if let Err(e) = fs::write(&omni_path, &starter) {
                return json!({
                    "type": "error",
                    "message": format!("Failed to write starter overlay: {}", e),
                });
            }

            info!(name, "Created new overlay");
            json!({ "type": "file.created", "overlay": name })
        }
        "theme" => {
            let theme_path = data_dir.join("themes").join(name);
            if theme_path.exists() {
                return json!({
                    "type": "error",
                    "message": format!("Theme '{}' already exists", name),
                });
            }

            let starter = ":root {\n  \n}\n";
            if let Err(e) = fs::write(&theme_path, starter) {
                return json!({
                    "type": "error",
                    "message": format!("Failed to create theme: {}", e),
                });
            }

            info!(name, "Created new theme");
            json!({ "type": "file.created", "theme": name })
        }
        _ => json!({
            "type": "error",
            "message": format!("Unknown create type: {}", create_type),
        }),
    }
}

/// Handle a file.delete request.
/// Input: { path: "overlays/My Old Overlay" } or { path: "themes/old.css" }
pub fn handle_delete(data_dir: &Path, relative_path: &str) -> Value {
    if let Err(e) = structure::validate_relative_path(relative_path) {
        return json!({ "type": "error", "message": e });
    }

    // Prevent deleting Default overlay
    if relative_path == "overlays/Default" || relative_path == "overlays\\Default" {
        return json!({
            "type": "error",
            "message": "Cannot delete the Default overlay",
        });
    }

    let full_path = data_dir.join(relative_path);

    if !full_path.exists() {
        return json!({
            "type": "error",
            "message": format!("{} not found", relative_path),
        });
    }

    let result = if full_path.is_dir() {
        fs::remove_dir_all(&full_path)
    } else {
        fs::remove_file(&full_path)
    };

    match result {
        Ok(()) => {
            info!(path = relative_path, "File/folder deleted");
            json!({ "type": "file.deleted", "path": relative_path })
        }
        Err(e) => json!({
            "type": "error",
            "message": format!("Failed to delete {}: {}", relative_path, e),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("omni_test_fa_{}", std::process::id()));
        fs::create_dir_all(dir.join("overlays/Default")).ok();
        fs::create_dir_all(dir.join("themes")).ok();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_returns_overlays_and_themes() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/Gaming")).ok();
        fs::write(dir.join("themes/dark.css"), ":root {}").ok();

        let result = handle_list(&dir);
        let overlays = result["overlays"].as_array().unwrap();
        let themes = result["themes"].as_array().unwrap();

        assert!(overlays.iter().any(|v| v == "Default"));
        assert!(overlays.iter().any(|v| v == "Gaming"));
        assert!(themes.iter().any(|v| v == "dark.css"));

        cleanup(&dir);
    }

    #[test]
    fn read_and_write_file() {
        let dir = temp_dir();
        let path = "overlays/Default/overlay.omni";

        // Write
        let write_result = handle_write(&dir, path, "test content");
        assert_eq!(write_result["type"], "file.written");

        // Read
        let read_result = handle_read(&dir, path);
        assert_eq!(read_result["type"], "file.content");
        assert_eq!(read_result["content"], "test content");

        cleanup(&dir);
    }

    #[test]
    fn read_rejects_path_traversal() {
        let dir = temp_dir();
        let result = handle_read(&dir, "../../../etc/passwd");
        assert_eq!(result["type"], "error");
        assert!(result["message"].as_str().unwrap().contains("traversal"));
    }

    #[test]
    fn create_overlay() {
        let dir = temp_dir();
        let result = handle_create(&dir, "overlay", "My New Overlay");
        assert_eq!(result["type"], "file.created");
        assert!(dir.join("overlays/My New Overlay/overlay.omni").exists());

        cleanup(&dir);
    }

    #[test]
    fn create_theme() {
        let dir = temp_dir();
        let result = handle_create(&dir, "theme", "neon.css");
        assert_eq!(result["type"], "file.created");
        assert!(dir.join("themes/neon.css").exists());

        cleanup(&dir);
    }

    #[test]
    fn delete_overlay() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("overlays/ToDelete")).ok();
        fs::write(dir.join("overlays/ToDelete/overlay.omni"), "").ok();

        let result = handle_delete(&dir, "overlays/ToDelete");
        assert_eq!(result["type"], "file.deleted");
        assert!(!dir.join("overlays/ToDelete").exists());

        cleanup(&dir);
    }

    #[test]
    fn cannot_delete_default() {
        let dir = temp_dir();
        let result = handle_delete(&dir, "overlays/Default");
        assert_eq!(result["type"], "error");
        assert!(dir.join("overlays/Default").exists());

        cleanup(&dir);
    }

    #[test]
    fn create_existing_overlay_errors() {
        let dir = temp_dir();
        let result = handle_create(&dir, "overlay", "Default");
        assert_eq!(result["type"], "error");

        cleanup(&dir);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- workspace::file_api`
Expected: 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/workspace/file_api.rs
git commit -m "feat(host): add file management API (list, read, write, create, delete)"
```

---

### Task 5: Wire WebSocket File Endpoints

**Files:**
- Modify: `host/src/ws_server.rs`

Add `file.list`, `file.read`, `file.write`, `file.create`, `file.delete`, and `widget.apply` to the message handler.

- [ ] **Step 1: Add data_dir to WsSharedState**

The file API needs the data directory path:

```rust
pub struct WsSharedState {
    pub latest_snapshot: Mutex<SensorSnapshot>,
    pub active_omni_file: Mutex<Option<crate::omni::types::OmniFile>>,
    pub data_dir: std::path::PathBuf,
    pub running: AtomicBool,
}

impl WsSharedState {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        Self {
            latest_snapshot: Mutex::new(SensorSnapshot::default()),
            active_omni_file: Mutex::new(None),
            data_dir,
            running: AtomicBool::new(true),
        }
    }
}
```

- [ ] **Step 2: Add file and widget.apply handlers to handle_message**

Add these cases to the `handle_message` match:

```rust
        "file.list" => {
            Some(crate::workspace::file_api::handle_list(&state.data_dir).to_string())
        }
        "file.read" => {
            let path = msg.get("path").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::workspace::file_api::handle_read(&state.data_dir, path).to_string())
        }
        "file.write" => {
            let path = msg.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::workspace::file_api::handle_write(&state.data_dir, path, content).to_string())
        }
        "file.create" => {
            let create_type = msg.get("createType").and_then(|v| v.as_str()).unwrap_or("");
            let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::workspace::file_api::handle_create(&state.data_dir, create_type, name).to_string())
        }
        "file.delete" => {
            let path = msg.get("path").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::workspace::file_api::handle_delete(&state.data_dir, path).to_string())
        }
        "widget.apply" => {
            let source = msg.get("source").and_then(|v| v.as_str()).unwrap_or("");
            match crate::omni::parser::parse_omni(source) {
                Ok(file) => {
                    if let Ok(mut active) = state.active_omni_file.lock() {
                        *active = Some(file.clone());
                    }
                    let json_file = serde_json::to_value(&file).unwrap_or(json!(null));
                    Some(json!({
                        "type": "widget.applied",
                        "file": json_file,
                        "errors": [],
                    }).to_string())
                }
                Err(errors) => {
                    let error_list: Vec<Value> = errors.iter().map(|e| json!({
                        "message": e.message,
                        "offset": e.offset,
                    })).collect();
                    Some(json!({
                        "type": "widget.applied",
                        "file": null,
                        "errors": error_list,
                    }).to_string())
                }
            }
        }
```

- [ ] **Step 3: Update tests for new WsSharedState constructor**

All existing tests that create `WsSharedState::new()` need to pass a `data_dir`. Use a temp directory:

```rust
let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
```

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- ws_server`
Expected: All ws_server tests pass.

- [ ] **Step 5: Commit**

```bash
git add host/src/ws_server.rs
git commit -m "feat(host): add file management and widget.apply WebSocket endpoints"
```

---

### Task 6: Wire Workspace into Main Loop

**Files:**
- Modify: `host/src/main.rs`

Replace the old flat file loading with workspace-based overlay loading.

- [ ] **Step 1: Update run_host to use workspace**

The `run_host` function needs these changes:

1. Initialize workspace on startup
2. Resolve overlay name using config + scanner
3. Load overlay from workspace folder
4. Resolve theme using workspace theme resolution
5. Hardcode poll interval to 2000ms (was `config.poll_interval_ms`, removed in Task 1)
6. Pass `data_dir` to `WsSharedState::new()`

Replace the overlay loading section (from `let omni_path = ...` through the theme loading) with:

```rust
    let data_dir = config::data_dir();
    workspace::structure::init_workspace(&data_dir);

    // Resolve which overlay to load
    let overlay_name = workspace::overlay_resolver::resolve_overlay_name(
        None, // No game running yet — will be updated when scanner detects one
        &config.overlay_by_game,
        &config.active_overlay,
        &data_dir,
    );
    info!(overlay = %overlay_name, "Resolved active overlay");

    // Load the overlay .omni file
    let omni_path = workspace::structure::overlay_omni_path(&data_dir, &overlay_name);
    let omni_source = match std::fs::read_to_string(&omni_path) {
        Ok(s) => {
            info!(path = %omni_path.display(), "Loaded overlay file");
            s
        }
        Err(e) => {
            warn!(path = %omni_path.display(), error = %e, "Failed to read overlay, using default");
            omni::default::DEFAULT_OMNI.to_string()
        }
    };

    let mut omni_file = match omni::parser::parse_omni(&omni_source) {
        Ok(f) => f,
        Err(errs) => {
            warn!(?errs, "Failed to parse overlay, using empty file");
            omni::OmniFile::empty()
        }
    };

    let mut omni_resolver = omni::resolver::OmniResolver::new();

    // Load theme with workspace resolution (local → shared)
    if let Some(theme_src) = &omni_file.theme_src {
        if let Some(theme_path) = workspace::structure::resolve_theme_path(&data_dir, &overlay_name, theme_src) {
            match std::fs::read_to_string(&theme_path) {
                Ok(css) => {
                    info!(path = %theme_path.display(), "Loaded theme CSS");
                    omni_resolver.load_theme(&css);
                }
                Err(e) => warn!(path = %theme_path.display(), error = %e, "Failed to load theme"),
            }
        } else {
            warn!(theme_src, "Theme file not found in overlay folder or shared themes");
        }
    }
```

Also update the `WsSharedState` creation:
```rust
    let ws_state = Arc::new(ws_server::WsSharedState::new(data_dir.clone()));
```

And hardcode the poll interval:
```rust
    let poll_interval = Duration::from_millis(2000);
```

Remove the `config.poll_interval_ms` reference.

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/main.rs
git commit -m "feat(host): wire workspace into main loop with overlay resolution and theme loading"
```

---

### Task 7: Integration Test — Workspace + File API

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Verify workspace initialization**

Start the host in service mode. Check `%APPDATA%\Omni\`:

```bash
cargo run -p omni-host -- --service
```

Expected filesystem:
```
%APPDATA%\Omni\
  config.json          (with active_overlay, overlay_by_game, keybinds)
  overlays/
    Default/
      overlay.omni     (built-in default)
  themes/
    dark.css           (default theme)
```

- [ ] **Step 3: Test file.list via WebSocket**

```javascript
const ws = new WebSocket('ws://localhost:9473');
ws.onmessage = (e) => console.log(JSON.parse(e.data));
ws.onopen = () => ws.send(JSON.stringify({type: 'file.list'}));
```

Expected: `{ type: "file.list", overlays: ["Default"], themes: ["dark.css"] }`

- [ ] **Step 4: Test file.create — new overlay**

```javascript
ws.send(JSON.stringify({type: 'file.create', createType: 'overlay', name: 'My Gaming Setup'}));
```

Then list again — should show both "Default" and "My Gaming Setup".

- [ ] **Step 5: Test file.write + widget.apply — live preview**

```javascript
ws.send(JSON.stringify({
  type: 'widget.apply',
  source: '<widget id="test" name="Test" enabled="true"><template><div style="position: fixed; top: 10px; right: 10px;"><span style="color: #ff4444; font-size: 28px; font-weight: bold;">{fps}</span></div></template><style></style></widget>'
}));
```

The overlay in-game should update to show a red FPS counter.

- [ ] **Step 6: Verify overlay persists after restart**

Create a custom `overlay.omni` in `overlays/Default/`, restart the host, verify it loads.

- [ ] **Step 7: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9a-2a integration test"
```

---

## Phase 9a-2a Complete — Summary

At this point you have:

1. **Organized workspace** — `overlays/` folders + `themes/` for shared CSS
2. **Config.json** — `active_overlay`, `overlay_by_game`, `keybinds`
3. **Overlay resolution chain** — game exe → active → Default
4. **Theme resolution** — local folder → shared themes/
5. **File management API** — list, read, write, create, delete via WebSocket
6. **`widget.apply`** — live preview from raw source (no disk write)
7. **Migration** — old flat `overlay.omni` moved to `overlays/Default/`
8. **Path security** — traversal prevention on all file operations
9. **Default theme** — `dark.css` with standard CSS variables

**Next:** Phase 9a-2b adds CSS cascade (descendant/compound selectors, specificity) and per-sensor poll intervals.
