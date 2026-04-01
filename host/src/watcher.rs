use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Events emitted by the file watcher after debouncing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReloadEvent {
    Overlay,
    Theme,
    Config,
}

/// A file watcher that monitors overlay, theme, and config paths and
/// emits debounced [`ReloadEvent`]s over the `rx` channel.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<ReloadEvent>,
}

impl FileWatcher {
    /// Drain all pending events from the channel.
    pub fn drain_events(&self) -> Vec<ReloadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// Block until an event arrives or the timeout elapses.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<ReloadEvent, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

impl FileWatcher {
    /// Start watching `overlay_dir` (recursive), `themes_dir` (recursive), and the
    /// parent directory of `config_path` (non-recursive).
    ///
    /// Returns `Err` if any path cannot be watched or the watcher cannot be created.
    pub fn start(
        overlay_dir: PathBuf,
        themes_dir: PathBuf,
        config_path: PathBuf,
    ) -> Result<Self, crate::error::HostError> {
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<Event>>();
        let (event_tx, event_rx) = mpsc::channel::<ReloadEvent>();

        let mut watcher =
            RecommendedWatcher::new(raw_tx, notify::Config::default())
                .map_err(|e| format!("Failed to create file watcher: {e}"))?;

        watcher
            .watch(&overlay_dir, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch overlay_dir {}: {e}", overlay_dir.display()))?;

        watcher
            .watch(&themes_dir, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch themes_dir {}: {e}", themes_dir.display()))?;

        let config_parent = config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        watcher
            .watch(&config_parent, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch config parent {}: {e}", config_parent.display()))?;

        // Canonicalize reference paths once so debounce_loop never pays a syscall per event.
        let overlay_dir_c = canon(&overlay_dir);
        let themes_dir_c = canon(&themes_dir);
        let config_path_c = canon(&config_path);

        // Spawn the debounce thread
        std::thread::Builder::new()
            .name("file-watcher-debounce".to_string())
            .spawn(move || {
                debounce_loop(
                    raw_rx,
                    event_tx,
                    overlay_dir_c,
                    themes_dir_c,
                    config_path_c,
                );
            })
            .map_err(|e| format!("Failed to spawn debounce thread: {e}"))?;

        Ok(Self {
            _watcher: watcher,
            rx: event_rx,
        })
    }
}

/// Canonicalize a path, falling back to the raw path on error.
fn canon(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Classify a changed `path` into a [`ReloadEvent`], or `None` if the change is irrelevant.
///
/// `overlay_dir`, `themes_dir`, and `config_path` must already be canonicalized by the caller.
/// Only `path` (the incoming event path) is canonicalized here.
pub fn classify_path(
    path: &Path,
    overlay_dir: &Path,
    themes_dir: &Path,
    config_path: &Path,
) -> Option<ReloadEvent> {
    let path_c = canon(path);

    // Exact match on config path
    if path_c == config_path {
        return Some(ReloadEvent::Config);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    // Theme: .css file inside themes_dir
    if path_c.starts_with(themes_dir) {
        if ext.as_deref() == Some("css") {
            return Some(ReloadEvent::Theme);
        }
        return None; // non-css in themes dir → ignored
    }

    // Overlay: .omni or .css file inside overlay_dir
    if path_c.starts_with(overlay_dir) {
        match ext.as_deref() {
            Some("omni") | Some("css") => return Some(ReloadEvent::Overlay),
            _ => return None,
        }
    }

    None
}

/// Coalesces rapid file-change events per kind, waiting `DEBOUNCE_MS` since the last
/// event of that kind before forwarding it.  When nothing is pending the thread
/// blocks on `recv_timeout(1s)` to avoid busy-waiting.
fn debounce_loop(
    raw_rx: mpsc::Receiver<notify::Result<Event>>,
    event_tx: mpsc::Sender<ReloadEvent>,
    overlay_dir: PathBuf,
    themes_dir: PathBuf,
    config_path: PathBuf,
) {
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    // last time we saw each kind of event
    let mut pending: HashMap<ReloadEvent, Instant> = HashMap::new();

    loop {
        // If nothing is pending, block for up to 1 s; otherwise use a short poll.
        let timeout = if pending.is_empty() {
            Duration::from_secs(1)
        } else {
            Duration::from_millis(10)
        };

        match raw_rx.recv_timeout(timeout) {
            Ok(Ok(event)) => {
                // Only react to create / modify / remove
                let interesting = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                );
                if interesting {
                    for path in &event.paths {
                        if let Some(kind) =
                            classify_path(path, &overlay_dir, &themes_dir, &config_path)
                        {
                            pending.insert(kind, Instant::now());
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                // Watcher error — log and continue
                eprintln!("[file-watcher-debounce] notify error: {e}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Normal — check pending below
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Sender dropped; the watcher is gone — exit thread
                break;
            }
        }

        // Flush any pending events whose debounce window has elapsed
        let now = Instant::now();
        pending.retain(|kind, last_seen| {
            if now.duration_since(*last_seen) >= debounce {
                // Best-effort send; if the receiver is gone we'll catch it on next loop
                let _ = event_tx.send(kind.clone());
                false // remove from pending
            } else {
                true // keep waiting
            }
        });

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn paths() -> (PathBuf, PathBuf, PathBuf) {
        let overlay_dir = PathBuf::from("/data/overlays/Default");
        let themes_dir = PathBuf::from("/data/themes");
        let config_path = PathBuf::from("/data/config.json");
        (overlay_dir, themes_dir, config_path)
    }

    #[test]
    fn test_classify_config() {
        let (overlay_dir, themes_dir, config_path) = paths();
        let result = classify_path(&config_path, &overlay_dir, &themes_dir, &config_path);
        assert_eq!(result, Some(ReloadEvent::Config));
    }

    #[test]
    fn test_classify_overlay_omni() {
        let (overlay_dir, themes_dir, config_path) = paths();
        let file = overlay_dir.join("overlay.omni");
        let result = classify_path(&file, &overlay_dir, &themes_dir, &config_path);
        assert_eq!(result, Some(ReloadEvent::Overlay));
    }

    #[test]
    fn test_classify_overlay_css() {
        let (overlay_dir, themes_dir, config_path) = paths();
        let file = overlay_dir.join("style.css");
        let result = classify_path(&file, &overlay_dir, &themes_dir, &config_path);
        assert_eq!(result, Some(ReloadEvent::Overlay));
    }

    #[test]
    fn test_classify_theme_css() {
        let (overlay_dir, themes_dir, config_path) = paths();
        let file = themes_dir.join("dark.css");
        let result = classify_path(&file, &overlay_dir, &themes_dir, &config_path);
        assert_eq!(result, Some(ReloadEvent::Theme));
    }

    #[test]
    fn test_classify_non_css_in_themes_ignored() {
        let (overlay_dir, themes_dir, config_path) = paths();
        let file = themes_dir.join("README.txt");
        let result = classify_path(&file, &overlay_dir, &themes_dir, &config_path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_classify_unrelated_file_ignored() {
        let (overlay_dir, themes_dir, config_path) = paths();
        let file = PathBuf::from("/tmp/something_else.css");
        let result = classify_path(&file, &overlay_dir, &themes_dir, &config_path);
        assert_eq!(result, None);
    }

    // -------------------------------------------------------------------------
    // Integration tests — require a real filesystem and a running watcher thread
    // Run with: cargo test -p omni-host watcher -- --test-threads=1
    // -------------------------------------------------------------------------

    /// RAII temp workspace: directories are removed on drop even if the test panics.
    struct TempWorkspace {
        root: PathBuf,
        overlay_dir: PathBuf,
        themes_dir: PathBuf,
        config_path: PathBuf,
    }

    impl TempWorkspace {
        fn new() -> Self {
            let id = format!(
                "omni-watcher-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            );
            let root = std::env::temp_dir().join(id);
            let overlay_dir = root.join("overlays").join("Default");
            let themes_dir = root.join("themes");
            let config_path = root.join("config.json");

            fs::create_dir_all(&overlay_dir).expect("create overlay_dir");
            fs::create_dir_all(&themes_dir).expect("create themes_dir");
            fs::write(&config_path, "{}").expect("create config.json");
            // Seed a file so the overlay dir isn't empty
            fs::write(overlay_dir.join("overlay.omni"), "# initial").expect("seed overlay.omni");

            Self { root, overlay_dir, themes_dir, config_path }
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn watcher_detects_overlay_change() {
        let ws = TempWorkspace::new();

        let fw = FileWatcher::start(
            ws.overlay_dir.clone(),
            ws.themes_dir.clone(),
            ws.config_path.clone(),
        )
        .expect("FileWatcher::start");

        // Give the watcher time to register its watches
        std::thread::sleep(Duration::from_millis(100));

        fs::write(ws.overlay_dir.join("overlay.omni"), "# changed").expect("write overlay.omni");

        let event = fw
            .recv_timeout(Duration::from_secs(5))
            .expect("expected ReloadEvent::Overlay within 5s");
        assert_eq!(event, ReloadEvent::Overlay);
    }

    #[test]
    fn watcher_detects_theme_change() {
        let ws = TempWorkspace::new();

        let fw = FileWatcher::start(
            ws.overlay_dir.clone(),
            ws.themes_dir.clone(),
            ws.config_path.clone(),
        )
        .expect("FileWatcher::start");

        std::thread::sleep(Duration::from_millis(100));

        fs::write(ws.themes_dir.join("dark.css"), "body { color: red; }").expect("write dark.css");

        let event = fw
            .recv_timeout(Duration::from_secs(5))
            .expect("expected ReloadEvent::Theme within 5s");
        assert_eq!(event, ReloadEvent::Theme);
    }

    #[test]
    fn watcher_detects_config_change() {
        let ws = TempWorkspace::new();

        let fw = FileWatcher::start(
            ws.overlay_dir.clone(),
            ws.themes_dir.clone(),
            ws.config_path.clone(),
        )
        .expect("FileWatcher::start");

        std::thread::sleep(Duration::from_millis(100));

        fs::write(&ws.config_path, r#"{"theme":"dark"}"#).expect("write config.json");

        let event = fw
            .recv_timeout(Duration::from_secs(5))
            .expect("expected ReloadEvent::Config within 5s");
        assert_eq!(event, ReloadEvent::Config);
    }

    #[test]
    fn watcher_debounces_rapid_changes() {
        let ws = TempWorkspace::new();

        let fw = FileWatcher::start(
            ws.overlay_dir.clone(),
            ws.themes_dir.clone(),
            ws.config_path.clone(),
        )
        .expect("FileWatcher::start");

        std::thread::sleep(Duration::from_millis(100));

        // Rapid-fire 5 writes with 50 ms between each
        let overlay_file = ws.overlay_dir.join("overlay.omni");
        for i in 0..5u32 {
            fs::write(&overlay_file, format!("# write {i}")).expect("rapid write");
            std::thread::sleep(Duration::from_millis(50));
        }

        // Exactly one debounced event should arrive
        let event = fw
            .recv_timeout(Duration::from_secs(5))
            .expect("expected exactly one ReloadEvent::Overlay within 5s");
        assert_eq!(event, ReloadEvent::Overlay);

        // No second event should arrive within 300 ms
        let second = fw.recv_timeout(Duration::from_millis(300));
        assert!(
            second.is_err(),
            "expected no second event within 300ms, got: {second:?}"
        );
    }
}
