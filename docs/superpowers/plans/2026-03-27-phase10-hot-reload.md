# Phase 10: Hot-Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add file watching with debounced hot-reload so .omni, theme CSS, and config.json changes are automatically picked up without restarting the host.

**Architecture:** A `notify` crate file watcher runs on a background thread, sending debounced change events through an `mpsc` channel to the main 120Hz loop. The main loop checks for events each frame, re-parses/re-resolves as needed, and keeps the previous overlay on parse errors. Watch paths update dynamically when the active overlay changes (game-specific switching or config edit).

**Tech Stack:** `notify 7` crate (cross-platform file watching), existing `mpsc` channels, existing parser/resolver pipeline.

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `host/src/watcher.rs` | File watcher setup, debounce logic, event channel types |
| Modify | `host/src/main.rs` | Integrate watcher into main loop, handle reload events |
| Modify | `host/src/scanner.rs` | Expose detected game exe name for overlay re-resolution |
| Modify | `host/Cargo.toml` | Add `notify` dependency |

---

### Task 1: Add `notify` dependency and create watcher module with event types

**Files:**
- Modify: `host/Cargo.toml`
- Create: `host/src/watcher.rs`
- Modify: `host/src/main.rs` (add `mod watcher;`)

- [ ] **Step 1: Add `notify` to Cargo.toml**

In `host/Cargo.toml`, add to the `[dependencies]` section:

```toml
notify = "7"
```

- [ ] **Step 2: Run `cargo check` to verify dependency resolves**

Run: `cd host && cargo check`
Expected: Compiles successfully with notify downloaded.

- [ ] **Step 3: Create `host/src/watcher.rs` with event types and constructor**

```rust
//! File watcher for hot-reload.
//!
//! Watches the active overlay folder, shared themes/, and config.json.
//! Sends debounced `ReloadEvent` variants to the main loop via mpsc.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind};
use tracing::{debug, error, info, warn};

/// What kind of reload the main loop should perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadEvent {
    /// An .omni file or local CSS in the active overlay folder changed.
    Overlay,
    /// A shared theme CSS file changed.
    Theme,
    /// config.json changed.
    Config,
}

const DEBOUNCE_MS: u64 = 500;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    pub rx: mpsc::Receiver<ReloadEvent>,
}

impl FileWatcher {
    /// Start watching the given paths. Returns the watcher handle and a
    /// receiver for debounced reload events.
    ///
    /// # Arguments
    /// * `overlay_dir` - Path to the active overlay folder (e.g., `%APPDATA%/Omni/overlays/Default`)
    /// * `themes_dir` - Path to the shared themes folder (e.g., `%APPDATA%/Omni/themes`)
    /// * `config_path` - Path to config.json
    pub fn start(
        overlay_dir: &Path,
        themes_dir: &Path,
        config_path: &Path,
    ) -> Result<Self, String> {
        let (event_tx, event_rx) = mpsc::channel::<ReloadEvent>();

        // Clone paths for the closure.
        let overlay_dir_owned = overlay_dir.to_path_buf();
        let themes_dir_owned = themes_dir.to_path_buf();
        let config_path_owned = config_path.to_path_buf();

        // Debounce state: track last event time per category.
        // We use a thread to receive raw notify events, debounce, and forward.
        let (raw_tx, raw_rx) = mpsc::channel::<ReloadEvent>();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "File watcher error");
                    return;
                }
            };

            // Only react to content changes (create, modify, remove).
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
                _ => return,
            }

            for path in &event.paths {
                let reload_kind = classify_path(
                    path,
                    &overlay_dir_owned,
                    &themes_dir_owned,
                    &config_path_owned,
                );
                if let Some(kind) = reload_kind {
                    debug!(path = %path.display(), kind = ?kind, "File change detected");
                    let _ = raw_tx.send(kind);
                }
            }
        }).map_err(|e| format!("Failed to create file watcher: {e}"))?;

        // Watch overlay dir (recursive — catches overlay.omni and local CSS).
        if overlay_dir.exists() {
            watcher.watch(overlay_dir, RecursiveMode::Recursive)
                .map_err(|e| format!("Failed to watch overlay dir: {e}"))?;
            info!(path = %overlay_dir.display(), "Watching overlay folder");
        }

        // Watch themes dir (recursive).
        if themes_dir.exists() {
            watcher.watch(themes_dir, RecursiveMode::Recursive)
                .map_err(|e| format!("Failed to watch themes dir: {e}"))?;
            info!(path = %themes_dir.display(), "Watching themes folder");
        }

        // Watch config.json (its parent directory, non-recursive).
        if let Some(config_parent) = config_path.parent() {
            if config_parent.exists() {
                watcher.watch(config_parent, RecursiveMode::NonRecursive)
                    .map_err(|e| format!("Failed to watch config dir: {e}"))?;
                info!(path = %config_path.display(), "Watching config file");
            }
        }

        // Spawn debounce thread.
        std::thread::Builder::new()
            .name("file-watcher-debounce".into())
            .spawn(move || {
                debounce_loop(raw_rx, event_tx);
            })
            .map_err(|e| format!("Failed to spawn debounce thread: {e}"))?;

        Ok(Self {
            _watcher: watcher,
            rx: event_rx,
        })
    }

    /// Update watched paths when the active overlay changes.
    /// Unwatches old overlay dir, watches new one.
    pub fn update_overlay_dir(&mut self, old_dir: &Path, new_dir: &Path) -> Result<(), String> {
        // Unwatch old.
        if old_dir.exists() {
            let _ = self._watcher.unwatch(old_dir);
        }

        // Watch new.
        if new_dir.exists() {
            self._watcher.watch(new_dir, RecursiveMode::Recursive)
                .map_err(|e| format!("Failed to watch new overlay dir: {e}"))?;
            info!(path = %new_dir.display(), "Switched overlay watch to new folder");
        }

        Ok(())
    }
}

/// Classify a changed file path into a reload event kind.
fn classify_path(
    path: &Path,
    overlay_dir: &Path,
    themes_dir: &Path,
    config_path: &Path,
) -> Option<ReloadEvent> {
    // Canonicalize for reliable prefix matching on Windows.
    // Fall back to raw comparison if canonicalize fails.
    let path_canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let overlay_canonical = overlay_dir.canonicalize().unwrap_or_else(|_| overlay_dir.to_path_buf());
    let themes_canonical = themes_dir.canonicalize().unwrap_or_else(|_| themes_dir.to_path_buf());
    let config_canonical = config_path.canonicalize().unwrap_or_else(|_| config_path.to_path_buf());

    if path_canonical == config_canonical {
        return Some(ReloadEvent::Config);
    }

    if path_canonical.starts_with(&themes_canonical) {
        // Only care about .css files in themes.
        if path_canonical.extension().and_then(|e| e.to_str()) == Some("css") {
            return Some(ReloadEvent::Theme);
        }
        return None;
    }

    if path_canonical.starts_with(&overlay_canonical) {
        // Care about .omni and .css files in the overlay folder.
        match path_canonical.extension().and_then(|e| e.to_str()) {
            Some("omni") | Some("css") => return Some(ReloadEvent::Overlay),
            _ => return None,
        }
    }

    None
}

/// Debounce loop: coalesce rapid events into single notifications.
/// Waits until DEBOUNCE_MS has elapsed since the last event of each kind
/// before forwarding.
fn debounce_loop(
    raw_rx: mpsc::Receiver<ReloadEvent>,
    event_tx: mpsc::Sender<ReloadEvent>,
) {
    let mut pending: HashMap<ReloadEvent, Instant> = HashMap::new();
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    let tick = Duration::from_millis(50);

    loop {
        // Drain all available raw events.
        while let Ok(kind) = raw_rx.try_recv() {
            pending.insert(kind, Instant::now());
        }

        // Check which pending events have matured past the debounce window.
        let now = Instant::now();
        let mut matured = Vec::new();
        for (kind, timestamp) in &pending {
            if now.duration_since(*timestamp) >= debounce {
                matured.push(kind.clone());
            }
        }

        for kind in matured {
            pending.remove(&kind);
            if event_tx.send(kind).is_err() {
                // Main thread dropped the receiver — exit.
                return;
            }
        }

        // Sleep briefly, or exit if raw channel is disconnected and nothing pending.
        if pending.is_empty() {
            // Block on next raw event (no busy-wait when idle).
            match raw_rx.recv_timeout(Duration::from_secs(1)) {
                Ok(kind) => {
                    pending.insert(kind, Instant::now());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            std::thread::sleep(tick);
        }
    }
}

// ---------------------------------------------------------------------------
// Hashing for ReloadEvent (needed for HashMap key)
// ---------------------------------------------------------------------------
impl std::hash::Hash for ReloadEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classify_config_path() {
        let overlay = PathBuf::from(r"C:\Users\Test\AppData\Roaming\Omni\overlays\Default");
        let themes = PathBuf::from(r"C:\Users\Test\AppData\Roaming\Omni\themes");
        let config = PathBuf::from(r"C:\Users\Test\AppData\Roaming\Omni\config.json");

        assert_eq!(
            classify_path(&config, &overlay, &themes, &config),
            Some(ReloadEvent::Config)
        );
    }

    #[test]
    fn classify_overlay_omni() {
        let overlay = PathBuf::from(r"C:\Omni\overlays\Default");
        let themes = PathBuf::from(r"C:\Omni\themes");
        let config = PathBuf::from(r"C:\Omni\config.json");

        let changed = PathBuf::from(r"C:\Omni\overlays\Default\overlay.omni");
        assert_eq!(
            classify_path(&changed, &overlay, &themes, &config),
            Some(ReloadEvent::Overlay)
        );
    }

    #[test]
    fn classify_overlay_css() {
        let overlay = PathBuf::from(r"C:\Omni\overlays\Default");
        let themes = PathBuf::from(r"C:\Omni\themes");
        let config = PathBuf::from(r"C:\Omni\config.json");

        let changed = PathBuf::from(r"C:\Omni\overlays\Default\local-theme.css");
        assert_eq!(
            classify_path(&changed, &overlay, &themes, &config),
            Some(ReloadEvent::Overlay)
        );
    }

    #[test]
    fn classify_theme_css() {
        let overlay = PathBuf::from(r"C:\Omni\overlays\Default");
        let themes = PathBuf::from(r"C:\Omni\themes");
        let config = PathBuf::from(r"C:\Omni\config.json");

        let changed = PathBuf::from(r"C:\Omni\themes\dark.css");
        assert_eq!(
            classify_path(&changed, &overlay, &themes, &config),
            Some(ReloadEvent::Theme)
        );
    }

    #[test]
    fn classify_ignores_non_css_in_themes() {
        let overlay = PathBuf::from(r"C:\Omni\overlays\Default");
        let themes = PathBuf::from(r"C:\Omni\themes");
        let config = PathBuf::from(r"C:\Omni\config.json");

        let changed = PathBuf::from(r"C:\Omni\themes\readme.txt");
        assert_eq!(
            classify_path(&changed, &overlay, &themes, &config),
            None
        );
    }

    #[test]
    fn classify_ignores_unrelated_file() {
        let overlay = PathBuf::from(r"C:\Omni\overlays\Default");
        let themes = PathBuf::from(r"C:\Omni\themes");
        let config = PathBuf::from(r"C:\Omni\config.json");

        let changed = PathBuf::from(r"C:\Other\something.txt");
        assert_eq!(
            classify_path(&changed, &overlay, &themes, &config),
            None
        );
    }
}
```

- [ ] **Step 4: Register the watcher module in `main.rs`**

Add `mod watcher;` to the module declarations at the top of `host/src/main.rs`, after the existing `mod workspace;` line:

```rust
mod workspace;
mod watcher;
```

- [ ] **Step 5: Run tests to verify**

Run: `cd host && cargo test watcher`
Expected: All 6 `classify_*` tests pass.

- [ ] **Step 6: Commit**

```bash
git add host/Cargo.toml host/src/watcher.rs host/src/main.rs
git commit -m "feat(host): add file watcher module with notify crate and event classification"
```

---

### Task 2: Expose detected game exe name from Scanner

The main loop needs to know which game exe is currently injected so it can re-resolve the overlay when the scanner detects a new game. Currently, `Scanner` tracks injected PIDs but doesn't expose the exe name.

**Files:**
- Modify: `host/src/scanner.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `host/src/scanner.rs`:

```rust
#[test]
fn last_injected_exe_starts_none() {
    let config = crate::config::Config::default();
    let scanner = Scanner::new("fake.dll".to_string(), config);
    assert_eq!(scanner.last_injected_exe(), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd host && cargo test last_injected_exe_starts_none`
Expected: FAIL — `last_injected_exe` method doesn't exist.

- [ ] **Step 3: Add `last_injected_exe` field and method to Scanner**

In `host/src/scanner.rs`, add a field to the `Scanner` struct:

```rust
pub struct Scanner {
    // ... existing fields ...
    /// The exe name of the most recently injected process.
    last_injected_exe: Option<String>,
}
```

Initialize it in `Scanner::new`:

```rust
last_injected_exe: None,
```

Add the accessor method:

```rust
/// Returns the exe name of the most recently injected game process, if any.
pub fn last_injected_exe(&self) -> Option<&str> {
    self.last_injected_exe.as_deref()
}
```

In the `poll()` method, at the successful injection point (after `self.injected.insert(pid);` around line 199), set the field:

```rust
self.last_injected_exe = Some(exe_name.clone());
```

Also set it in the "already loaded — reconnecting" branch (around line 157):

```rust
self.last_injected_exe = Some(exe_name.clone());
```

- [ ] **Step 4: Run tests to verify**

Run: `cd host && cargo test last_injected_exe`
Expected: PASS

- [ ] **Step 5: Run all scanner tests**

Run: `cd host && cargo test scanner`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add host/src/scanner.rs
git commit -m "feat(host): expose last injected game exe name from Scanner"
```

---

### Task 3: Integrate file watcher into the main loop — overlay reload

Wire up the `FileWatcher` in `run_host()` and handle `ReloadEvent::Overlay` (re-parse .omni + reload local CSS).

**Files:**
- Modify: `host/src/main.rs`

- [ ] **Step 1: Start the file watcher after workspace init**

In `host/src/main.rs` `run_host()`, after the overlay is loaded and the resolver/theme are set up (after line 319), add the watcher initialization:

```rust
    // Start file watcher for hot-reload
    let current_overlay_dir = workspace::structure::overlay_dir(&data_dir, &overlay_name);
    let themes_dir = data_dir.join("themes");
    let mut file_watcher = match watcher::FileWatcher::start(
        &current_overlay_dir,
        &themes_dir,
        &config_path,
    ) {
        Ok(w) => {
            info!("File watcher started for hot-reload");
            Some(w)
        }
        Err(e) => {
            warn!(error = %e, "Failed to start file watcher — hot-reload disabled");
            None
        }
    };

    let mut current_overlay_name = overlay_name;
```

Also change line 261's `let mut scanner_instance` to record the config for later re-loading. We need to make `config` mutable and track the config path:

Before the main loop, replace:

```rust
let mut last_scan = Instant::now();
```

with:

```rust
    let mut config = config;
    let mut last_scan = Instant::now();
    let mut layout_version: u64 = 1;
```

Note: Change the original `let config = config::load_config(&config_path);` (line 211) to `let mut config = config::load_config(&config_path);`.

- [ ] **Step 2: Handle reload events in the main loop**

Inside the `while RUNNING.load(Ordering::Relaxed)` loop, after the WebSocket widget update check (after line 366) and before `let widgets = omni_resolver.resolve(...)`, add:

```rust
        // Handle file watcher events (hot-reload)
        if let Some(ref mut fw) = file_watcher {
            while let Ok(event) = fw.rx.try_recv() {
                match event {
                    watcher::ReloadEvent::Overlay => {
                        info!("Overlay file changed — reloading");
                        let omni_path = workspace::structure::overlay_omni_path(
                            &data_dir, &current_overlay_name,
                        );
                        match std::fs::read_to_string(&omni_path) {
                            Ok(source) => {
                                let (parsed, diagnostics) =
                                    omni::parser::parse_omni_with_diagnostics(&source);
                                for diag in &diagnostics {
                                    match diag.severity {
                                        omni::parser::Severity::Error => error!(
                                            line = diag.line, col = diag.column,
                                            msg = %diag.message,
                                            suggestion = ?diag.suggestion,
                                            "parse error"
                                        ),
                                        omni::parser::Severity::Warning => warn!(
                                            line = diag.line, col = diag.column,
                                            msg = %diag.message,
                                            suggestion = ?diag.suggestion,
                                            "parse warning"
                                        ),
                                    }
                                }
                                if let Some(new_file) = parsed {
                                    // Reload theme if specified
                                    if let Some(theme_src) = &new_file.theme_src {
                                        if let Some(theme_path) =
                                            workspace::structure::resolve_theme_path(
                                                &data_dir,
                                                &current_overlay_name,
                                                theme_src,
                                            )
                                        {
                                            if let Ok(css) = std::fs::read_to_string(&theme_path) {
                                                omni_resolver.load_theme(&css);
                                            }
                                        }
                                    }
                                    info!(
                                        widgets = new_file.widgets.len(),
                                        "Hot-reload successful"
                                    );
                                    omni_file = new_file;
                                    layout_version += 1;
                                } else {
                                    warn!("Parse errors in overlay — keeping previous version");
                                }
                            }
                            Err(e) => warn!(error = %e, "Failed to read overlay file"),
                        }
                    }
                    watcher::ReloadEvent::Theme => {
                        info!("Theme file changed — reloading");
                        if let Some(theme_src) = &omni_file.theme_src {
                            if let Some(theme_path) =
                                workspace::structure::resolve_theme_path(
                                    &data_dir,
                                    &current_overlay_name,
                                    theme_src,
                                )
                            {
                                match std::fs::read_to_string(&theme_path) {
                                    Ok(css) => {
                                        omni_resolver.load_theme(&css);
                                        layout_version += 1;
                                        info!("Theme hot-reload successful");
                                    }
                                    Err(e) => warn!(error = %e, "Failed to read theme file"),
                                }
                            }
                        }
                    }
                    watcher::ReloadEvent::Config => {
                        info!("Config changed — reloading");
                        let new_config = config::load_config(&config_path);

                        // Check if the active overlay changed
                        let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                            scanner_instance.last_injected_exe(),
                            &new_config.overlay_by_game,
                            &new_config.active_overlay,
                            &data_dir,
                        );

                        if new_overlay != current_overlay_name {
                            info!(
                                from = %current_overlay_name,
                                to = %new_overlay,
                                "Active overlay changed — switching"
                            );
                            let old_dir = workspace::structure::overlay_dir(
                                &data_dir, &current_overlay_name,
                            );
                            let new_dir = workspace::structure::overlay_dir(
                                &data_dir, &new_overlay,
                            );

                            // Update watcher paths
                            if let Some(ref mut fw) = file_watcher {
                                if let Err(e) = fw.update_overlay_dir(&old_dir, &new_dir) {
                                    warn!(error = %e, "Failed to update watcher paths");
                                }
                            }

                            // Load the new overlay
                            let omni_path = workspace::structure::overlay_omni_path(
                                &data_dir, &new_overlay,
                            );
                            match std::fs::read_to_string(&omni_path) {
                                Ok(source) => {
                                    let (parsed, diagnostics) =
                                        omni::parser::parse_omni_with_diagnostics(&source);
                                    for diag in &diagnostics {
                                        match diag.severity {
                                            omni::parser::Severity::Error => error!(
                                                line = diag.line, col = diag.column,
                                                msg = %diag.message,
                                                suggestion = ?diag.suggestion,
                                                "parse error"
                                            ),
                                            omni::parser::Severity::Warning => warn!(
                                                line = diag.line, col = diag.column,
                                                msg = %diag.message,
                                                suggestion = ?diag.suggestion,
                                                "parse warning"
                                            ),
                                        }
                                    }
                                    if let Some(new_file) = parsed {
                                        if let Some(theme_src) = &new_file.theme_src {
                                            if let Some(theme_path) =
                                                workspace::structure::resolve_theme_path(
                                                    &data_dir, &new_overlay, theme_src,
                                                )
                                            {
                                                if let Ok(css) =
                                                    std::fs::read_to_string(&theme_path)
                                                {
                                                    omni_resolver.load_theme(&css);
                                                }
                                            }
                                        }
                                        omni_file = new_file;
                                        layout_version += 1;
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to read new overlay file");
                                }
                            }
                            current_overlay_name = new_overlay;
                        }

                        config = new_config;
                    }
                }
            }
        }
```

- [ ] **Step 3: Update the `shm_writer.write` call to use `layout_version`**

Change the existing line:

```rust
shm_writer.write(&latest_snapshot, &widgets, 1);
```

to:

```rust
shm_writer.write(&latest_snapshot, &widgets, layout_version);
```

- [ ] **Step 4: Run `cargo check` to verify compilation**

Run: `cd host && cargo check`
Expected: Compiles successfully.

- [ ] **Step 5: Commit**

```bash
git add host/src/main.rs
git commit -m "feat(host): integrate file watcher for hot-reload of overlays, themes, and config"
```

---

### Task 4: Game-specific overlay switching on scanner detection

When the scanner detects and injects a new game, re-resolve the overlay name. If it changes (due to `overlay_by_game` mapping), switch the active overlay and update the watcher.

**Files:**
- Modify: `host/src/main.rs`

- [ ] **Step 1: Track previous game exe and re-resolve after scanner poll**

In the main loop, after the scanner poll block:

```rust
        if last_scan.elapsed() >= scan_interval {
            scanner_instance.poll();
            last_scan = Instant::now();
```

Add overlay re-resolution after the poll:

```rust
            // Re-resolve overlay based on current game
            let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                scanner_instance.last_injected_exe(),
                &config.overlay_by_game,
                &config.active_overlay,
                &data_dir,
            );

            if new_overlay != current_overlay_name {
                info!(
                    from = %current_overlay_name,
                    to = %new_overlay,
                    game = ?scanner_instance.last_injected_exe(),
                    "Game-specific overlay switch"
                );
                let old_dir = workspace::structure::overlay_dir(
                    &data_dir, &current_overlay_name,
                );
                let new_dir = workspace::structure::overlay_dir(
                    &data_dir, &new_overlay,
                );

                // Update watcher paths
                if let Some(ref mut fw) = file_watcher {
                    if let Err(e) = fw.update_overlay_dir(&old_dir, &new_dir) {
                        warn!(error = %e, "Failed to update watcher paths");
                    }
                }

                // Load the new overlay
                let omni_path = workspace::structure::overlay_omni_path(
                    &data_dir, &new_overlay,
                );
                match std::fs::read_to_string(&omni_path) {
                    Ok(source) => {
                        let (parsed, diagnostics) =
                            omni::parser::parse_omni_with_diagnostics(&source);
                        for diag in &diagnostics {
                            match diag.severity {
                                omni::parser::Severity::Error => error!(
                                    line = diag.line, col = diag.column,
                                    msg = %diag.message,
                                    suggestion = ?diag.suggestion,
                                    "parse error"
                                ),
                                omni::parser::Severity::Warning => warn!(
                                    line = diag.line, col = diag.column,
                                    msg = %diag.message,
                                    suggestion = ?diag.suggestion,
                                    "parse warning"
                                ),
                            }
                        }
                        if let Some(new_file) = parsed {
                            if let Some(theme_src) = &new_file.theme_src {
                                if let Some(theme_path) =
                                    workspace::structure::resolve_theme_path(
                                        &data_dir, &new_overlay, theme_src,
                                    )
                                {
                                    if let Ok(css) = std::fs::read_to_string(&theme_path) {
                                        omni_resolver.load_theme(&css);
                                    }
                                }
                            }
                            omni_file = new_file;
                            layout_version += 1;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to read overlay file for game switch");
                    }
                }
                current_overlay_name = new_overlay;
            }
```

Close the scanner poll `if` block:

```rust
        }
```

- [ ] **Step 2: Run `cargo check`**

Run: `cd host && cargo check`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add host/src/main.rs
git commit -m "feat(host): auto-switch overlay when scanner detects game with overlay_by_game mapping"
```

---

### Task 5: Extract shared reload helper to reduce duplication

Tasks 3 and 4 introduced repeated overlay-loading logic. Extract a helper function in `main.rs` to keep things DRY.

**Files:**
- Modify: `host/src/main.rs`

- [ ] **Step 1: Extract `reload_overlay` helper function**

Add this function above `run_host()` in `host/src/main.rs`:

```rust
/// Attempt to load an overlay from disk and apply it.
/// Returns `true` if the overlay was successfully loaded, `false` on error
/// (in which case the caller should keep the previous overlay).
fn reload_overlay(
    overlay_name: &str,
    data_dir: &Path,
    omni_file: &mut omni::OmniFile,
    omni_resolver: &mut omni::resolver::OmniResolver,
    layout_version: &mut u64,
) -> bool {
    let omni_path = workspace::structure::overlay_omni_path(data_dir, overlay_name);
    let source = match std::fs::read_to_string(&omni_path) {
        Ok(s) => s,
        Err(e) => {
            warn!(path = %omni_path.display(), error = %e, "Failed to read overlay file");
            return false;
        }
    };

    let (parsed, diagnostics) = omni::parser::parse_omni_with_diagnostics(&source);
    for diag in &diagnostics {
        match diag.severity {
            omni::parser::Severity::Error => error!(
                line = diag.line, col = diag.column,
                msg = %diag.message,
                suggestion = ?diag.suggestion,
                "parse error"
            ),
            omni::parser::Severity::Warning => warn!(
                line = diag.line, col = diag.column,
                msg = %diag.message,
                suggestion = ?diag.suggestion,
                "parse warning"
            ),
        }
    }

    match parsed {
        Some(new_file) => {
            if let Some(theme_src) = &new_file.theme_src {
                if let Some(theme_path) =
                    workspace::structure::resolve_theme_path(data_dir, overlay_name, theme_src)
                {
                    if let Ok(css) = std::fs::read_to_string(&theme_path) {
                        omni_resolver.load_theme(&css);
                    }
                }
            }
            info!(widgets = new_file.widgets.len(), "Overlay loaded successfully");
            *omni_file = new_file;
            *layout_version += 1;
            true
        }
        None => {
            warn!("Parse errors in overlay — keeping previous version");
            false
        }
    }
}

/// Switch watch paths and load a new overlay. Returns `true` if the overlay
/// was successfully loaded.
fn switch_overlay(
    old_name: &str,
    new_name: &str,
    data_dir: &Path,
    omni_file: &mut omni::OmniFile,
    omni_resolver: &mut omni::resolver::OmniResolver,
    layout_version: &mut u64,
    file_watcher: &mut Option<watcher::FileWatcher>,
) -> bool {
    let old_dir = workspace::structure::overlay_dir(data_dir, old_name);
    let new_dir = workspace::structure::overlay_dir(data_dir, new_name);

    if let Some(ref mut fw) = file_watcher {
        if let Err(e) = fw.update_overlay_dir(&old_dir, &new_dir) {
            warn!(error = %e, "Failed to update watcher paths");
        }
    }

    reload_overlay(new_name, data_dir, omni_file, omni_resolver, layout_version)
}
```

- [ ] **Step 2: Replace duplicated code in the main loop**

Replace the overlay reload logic in the `ReloadEvent::Overlay` handler with:

```rust
                    watcher::ReloadEvent::Overlay => {
                        info!("Overlay file changed — reloading");
                        reload_overlay(
                            &current_overlay_name,
                            &data_dir,
                            &mut omni_file,
                            &mut omni_resolver,
                            &mut layout_version,
                        );
                    }
```

Replace the theme reload in `ReloadEvent::Theme` handler with:

```rust
                    watcher::ReloadEvent::Theme => {
                        info!("Theme file changed — reloading");
                        if let Some(theme_src) = &omni_file.theme_src {
                            if let Some(theme_path) =
                                workspace::structure::resolve_theme_path(
                                    &data_dir,
                                    &current_overlay_name,
                                    theme_src,
                                )
                            {
                                match std::fs::read_to_string(&theme_path) {
                                    Ok(css) => {
                                        omni_resolver.load_theme(&css);
                                        layout_version += 1;
                                        info!("Theme hot-reload successful");
                                    }
                                    Err(e) => warn!(error = %e, "Failed to read theme file"),
                                }
                            }
                        }
                    }
```

Replace the config handler's overlay switch logic with:

```rust
                        if new_overlay != current_overlay_name {
                            info!(
                                from = %current_overlay_name,
                                to = %new_overlay,
                                "Active overlay changed — switching"
                            );
                            switch_overlay(
                                &current_overlay_name,
                                &new_overlay,
                                &data_dir,
                                &mut omni_file,
                                &mut omni_resolver,
                                &mut layout_version,
                                &mut file_watcher,
                            );
                            current_overlay_name = new_overlay;
                        }
```

Replace the game-detection overlay switch (Task 4) with:

```rust
            if new_overlay != current_overlay_name {
                info!(
                    from = %current_overlay_name,
                    to = %new_overlay,
                    game = ?scanner_instance.last_injected_exe(),
                    "Game-specific overlay switch"
                );
                switch_overlay(
                    &current_overlay_name,
                    &new_overlay,
                    &data_dir,
                    &mut omni_file,
                    &mut omni_resolver,
                    &mut layout_version,
                    &mut file_watcher,
                );
                current_overlay_name = new_overlay;
            }
```

Also replace the initial overlay load (lines 264-319) with:

```rust
    let mut layout_version: u64 = 1;
    let mut omni_resolver = omni::resolver::OmniResolver::new();
    let mut omni_file = omni::OmniFile::empty();

    // Load the initial overlay
    reload_overlay(
        &overlay_name,
        &data_dir,
        &mut omni_file,
        &mut omni_resolver,
        &mut layout_version,
    );
    let mut current_overlay_name = overlay_name;
```

- [ ] **Step 3: Run `cargo check`**

Run: `cd host && cargo check`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add host/src/main.rs
git commit -m "refactor(host): extract reload_overlay and switch_overlay helpers to reduce duplication"
```

---

### Task 6: Integration test — end-to-end hot-reload verification

Write an integration test that creates a temp workspace, starts the file watcher, modifies a file, and verifies the debounced event arrives.

**Files:**
- Modify: `host/src/watcher.rs` (add integration test)

- [ ] **Step 1: Write the integration test**

Add to the `#[cfg(test)] mod tests` block in `host/src/watcher.rs`:

```rust
    #[test]
    fn watcher_detects_overlay_change() {
        use std::fs;
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "omni_watcher_test_{}_{}", std::process::id(), id
        ));
        let overlay_dir = base.join("overlays").join("Test");
        let themes_dir = base.join("themes");
        let config_path = base.join("config.json");

        fs::create_dir_all(&overlay_dir).unwrap();
        fs::create_dir_all(&themes_dir).unwrap();
        fs::write(&config_path, "{}").unwrap();
        fs::write(overlay_dir.join("overlay.omni"), "initial").unwrap();

        let watcher = FileWatcher::start(&overlay_dir, &themes_dir, &config_path)
            .expect("Failed to start watcher");

        // Wait for watcher to initialize.
        std::thread::sleep(Duration::from_millis(100));

        // Modify the overlay file.
        fs::write(overlay_dir.join("overlay.omni"), "modified").unwrap();

        // Wait for debounce (500ms) + margin.
        let event = watcher.rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(event.unwrap(), ReloadEvent::Overlay);

        // Cleanup
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn watcher_detects_theme_change() {
        use std::fs;
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "omni_watcher_theme_test_{}_{}", std::process::id(), id
        ));
        let overlay_dir = base.join("overlays").join("Test");
        let themes_dir = base.join("themes");
        let config_path = base.join("config.json");

        fs::create_dir_all(&overlay_dir).unwrap();
        fs::create_dir_all(&themes_dir).unwrap();
        fs::write(&config_path, "{}").unwrap();
        fs::write(themes_dir.join("dark.css"), "body {}").unwrap();

        let watcher = FileWatcher::start(&overlay_dir, &themes_dir, &config_path)
            .expect("Failed to start watcher");

        std::thread::sleep(Duration::from_millis(100));

        // Modify a theme file.
        fs::write(themes_dir.join("dark.css"), "body { color: red; }").unwrap();

        let event = watcher.rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(event.unwrap(), ReloadEvent::Theme);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn watcher_detects_config_change() {
        use std::fs;
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "omni_watcher_config_test_{}_{}", std::process::id(), id
        ));
        let overlay_dir = base.join("overlays").join("Test");
        let themes_dir = base.join("themes");
        let config_path = base.join("config.json");

        fs::create_dir_all(&overlay_dir).unwrap();
        fs::create_dir_all(&themes_dir).unwrap();
        fs::write(&config_path, "{}").unwrap();

        let watcher = FileWatcher::start(&overlay_dir, &themes_dir, &config_path)
            .expect("Failed to start watcher");

        std::thread::sleep(Duration::from_millis(100));

        // Modify config.
        fs::write(&config_path, r#"{"active_overlay": "Other"}"#).unwrap();

        let event = watcher.rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(event.unwrap(), ReloadEvent::Config);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn watcher_debounces_rapid_changes() {
        use std::fs;
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "omni_watcher_debounce_test_{}_{}", std::process::id(), id
        ));
        let overlay_dir = base.join("overlays").join("Test");
        let themes_dir = base.join("themes");
        let config_path = base.join("config.json");

        fs::create_dir_all(&overlay_dir).unwrap();
        fs::create_dir_all(&themes_dir).unwrap();
        fs::write(&config_path, "{}").unwrap();
        fs::write(overlay_dir.join("overlay.omni"), "v1").unwrap();

        let watcher = FileWatcher::start(&overlay_dir, &themes_dir, &config_path)
            .expect("Failed to start watcher");

        std::thread::sleep(Duration::from_millis(100));

        // Rapid-fire 5 changes in quick succession.
        for i in 0..5 {
            fs::write(overlay_dir.join("overlay.omni"), format!("v{}", i + 2)).unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }

        // Should get exactly one debounced event.
        let event = watcher.rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(event.unwrap(), ReloadEvent::Overlay);

        // No second event within a short window.
        let second = watcher.rx.recv_timeout(Duration::from_millis(300));
        assert!(second.is_err(), "Expected no second event, got: {:?}", second);

        let _ = fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run the integration tests**

Run: `cd host && cargo test watcher -- --test-threads=1`
Expected: All tests pass. (Use `--test-threads=1` since file watcher tests can be timing-sensitive.)

- [ ] **Step 3: Commit**

```bash
git add host/src/watcher.rs
git commit -m "test(host): add integration tests for file watcher hot-reload"
```

---

### Task 7: Full build and manual smoke test

**Files:**
- None (verification only)

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace`
Expected: All tests pass across all crates.

- [ ] **Step 2: Build the project**

Run: `cargo build`
Expected: Clean build with no warnings related to the watcher module.

- [ ] **Step 3: Manual smoke test**

1. Start the host: `cargo run -p omni-host -- --watch target/debug/omni_overlay.dll`
2. Open `%APPDATA%\Omni\overlays\Default\overlay.omni` in a text editor
3. Make a visible change (e.g., change a color value in a widget's CSS)
4. Save the file
5. Observe the host log output — should see "Overlay file changed — reloading" and "Hot-reload successful" within ~1 second
6. The overlay in-game should update without restarting

- [ ] **Step 4: Test error resilience**

1. Introduce a syntax error in overlay.omni (e.g., remove a closing `</widget>` tag)
2. Save the file
3. Observe the host log — should see parse errors and "keeping previous version"
4. Fix the syntax error and save again
5. Overlay should reload successfully

- [ ] **Step 5: Test theme hot-reload**

1. Edit `%APPDATA%\Omni\themes\dark.css`
2. Change a color value
3. Save — host should log "Theme file changed — reloading" and "Theme hot-reload successful"

- [ ] **Step 6: Test config hot-reload**

1. Edit `%APPDATA%\Omni\config.json`
2. Change `active_overlay` to a different overlay name (create the folder first)
3. Save — host should log "Config changed — reloading" and "Active overlay changed — switching"

- [ ] **Step 7: Commit (if any fixes were needed)**

```bash
git add -u
git commit -m "fix(host): address issues found during hot-reload smoke test"
```
