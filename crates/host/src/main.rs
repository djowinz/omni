use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

use omni::html_builder;
use serde_json::json;

/// Store initial HTML for new preview subscribers and broadcast to existing ones.
fn store_and_broadcast_preview(
    ws_state: &ws_server::WsSharedState,
    initial: &html_builder::InitialHtml,
) {
    {
        let mut latest = ws_state.latest_initial_html.lock().unwrap();
        *latest = Some((initial.html.clone(), initial.css.clone()));
    }
    let msg = json!({
        "type": "preview.html",
        "html": &initial.html,
        "css": &initial.css,
    });
    ws_server::broadcast_preview(ws_state, &msg.to_string());
}

struct HostState {
    omni_file: omni::OmniFile,
    current_overlay: String,
    file_watcher: Option<watcher::FileWatcher>,
    data_dir: PathBuf,
    config_path: PathBuf,
    sensor_history: omni::history::SensorHistory,
}

impl HostState {
    fn new(overlay_name: String, data_dir: PathBuf, config_path: PathBuf) -> Self {
        Self {
            omni_file: omni::OmniFile::empty(),
            current_overlay: overlay_name,
            file_watcher: None,
            data_dir,
            config_path,
            sensor_history: omni::history::SensorHistory::new(),
        }
    }

    /// Sync `sensor_history` registrations to the sensors referenced by charts
    /// in the currently loaded `omni_file`. Call after any overlay load/reload.
    fn sync_chart_sensor_registrations(&mut self) {
        let chart_sensors = omni::parser::collect_chart_sensors(&self.omni_file);
        for s in &chart_sensors {
            self.sensor_history.register(s);
        }
        self.sensor_history.clear_unregistered(&chart_sensors);
    }

    /// Attempt to load the current overlay from disk and apply it.
    /// Returns `true` if the overlay was successfully loaded.
    fn reload_overlay(&mut self) -> bool {
        let omni_path =
            workspace::structure::overlay_omni_path(&self.data_dir, &self.current_overlay);
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
                    line = diag.line,
                    col = diag.column,
                    msg = %diag.message,
                    suggestion = ?diag.suggestion,
                    "parse error"
                ),
                omni::parser::Severity::Warning => warn!(
                    line = diag.line,
                    col = diag.column,
                    msg = %diag.message,
                    suggestion = ?diag.suggestion,
                    "parse warning"
                ),
            }
        }

        match parsed {
            Some(new_file) => {
                info!(
                    widgets = new_file.widgets.len(),
                    "Overlay loaded successfully"
                );
                self.omni_file = new_file;
                true
            }
            None => {
                warn!("Parse errors in overlay — keeping previous version");
                false
            }
        }
    }

    /// Recreate the file watcher for a new overlay folder and load the overlay.
    /// We rebuild the watcher entirely because the debounce thread captures
    /// a canonicalized overlay_dir at startup and cannot be updated in place.
    fn switch_overlay(&mut self, new_name: &str) -> bool {
        let new_dir = workspace::structure::overlay_dir(&self.data_dir, new_name);
        let themes_dir = self.data_dir.join("themes");

        self.file_watcher = match watcher::FileWatcher::start(
            new_dir.clone(),
            themes_dir,
            self.config_path.clone(),
        ) {
            Ok(w) => {
                info!(path = %new_dir.display(), "Recreated file watcher for new overlay");
                Some(w)
            }
            Err(e) => {
                warn!(error = %e, "Failed to recreate file watcher");
                None
            }
        };

        self.current_overlay = new_name.to_string();
        self.reload_overlay()
    }
}

use omni_host::{
    config, etw, hotkey, ipc, omni, scanner, sensors, ul_renderer, watcher, win32, workspace,
    ws_server,
};

static RUNNING: AtomicBool = AtomicBool::new(true);

/// A capped log file writer. When the file exceeds `max_bytes`, it keeps
/// only the most recent half of the content.
struct CappedLogWriter {
    path: PathBuf,
    file: std::fs::File,
    max_bytes: u64,
    bytes_written: u64,
}

impl CappedLogWriter {
    fn new(path: PathBuf, max_bytes: u64) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let bytes_written = file.metadata()?.len();
        Ok(Self {
            path,
            file,
            max_bytes,
            bytes_written,
        })
    }

    fn truncate_if_needed(&mut self) {
        if self.bytes_written <= self.max_bytes {
            return;
        }
        // Read the file, keep the last half
        if let Ok(content) = std::fs::read_to_string(&self.path) {
            let keep_from = content.len() / 2;
            // Find the next newline after the midpoint so we don't cut a line
            let start = content[keep_from..]
                .find('\n')
                .map(|i| keep_from + i + 1)
                .unwrap_or(keep_from);
            let trimmed = &content[start..];
            if std::fs::write(&self.path, trimmed).is_ok() {
                // Reopen in append mode
                if let Ok(f) = std::fs::OpenOptions::new().append(true).open(&self.path) {
                    self.file = f;
                    self.bytes_written = trimmed.len() as u64;
                }
            }
        }
    }
}

impl std::io::Write for CappedLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.bytes_written += n as u64;
        // Check every 64KB of writes to avoid checking on every line
        if self.bytes_written > self.max_bytes {
            self.truncate_if_needed();
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn main() {
    // Set up logging to both stderr and %APPDATA%\Omni\logs\omni-host.log
    let log_dir = config::data_dir().join("logs");
    std::fs::create_dir_all(&log_dir).ok();

    let log_path = log_dir.join("omni-host.log");
    // Cap log file at 5 MB — when exceeded, keeps the most recent half
    let log_writer =
        CappedLogWriter::new(log_path, 5 * 1024 * 1024).expect("Failed to open log file");

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Mutex::new(log_writer))
                .with_ansi(false),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "--stop" => run_stop(),
        "--service" | "--watch" => {
            run_host();
        }
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  omni-host --service              Service mode (WebSocket API, external overlay)");
    eprintln!("  omni-host --stop                 Stop all running omni-host instances");
}

fn run_stop() {
    let my_pid = std::process::id();

    let processes = match win32::iter_processes() {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "Failed to enumerate processes");
            std::process::exit(1);
        }
    };

    // Kill any running omni-host instances.
    let mut killed = 0u32;
    for entry in &processes {
        let pid = entry.th32ProcessID;
        if pid == my_pid || pid <= 4 {
            continue;
        }

        let name = win32::wchar_to_string(&entry.szExeFile);
        if !name.eq_ignore_ascii_case("omni-host.exe") {
            continue;
        }

        info!(pid, "Terminating omni-host instance");
        // SAFETY: Opening with PROCESS_TERMINATE on a verified omni-host.exe PID.
        match unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) } {
            Ok(raw) => {
                let handle = win32::OwnedHandle::new(raw);
                // SAFETY: Valid handle, process is a known omni-host instance.
                unsafe {
                    if TerminateProcess(handle.raw(), 0).is_err() {
                        error!(pid, "Failed to terminate process");
                    }
                }
                killed += 1;
            }
            Err(_) => {
                error!(pid, "Failed to open process for termination");
            }
        }
    }

    if killed == 0 {
        info!("No running omni-host instances found");
    } else {
        info!(killed, "Terminated omni-host instances");
    }
}

/// Core host loop shared by --watch and --service modes.
fn run_host() {
    let config_path = config::config_path();
    let mut config = config::load_config(&config_path);
    let data_dir = config::data_dir();
    let scan_interval = Duration::from_millis(2000);

    // Initialize workspace folder structure (overlays/, themes/, Default overlay)
    workspace::structure::init_workspace(&data_dir);

    ctrlc::set_handler(|| {
        RUNNING.store(false, Ordering::Relaxed);
    })
    .expect("Failed to set Ctrl+C handler");

    // Parse the toggle keybind and create a poller
    let initial_hotkey =
        hotkey::parse_keybind(&config.keybinds.toggle_overlay).unwrap_or_else(|| {
            warn!(
                keybind = %config.keybinds.toggle_overlay,
                "Failed to parse toggle keybind, falling back to F12"
            );
            hotkey::parse_keybind("F12").unwrap()
        });
    let mut hotkey_poller = hotkey::HotkeyPoller::new(initial_hotkey);

    // Shared state for WebSocket server
    let ws_state = Arc::new(ws_server::WsSharedState::new(data_dir.clone()));

    // Start WebSocket server
    let ws_handle = ws_server::start(ws_state.clone());

    let sensor_running = std::sync::Arc::new(AtomicBool::new(true));

    info!(
        config_path = ?config_path,
        ws_port = ws_server::WS_PORT,
        exclude_count = config.exclude.len(),
        "Omni host starting"
    );
    info!("Press Ctrl+C to stop");

    let mut latest_snapshot = omni_shared::SensorSnapshot::default();

    // Resolve which overlay to load
    let overlay_name = workspace::overlay_resolver::resolve_overlay_name(
        None, // No game running yet — will be updated when scanner detects one
        &config.overlay_by_game,
        &config.active_overlay,
        &data_dir,
    );
    info!(overlay = %overlay_name, "Resolved active overlay");

    // Determine overlay exe path (next to host exe)
    let overlay_exe_path = std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent()
                .map(|d| d.join("omni-overlay.exe").to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "omni-overlay.exe".to_string());

    let mut scanner_instance = scanner::Scanner::new(overlay_exe_path, config.clone());

    let mut etw_captures: std::collections::HashMap<u32, etw::EtwCapture> =
        std::collections::HashMap::new();
    let mut etw_failed: std::collections::HashSet<u32> = std::collections::HashSet::new();

    let mut host = HostState::new(overlay_name, data_dir.clone(), config_path.clone());

    // Load the initial overlay
    host.reload_overlay();
    host.sync_chart_sensor_registrations();
    if let Ok(mut overlay) = ws_state.active_overlay.lock() {
        *overlay = host.current_overlay.clone();
    }

    // Start sensor polling on background thread (uses poll_config from .omni file)
    let (mut sensor_poller, sensor_rx, hwinfo_rx) =
        sensors::SensorPoller::start(host.omni_file.poll_config.clone(), sensor_running);

    // Start file watcher for hot-reload
    let current_overlay_dir =
        workspace::structure::overlay_dir(&host.data_dir, &host.current_overlay);
    let themes_dir = host.data_dir.join("themes");
    host.file_watcher = match watcher::FileWatcher::start(
        current_overlay_dir,
        themes_dir,
        host.config_path.clone(),
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

    // Create bitmap shared memory for Ultralight pixel output
    let mut bitmap_writer =
        ipc::BitmapWriter::create().expect("Failed to create bitmap shared memory");

    // Determine the resources directory (next to the exe, where build.rs copies it)
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut ul = ul_renderer::UlRenderer::init(1920, 1080, &exe_dir)
        .expect("Failed to initialize Ultralight renderer");

    let mut ul_viewport_w: u32 = 0;
    let mut ul_viewport_h: u32 = 0;
    let mut ul_needs_reload = false;

    // Load initial HTML into Ultralight (styles + body + omniUpdate JS function)
    {
        let (hwinfo_values, hwinfo_units) = ws_state.hwinfo_values_and_units();
        let initial = html_builder::build_initial_html(
            &host.omni_file,
            &latest_snapshot,
            1920,
            1080,
            &data_dir,
            &host.current_overlay,
            &hwinfo_values,
            &hwinfo_units,
            &host.sensor_history,
            crate::omni::ViewTrust::LocalAuthored,
        );
        let overlay_root = workspace::structure::overlay_dir(&data_dir, &host.current_overlay);
        if let Err(e) = ul.mount(
            &overlay_root,
            &initial.full_document,
            omni_host::omni::view_trust::ViewTrust::LocalAuthored,
        ) {
            tracing::warn!(error = %e, "Failed to mount overlay");
        }
        store_and_broadcast_preview(&ws_state, &initial);
        // Pump Ultralight for a few frames to let it initialize
        for _ in 0..10 {
            ul.update_and_render();
            std::thread::sleep(Duration::from_millis(16));
        }
        info!("Ultralight initial HTML loaded and pumped");
    }

    let mut last_scan = Instant::now();

    while RUNNING.load(Ordering::Relaxed) {
        if last_scan.elapsed() >= scan_interval {
            scanner_instance.poll();
            last_scan = Instant::now();

            // Start ETW capture for newly tracked external overlay processes
            for &pid in scanner_instance.tracked_pids() {
                if !etw_captures.contains_key(&pid) && !etw_failed.contains(&pid) {
                    match etw::EtwCapture::start(pid) {
                        Ok(capture) => {
                            info!(pid, "Started ETW frame capture");
                            etw_captures.insert(pid, capture);
                        }
                        Err(e) => {
                            warn!(pid, error = %e, "Failed to start ETW capture — frame metrics unavailable for this process");
                            etw_failed.insert(pid);
                        }
                    }
                }
            }

            // Clean up ETW sessions and failure tracking for exited processes
            etw_captures.retain(|pid, _| scanner_instance.is_tracked(*pid));
            etw_failed.retain(|pid| scanner_instance.is_tracked(*pid));

            if let Ok(mut game) = ws_state.active_game.lock() {
                *game = scanner_instance.last_game_exe().map(|s| s.to_string());
            }

            // Re-resolve overlay based on current game
            let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                scanner_instance.last_game_exe(),
                &config.overlay_by_game,
                &config.active_overlay,
                &data_dir,
            );

            if new_overlay != host.current_overlay {
                info!(
                    from = %host.current_overlay,
                    to = %new_overlay,
                    game = ?scanner_instance.last_game_exe(),
                    "Game-specific overlay switch"
                );
                host.switch_overlay(&new_overlay);
                host.sync_chart_sensor_registrations();
                if let Ok(mut overlay) = ws_state.active_overlay.lock() {
                    *overlay = host.current_overlay.clone();
                }
            }
        }

        // Drain the poller's snapshot channel. Each delivered snapshot represents
        // one poll cycle, so push samples into the chart history ONCE per snapshot.
        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;

            let registered: Vec<String> = host
                .sensor_history
                .registered_iter()
                .map(str::to_string)
                .collect();
            if !registered.is_empty() {
                let hwinfo_values_snapshot = ws_state
                    .hwinfo_state
                    .lock()
                    .map(|s| s.values.clone())
                    .unwrap_or_default();
                for path in &registered {
                    if let Some(v) = omni::sensor_map::get_sensor_value_f64(
                        path,
                        &latest_snapshot,
                        &hwinfo_values_snapshot,
                    ) {
                        host.sensor_history.push_sample(path, v);
                    }
                }
            }
        }

        // Receive HWiNFO state updates
        let mut hwinfo_updated = false;
        while let Ok((new_hwinfo_state, sensors_changed)) = hwinfo_rx.try_recv() {
            if let Ok(mut hwinfo_state) = ws_state.hwinfo_state.lock() {
                *hwinfo_state = new_hwinfo_state;
            }
            hwinfo_updated = true;
            if sensors_changed {
                if let Ok(mut changed) = ws_state.hwinfo_sensors_changed.lock() {
                    *changed = true;
                }
            }
        }

        // Push HWiNFO-referenced chart sensor samples into history on update
        if hwinfo_updated {
            let registered: Vec<String> = host
                .sensor_history
                .registered_iter()
                .map(str::to_string)
                .collect();
            if !registered.is_empty() {
                let hwinfo_values_now = ws_state
                    .hwinfo_state
                    .lock()
                    .map(|s| s.values.clone())
                    .unwrap_or_default();
                for path in &registered {
                    if let Some(v) = hwinfo_values_now.get(path) {
                        host.sensor_history.push_sample(path, *v);
                    }
                }
            }
        }

        // Merge ETW frame metrics for the most recently spawned external overlay
        if let Some(last_pid) = scanner_instance.last_external_pid() {
            if let Some(capture) = etw_captures.get(&last_pid) {
                let etw_metrics = capture.latest_metrics();
                if etw_metrics.available {
                    latest_snapshot.frame = etw_metrics.into();
                }
            }
        }

        // For external overlay mode: use the game window's client rect as
        // the render viewport. The DLL normally reports this via frame data,
        // but external overlay mode has no DLL.
        if latest_snapshot.frame.render_width == 0 || latest_snapshot.frame.render_height == 0 {
            if let Some(hwnd_val) = scanner_instance.last_game_hwnd() {
                let hwnd = windows::Win32::Foundation::HWND(hwnd_val as *mut _);
                let mut rect = windows::Win32::Foundation::RECT::default();
                if unsafe {
                    windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect)
                }
                .is_ok()
                {
                    let w = (rect.right - rect.left) as u32;
                    let h = (rect.bottom - rect.top) as u32;
                    if w > 0 && h > 0 {
                        latest_snapshot.frame.render_width = w;
                        latest_snapshot.frame.render_height = h;
                    }
                }
            }
        }

        // Update WebSocket shared state
        if let Ok(mut ws_snapshot) = ws_state.latest_snapshot.lock() {
            *ws_snapshot = latest_snapshot;
        }

        // Check for widget updates from WebSocket (Electron app)
        let mut overlay_changed = false;
        if let Ok(mut active) = ws_state.active_omni_file.lock() {
            if let Some(new_file) = active.take() {
                info!(
                    widget_count = new_file.widgets.len(),
                    enabled = new_file.widgets.iter().filter(|w| w.enabled).count(),
                    "Applied widget update from WebSocket"
                );
                host.omni_file = new_file;
                ul_needs_reload = true;
                overlay_changed = true;
            }
        }
        if overlay_changed {
            host.sync_chart_sensor_registrations();
        }

        // Handle file watcher events (hot-reload)
        let pending_events = match host.file_watcher {
            Some(ref fw) => fw.drain_events(),
            None => Vec::new(),
        };

        for event in pending_events {
            match event {
                watcher::ReloadEvent::Overlay => {
                    info!("Overlay file changed — reloading");
                    host.reload_overlay();
                    host.sync_chart_sensor_registrations();
                    ul_needs_reload = true;
                }
                watcher::ReloadEvent::Theme => {
                    info!("Theme file changed — reloading");
                    ul_needs_reload = true;
                }
                watcher::ReloadEvent::Config => {
                    info!("Config changed — reloading");
                    let new_config = config::load_config(&host.config_path);

                    let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                        scanner_instance.last_game_exe(),
                        &new_config.overlay_by_game,
                        &new_config.active_overlay,
                        &host.data_dir,
                    );

                    if new_overlay != host.current_overlay {
                        info!(
                            from = %host.current_overlay,
                            to = %new_overlay,
                            "Active overlay changed — switching"
                        );
                        host.switch_overlay(&new_overlay);
                        host.sync_chart_sensor_registrations();
                        if let Ok(mut overlay) = ws_state.active_overlay.lock() {
                            *overlay = host.current_overlay.clone();
                        }
                    }

                    config = new_config;
                    // Update hotkey poller if keybind changed
                    if let Some(hk) = hotkey::parse_keybind(&config.keybinds.toggle_overlay) {
                        hotkey_poller.set_hotkey(hk);
                        info!(keybind = %config.keybinds.toggle_overlay, "Toggle keybind updated");
                    }
                }
            }
        }

        // Poll toggle overlay hotkey
        if hotkey_poller.poll() {
            let bmp_header = unsafe { &*bitmap_writer.header_ptr() };
            bmp_header.toggle_visible();
            info!("Overlay visibility toggled");
        }

        // --- Ultralight: viewport resize ---
        {
            let rw = latest_snapshot.frame.render_width;
            let rh = latest_snapshot.frame.render_height;
            if rw > 0 && rh > 0 && (rw != ul_viewport_w || rh != ul_viewport_h) {
                ul_viewport_w = rw;
                ul_viewport_h = rh;
                ul.resize(rw, rh);
                ul_needs_reload = true;
                info!(width = rw, height = rh, "Ultralight viewport resized");
            }
        }

        // --- Ultralight: reload full HTML if structure changed ---
        if ul_needs_reload {
            let vw = if latest_snapshot.frame.render_width > 0 {
                latest_snapshot.frame.render_width
            } else {
                1920
            };
            let vh = if latest_snapshot.frame.render_height > 0 {
                latest_snapshot.frame.render_height
            } else {
                1080
            };
            let (hwinfo_values, hwinfo_units) = ws_state
                .hwinfo_state
                .lock()
                .map(|s| (s.values.clone(), s.units.clone()))
                .unwrap_or_default();
            let initial = html_builder::build_initial_html(
                &host.omni_file,
                &latest_snapshot,
                vw,
                vh,
                &data_dir,
                &host.current_overlay,
                &hwinfo_values,
                &hwinfo_units,
                &host.sensor_history,
                crate::omni::ViewTrust::LocalAuthored,
            );
            let overlay_root = workspace::structure::overlay_dir(&data_dir, &host.current_overlay);
            if let Err(e) = ul.mount(
                &overlay_root,
                &initial.full_document,
                omni_host::omni::view_trust::ViewTrust::LocalAuthored,
            ) {
                tracing::warn!(error = %e, "Failed to mount overlay");
            }
            store_and_broadcast_preview(&ws_state, &initial);
            for _ in 0..10 {
                ul.update_and_render();
                std::thread::sleep(Duration::from_millis(16));
            }
            ul_needs_reload = false;
            info!("Ultralight HTML reloaded");
        }

        // --- Ultralight: push sensor/class updates via JS, then render ---
        // The HTML is loaded once (bootstrap + styles + body). Each cycle we:
        //   1. Push raw sensor values via __omni_update(values) so the bootstrap
        //      updates all data-sensor spans — CSS animations survive DOM mutations.
        //   2. Push conditional-class changes via __omni_set_classes for server-side
        //      threshold expressions and attribute interpolations.
        // The DOM persists so CSS transitions animate naturally.
        let (hwinfo_values, hwinfo_units) = ws_state.hwinfo_values_and_units();

        // Push raw sensor values via the bootstrap's __omni_update.
        let values = html_builder::collect_sensor_values(
            &host.omni_file,
            &latest_snapshot,
            &hwinfo_values,
        );
        if !values.is_empty() {
            ul.evaluate_script(&html_builder::format_values_js(&values));
        }

        // Push conditional-class updates for expressions too rich for threshold attrs.
        let class_diff = html_builder::compute_update_diff(
            &host.omni_file,
            &latest_snapshot,
            &hwinfo_values,
            &hwinfo_units,
        );
        let class_js = class_diff
            .as_ref()
            .and_then(|d| html_builder::format_classes_js(d));
        if let Some(js) = &class_js {
            ul.evaluate_script(js);
        }

        // Preview subscribers keep receiving the class diff; values are also
        // included so the Nextron editor can display live sensor readings.
        let subs = ws_state.preview_subscribers.lock().unwrap();
        if !subs.is_empty() {
            drop(subs);
            let preview_msg = json!({
                "type": "preview.update",
                "values": values,
                "diff": class_diff,
            });
            ws_server::broadcast_preview(&ws_state, &preview_msg.to_string());
        }
        ul.update_and_render();
        ul.with_pixels(|w, h, rb, pixels, dirty| {
            bitmap_writer.write(w, h, rb, pixels, dirty);
        });

        debug!("Ultralight frame rendered");

        // Wake on scanner poll, watcher events, or sensor data
        let timeout = {
            let until_scan = scan_interval.saturating_sub(last_scan.elapsed());
            until_scan.min(Duration::from_millis(100))
        };

        // Block until sensor data arrives or timeout expires.
        if let Ok(snapshot) = sensor_rx.recv_timeout(timeout) {
            latest_snapshot = snapshot;
        }
    }

    info!("Shutting down — killing external overlay processes");
    scanner_instance.kill_all();
    sensor_poller.stop();
    ws_state.running.store(false, Ordering::Relaxed);
    if let Err(e) = ws_handle.join() {
        warn!("WebSocket server thread panicked: {e:?}");
    }
    info!("Omni host stopped");
}
