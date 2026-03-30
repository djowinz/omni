//! File management API for the Electron workspace.
//!
//! All paths are relative to the Omni data directory.
//! Path traversal is rejected for security.

use std::path::Path;
use std::fs;

use serde_json::{json, Value};
use tracing::info;

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
    if let Err(e) = structure::validate_name(name) {
        return json!({ "type": "error", "message": e });
    }

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

    // Prevent deleting Default overlay (normalize trailing slashes and separators)
    let normalized = relative_path.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    if normalized == "overlays/Default" {
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
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "omni_test_fa_{}_{}", std::process::id(), id
        ));
        let _ = fs::remove_dir_all(&dir);
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
