use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, MonitorFromPoint, HDC, HMONITOR, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// Primary monitor dimensions in physical pixels.
///
/// The host process is marked PerMonitorV2 DPI-aware via `resources/app.manifest`,
/// so `GetSystemMetrics(SM_CXSCREEN/SM_CYSCREEN)` returns true physical pixels
/// rather than DPI-virtualized values. Used as the initial Ultralight viewport
/// before a game window is detected, and as the fallback when the game window's
/// client rect query is unavailable.
fn primary_monitor_size() -> (u32, u32) {
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    if w > 0 && h > 0 {
        (w as u32, h as u32)
    } else {
        (1920, 1080)
    }
}

/// Primary monitor's effective DPI (96 = 100% scaling). Falls back to 96
/// if `GetDpiForMonitor` returns an error or zero.
///
/// Used by `resolve_dpi_scale` when an overlay declares
/// `<dpi-scale value="auto"/>` and no game window is currently tracked,
/// or as a fallback when `GetDpiForWindow` returns 0.
///
/// Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
fn primary_monitor_dpi() -> u32 {
    let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut x_dpi: u32 = 96;
    let mut y_dpi: u32 = 96;
    let _ = unsafe { GetDpiForMonitor(primary, MDT_EFFECTIVE_DPI, &mut x_dpi, &mut y_dpi) };
    if x_dpi == 0 {
        96
    } else {
        x_dpi
    }
}

/// Resolve the overlay's `<dpi-scale>` directive against the current
/// monitor context. Returns the f64 scale to pass to UlRenderer, or `None`
/// when the overlay opts out (no `<dpi-scale>` directive — preserves
/// today's behavior).
///
/// `Manual(s)` clamps `s` to `[0.5, 4.0]`. `Auto` queries the game window's
/// monitor DPI via `GetDpiForWindow` (falling back to primary monitor DPI
/// when no game is tracked or the call returns 0) and divides by 96.
///
/// Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
fn resolve_dpi_scale(
    config: Option<omni_host::omni::types::DpiScale>,
    game_hwnd: Option<isize>,
) -> Option<f64> {
    use omni_host::omni::types::DpiScale;
    match config {
        None => None,
        Some(DpiScale::Manual(s)) => Some(s.clamp(0.5, 4.0)),
        Some(DpiScale::Auto) => {
            let dpi = match game_hwnd {
                Some(h) => {
                    let hwnd = HWND(h as *mut _);
                    let d = unsafe { GetDpiForWindow(hwnd) };
                    if d == 0 {
                        primary_monitor_dpi()
                    } else {
                        d
                    }
                }
                None => primary_monitor_dpi(),
            };
            Some(((dpi as f64) / 96.0).clamp(0.5, 4.0))
        }
    }
}

/// Re-resolve the desired DPI scale and recreate the Ultralight view if
/// it differs from the currently-applied scale. On success, sets
/// `ul_needs_reload` so the next loop iteration rebuilds HTML and re-mounts.
///
/// Called after every overlay-state mutator (`switch_overlay`,
/// `reload_overlay`, WS push) — anywhere `host.omni_file.dpi_scale` may
/// have changed. The per-frame Auto tracker has its own dedicated block
/// since it doesn't pass through `host.omni_file`.
///
/// On `recreate_view` failure, the renderer rolls back to its previous
/// dimensions WITH NO DEVICE SCALE (see `UlRenderer::recreate_view`'s
/// rollback-on-failure guarantee). We mirror that reality into
/// `current_dpi_*` (clearing both) so:
///   - The renderer's actual state matches the host's tracked state.
///   - The per-frame `Auto` tracker stops firing for the failed scale
///     (its gate is `current_dpi_config == Some(Auto)`); the next
///     overlay-state mutator (switch/reload/WS push) re-reads the
///     overlay's `dpi_scale` and tries again.
///
/// On the no-change branch, `current_dpi_config` is still mirrored so
/// the per-frame `Auto` tracker reads fresh state even when consecutive
/// overlays happen to resolve to the same scale.
///
/// Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
#[allow(clippy::too_many_arguments)]
fn maybe_recreate_for_scale_change(
    ul: &mut ul_renderer::UlRenderer,
    current_dpi_config: &mut Option<omni_host::omni::types::DpiScale>,
    current_dpi_scale: &mut Option<f64>,
    ul_needs_reload: &mut bool,
    new_config: Option<omni_host::omni::types::DpiScale>,
    game_hwnd: Option<isize>,
    width: u32,
    height: u32,
) {
    let new_scale = resolve_dpi_scale(new_config, game_hwnd);
    if new_scale == *current_dpi_scale {
        // No recreation needed, but still mirror the active overlay's
        // declared config so the per-frame Auto tracker sees fresh state.
        *current_dpi_config = new_config;
        return;
    }
    match ul.recreate_view(width, height, new_scale) {
        Ok(()) => {
            info!(?new_scale, ?new_config, "DPI scale changed; view recreated");
            *current_dpi_config = new_config;
            *current_dpi_scale = new_scale;
            *ul_needs_reload = true;
        }
        Err(e) => {
            warn!(
                error = %e,
                "view recreation failed; renderer rolled back to no-scale geometry"
            );
            // Rollback put the renderer at OLD dims + NO scale per
            // recreate_view's contract. Reflect that truth and clear the
            // config so the per-frame Auto tracker stops retrying the
            // same failed scale. The next overlay mutator will re-read
            // host.omni_file.dpi_scale and attempt again.
            *current_dpi_config = None;
            *current_dpi_scale = None;
        }
    }
}

/// Largest single-monitor pixel area (width × height) across all currently
/// connected displays, in physical pixels. Used to size the bitmap shared
/// memory so any game window on any monitor fits without truncation.
///
/// Returns 0 if enumeration produces no monitors; caller must apply its own
/// floor (typically the primary monitor's area or a 4K constant).
fn max_single_monitor_pixel_area() -> u64 {
    let mut max_area: u64 = 0;
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC(std::ptr::null_mut()),
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut max_area as *mut u64 as isize),
        );
    }
    max_area
}

unsafe extern "system" fn monitor_enum_proc(
    _hmon: HMONITOR,
    _hdc: HDC,
    rc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    if rc.is_null() || lparam.0 == 0 {
        return BOOL(1);
    }
    let r = &*rc;
    let w = (r.right - r.left).max(0) as u64;
    let h = (r.bottom - r.top).max(0) as u64;
    let area = w * h;
    let max_ptr = lparam.0 as *mut u64;
    if area > *max_ptr {
        *max_ptr = area;
    }
    BOOL(1)
}

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
    // Initial retry delay after an ETW capture failure. Doubles on each
    // subsequent failure up to `ETW_RETRY_MAX`, so persistently-unsuccessful
    // PIDs (e.g. 32-bit games, insufficient privileges) don't spam retries.
    const ETW_RETRY_INITIAL: Duration = Duration::from_secs(5);
    const ETW_RETRY_MAX: Duration = Duration::from_secs(300);

    // Initialize workspace folder structure (overlays/, themes/, Default overlay)
    workspace::structure::init_workspace(&data_dir);

    // Sweep orphaned `.omni-staging-*` directories left behind by prior
    // host crashes mid-install. Invariant: this only removes directories
    // matching the staging prefix at the workspace root — no recursion
    // into user content. Non-fatal on failure.
    match workspace::atomic_dir::sweep_orphans(&data_dir) {
        Ok(0) => {}
        Ok(n) => info!("startup: cleaned {n} orphaned staging directories"),
        Err(e) => warn!("startup: sweep_orphans failed: {e}"),
    }

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

    // Resolve which overlay to load. Moved above `build_share_context` so the
    // real `ThemeSwapImpl` can seed its baseline CSS from the current
    // overlay's theme at host startup (phase-2 followup #3 — replaces the
    // `NoopThemeSwap` previously installed by `build_share_context`).
    let overlay_name = workspace::overlay_resolver::resolve_overlay_name(
        None, // No game running yet — will be updated when scanner detects one
        &config.overlay_by_game,
        &config.active_overlay,
        &data_dir,
    );
    info!(overlay = %overlay_name, "Resolved active overlay");

    // Shared pending-theme slot. `ThemeSwapImpl::apply`/`revert` write here
    // (from any tokio worker); the main render loop drains and emits the
    // `__omni_set_theme({…})` JS invocation on-thread once per frame. See
    // `crates/host/src/share/preview_impl.rs` module doc for rationale.
    let pending_theme_slot: omni_host::share::preview_impl::PendingSlot =
        Arc::new(std::sync::Mutex::new(None));

    // Attempt ShareContext construction (plan #021 T4). Failure is non-fatal —
    // `explorer.install|preview|cancelPreview|list|get` and `upload.*`/`identity.*`
    // handlers gracefully degrade to the `service_unavailable` D-004-J envelope via
    // the fallback path in `ws_server::handle_message`. Contributors building
    // offline (no identity file yet, no Worker reachable) still get a working host
    // minus the share surface.
    match build_share_context(&ws_state, &overlay_name, pending_theme_slot.clone()) {
        Ok(ctx) => ws_state.set_share_ctx(std::sync::Arc::new(ctx)),
        Err(e) => tracing::warn!(
            error = %e,
            "share service not configured; WS install/upload/preview will return service_unavailable"
        ),
    }

    // Initialize the moderation singleton (OWI-73) before the WS server starts
    // accepting connections. Both `share.moderationCheck` (renderer-initiated;
    // INV-7.7.2 site #1) and the pack-time `share::dep_resolver` content-safety
    // path (INV-7.7.2 site #2) require the cached NudeNet `Session` to be
    // loaded; otherwise every check returns `CheckError::NotInitialized`.
    //
    // Degraded-mode policy: if the bundled `nudenet.onnx` is missing (dev
    // builds without the model staged, or a corrupted install), log + warn and
    // allow startup to continue. The renderer's WS handler already surfaces a
    // structured `Moderation:NotInitialized` error, and `dep_resolver`'s
    // wrapper degrades to `Skipped` outcomes (see `share::dep_resolver` module
    // doc). Blocking startup would punish dev contributors building without
    // the model far more than it would harden the upload pipeline. Production
    // installers ship the model under `resources/moderation/`, so the warn
    // path should fire only in dev or on broken installs.
    match omni_host::share::moderation::default_model_path() {
        Some(path) => {
            if let Err(e) = omni_host::share::moderation::init_with_path(&path) {
                tracing::error!(
                    error = %e,
                    path = %path.display(),
                    "moderation init failed; uploads will degrade to skipped content-safety checks"
                );
            } else {
                tracing::info!(path = %path.display(), "moderation initialized");
            }
        }
        None => {
            tracing::warn!(
                "nudenet.onnx not found in installed or dev layout; moderation disabled (custom-image upload will return NotInitialized; pack-time content-safety will Skip)"
            );
        }
    }

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

    let mut latest_snapshot = shared::SensorSnapshot::default();

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
    // Per-PID ETW failure state: last failure timestamp + next backoff.
    // Backoff doubles on each consecutive failure up to ETW_RETRY_MAX.
    let mut etw_failed: std::collections::HashMap<u32, (Instant, Duration)> =
        std::collections::HashMap::new();

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

    // Size the bitmap shared memory for the largest connected monitor's
    // pixel area. Fallback to the primary monitor's area if enumeration
    // returns nothing (rare — empty desktop session). The host is DPI-aware
    // (PerMonitorV2 in resources/app.manifest), so monitor rects are reported
    // in physical pixels.
    let max_area = {
        let enumerated = max_single_monitor_pixel_area();
        if enumerated > 0 {
            enumerated
        } else {
            let (pw, ph) = primary_monitor_size();
            (pw as u64) * (ph as u64)
        }
    };
    let pixel_capacity_bytes = (max_area as usize)
        .checked_mul(shared::BPP as usize)
        .expect("bitmap pixel capacity overflowed usize");
    info!(
        max_area_pixels = max_area,
        capacity_bytes = pixel_capacity_bytes,
        "Sizing bitmap SHM for largest connected monitor"
    );
    let mut bitmap_writer = ipc::BitmapWriter::create(pixel_capacity_bytes)
        .expect("Failed to create bitmap shared memory");

    // Determine the resources directory (next to the exe, where build.rs copies it)
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let (initial_w, initial_h) = primary_monitor_size();

    // Per-overlay DPI scale state. `current_dpi_config` mirrors
    // `host.omni_file.dpi_scale` so the per-frame Auto tracker can
    // short-circuit without reading host. `current_dpi_scale` is the
    // resolved f64 last passed to UlRenderer (or its `recreate_view`).
    // Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
    let mut current_dpi_config: Option<omni_host::omni::types::DpiScale> = host.omni_file.dpi_scale;
    let mut current_dpi_scale: Option<f64> =
        resolve_dpi_scale(current_dpi_config, scanner_instance.last_game_hwnd());

    // `recreate_dims` returns the dimensions to pass to `recreate_view`:
    // the current viewport size when the renderer has been resized at
    // least once, otherwise the startup primary-monitor fallback. Used at
    // every overlay-state mutator and the per-frame Auto tracker.
    let recreate_dims = |viewport_w: u32, viewport_h: u32| -> (u32, u32) {
        (
            if viewport_w > 0 {
                viewport_w
            } else {
                initial_w
            },
            if viewport_h > 0 {
                viewport_h
            } else {
                initial_h
            },
        )
    };

    let mut ul = ul_renderer::UlRenderer::init(initial_w, initial_h, current_dpi_scale, &exe_dir)
        .expect("Failed to initialize Ultralight renderer");

    // Thumbnail render channel. The `share::thumbnail` pipeline cannot
    // spawn a second Ultralight renderer in this process (architectural
    // invariant #24 — Ultralight's C API has global state that crashes
    // on multi-instance use), so it sends requests here and the main
    // render loop services them on-thread between live ticks.
    let (thumb_tx, mut thumb_rx) =
        tokio::sync::mpsc::unbounded_channel::<ul_renderer::ThumbnailRequest>();
    ul_renderer::install_thumbnail_channel(thumb_tx);

    let mut ul_viewport_w: u32 = 0;
    let mut ul_viewport_h: u32 = 0;
    let mut ul_needs_reload = false;

    // Load initial HTML into Ultralight (styles + body + omniUpdate JS function)
    {
        let (hwinfo_values, hwinfo_units) = ws_state.hwinfo_values_and_units();
        let initial = html_builder::build_initial_html(
            &host.omni_file,
            &latest_snapshot,
            initial_w,
            initial_h,
            &data_dir,
            &host.current_overlay,
            &hwinfo_values,
            &hwinfo_units,
            &host.sensor_history,
            current_dpi_scale,
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
                if etw_captures.contains_key(&pid) {
                    continue;
                }
                // If this PID failed recently, back off before retrying.
                if !should_retry_etw(pid, &etw_failed) {
                    continue;
                }
                match etw::EtwCapture::start(pid) {
                    Ok(capture) => {
                        info!(pid, "Started ETW frame capture");
                        etw_captures.insert(pid, capture);
                        etw_failed.remove(&pid);
                    }
                    Err(e) => {
                        let next_backoff = etw_failed
                            .get(&pid)
                            .map(|(_, prev)| (*prev * 2).min(ETW_RETRY_MAX))
                            .unwrap_or(ETW_RETRY_INITIAL);
                        warn!(
                            pid,
                            error = %e,
                            retry_in_secs = next_backoff.as_secs(),
                            "Failed to start ETW capture — will retry"
                        );
                        etw_failed.insert(pid, (Instant::now(), next_backoff));
                    }
                }
            }

            // Clean up ETW sessions and failure tracking for exited processes
            etw_captures.retain(|pid, _| scanner_instance.is_tracked(*pid));
            etw_failed.retain(|pid, _| scanner_instance.is_tracked(*pid));

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
                let (rw, rh) = recreate_dims(ul_viewport_w, ul_viewport_h);
                maybe_recreate_for_scale_change(
                    &mut ul,
                    &mut current_dpi_config,
                    &mut current_dpi_scale,
                    &mut ul_needs_reload,
                    host.omni_file.dpi_scale,
                    scanner_instance.last_game_hwnd(),
                    rw,
                    rh,
                );
            }
        }

        // Drain the poller's snapshot channel. Each delivered snapshot represents
        // one poll cycle, so push samples into the chart history ONCE per snapshot
        // (not once per main-loop iteration). This keeps the 60-sample buffer
        // aligned with sensor poll intervals — at the default 1 Hz, 60 samples
        // is 60 seconds of history.
        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;

            // Collecting to owned Strings here avoids a mutable/immutable
            // borrow conflict — we iterate registered paths then push_sample
            // mutates the same struct.
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

        // Merge ETW frame metrics for the most recently spawned external overlay.
        // Mutates only the timing fields; `render_width`/`render_height` are
        // populated by the `GetClientRect` path below and must not be reset.
        if let Some(last_pid) = scanner_instance.last_external_pid() {
            if let Some(capture) = etw_captures.get(&last_pid) {
                let etw_metrics = capture.latest_metrics();
                if etw_metrics.available {
                    etw_metrics.merge_into(&mut latest_snapshot.frame);
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
            let (rw, rh) = recreate_dims(ul_viewport_w, ul_viewport_h);
            maybe_recreate_for_scale_change(
                &mut ul,
                &mut current_dpi_config,
                &mut current_dpi_scale,
                &mut ul_needs_reload,
                host.omni_file.dpi_scale,
                scanner_instance.last_game_hwnd(),
                rw,
                rh,
            );
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
                    let (rw, rh) = recreate_dims(ul_viewport_w, ul_viewport_h);
                    maybe_recreate_for_scale_change(
                        &mut ul,
                        &mut current_dpi_config,
                        &mut current_dpi_scale,
                        &mut ul_needs_reload,
                        host.omni_file.dpi_scale,
                        scanner_instance.last_game_hwnd(),
                        rw,
                        rh,
                    );
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
                        let (rw, rh) = recreate_dims(ul_viewport_w, ul_viewport_h);
                        maybe_recreate_for_scale_change(
                            &mut ul,
                            &mut current_dpi_config,
                            &mut current_dpi_scale,
                            &mut ul_needs_reload,
                            host.omni_file.dpi_scale,
                            scanner_instance.last_game_hwnd(),
                            rw,
                            rh,
                        );
                    }

                    scanner_instance.set_config(new_config.clone());

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

        // Per-frame DPI auto-tracking: when the active overlay declares
        // `<dpi-scale value="auto"/>`, the resolved scale follows the current
        // game window's monitor. The view is recreated when the resolved DPI
        // diverges from what's currently applied. Manual and `None` branches
        // skip this block (manual is constant; `None` is opt-out).
        //
        // Cost is one `GetDpiForWindow` (or `GetDpiForMonitor` fallback) per
        // frame — microseconds. Recreations only happen on actual divergence.
        // Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
        if matches!(
            current_dpi_config,
            Some(omni_host::omni::types::DpiScale::Auto)
        ) {
            let new_scale =
                resolve_dpi_scale(current_dpi_config, scanner_instance.last_game_hwnd());
            if new_scale != current_dpi_scale {
                let (recreate_w, recreate_h) = recreate_dims(ul_viewport_w, ul_viewport_h);
                match ul.recreate_view(recreate_w, recreate_h, new_scale) {
                    Ok(()) => {
                        info!(
                            ?new_scale,
                            "DPI scale auto-updated (game monitor changed); view recreated"
                        );
                        current_dpi_scale = new_scale;
                        ul_needs_reload = true;
                    }
                    Err(e) => {
                        warn!(error = %e, "auto DPI view recreation failed; renderer rolled back");
                        // Mirror the rollback (no-scale geometry) and clear
                        // the Auto config so this block stops firing for the
                        // failed scale every frame. The next overlay mutator
                        // re-reads host.omni_file.dpi_scale and tries again.
                        current_dpi_config = None;
                        current_dpi_scale = None;
                    }
                }
            }
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
                initial_w
            };
            let vh = if latest_snapshot.frame.render_height > 0 {
                latest_snapshot.frame.render_height
            } else {
                initial_h
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
                current_dpi_scale,
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

        // Drain any pending preview theme override written by
        // `ThemeSwapImpl::apply`/`revert` from the tokio-side preview
        // lifecycle. Emits `__omni_set_theme({…})` on this thread (Ultralight
        // `evaluate_script` requires the main render thread — see
        // `share::preview_impl` module doc). Ordered BEFORE `__omni_update`
        // so sensor-value updates in the same frame see the new custom
        // properties, matching the "theme change first, then redraw" mental
        // model.
        if let Some(theme_js) =
            omni_host::share::preview_impl::drain_pending_js(&pending_theme_slot)
        {
            ul.evaluate_script(&theme_js);
        }

        // Push raw sensor values via the bootstrap's __omni_update.
        let values =
            html_builder::collect_sensor_values(&host.omni_file, &latest_snapshot, &hwinfo_values);
        if !values.is_empty() {
            ul.evaluate_script(&html_builder::format_values_js(&values));
        }

        // Push conditional-class updates for expressions too rich for threshold attrs.
        let class_diff = html_builder::compute_update_diff(
            &host.omni_file,
            &latest_snapshot,
            &hwinfo_values,
            &hwinfo_units,
            &host.sensor_history,
        );
        let class_js = class_diff
            .as_ref()
            .and_then(html_builder::format_classes_js);
        if let Some(js) = &class_js {
            ul.evaluate_script(js);
        }

        // Push text updates for function-call interpolations (e.g. chart Y-axis
        // labels like {chart_y_max(sensor)}). Simple {sensor.path} placeholders
        // flow through __omni_update(values) via data-sensor spans instead.
        let text_js = class_diff.as_ref().and_then(html_builder::format_text_js);
        if let Some(js) = &text_js {
            ul.evaluate_script(js);
        }

        // Push per-element attribute updates (chart SVG width/points/d, etc.).
        let attrs_js = class_diff.as_ref().and_then(html_builder::format_attrs_js);
        if let Some(js) = &attrs_js {
            ul.evaluate_script(js);
        }

        // Preview subscribers keep receiving the class diff; values are also
        // included so the Nextron editor can display live sensor readings.
        let subs = ws_state.preview_subscribers.lock().unwrap();
        if !subs.is_empty() {
            drop(subs);
            let preview_msg = omni::preview::build_preview_payload(&values, class_diff.as_ref());
            ws_server::broadcast_preview(&ws_state, &preview_msg.to_string());
        }
        ul.update_and_render();
        ul.with_pixels(|w, h, rb, pixels, dirty| {
            bitmap_writer.write(w, h, rb, pixels, dirty);
        });

        debug!("Ultralight frame rendered");

        // Drain any pending thumbnail render requests. Each request
        // temporarily remounts the live view to render the thumbnail
        // overlay; the live overlay is restored before returning. See
        // `ul_renderer::render_thumbnail_to_png` for the save/restore
        // sequence. During the render the live preview briefly freezes
        // — acceptable because thumbnails are only generated on
        // user-initiated upload.
        while let Ok(req) = thumb_rx.try_recv() {
            let ul_renderer::ThumbnailRequest {
                overlay_root,
                html,
                sample_values,
                reply,
            } = req;
            let result = ul.render_thumbnail_to_png(&overlay_root, &html, &sample_values);
            if let Err(e) = &result {
                tracing::error!(error = %e, "thumbnail render failed");
            }
            let _ = reply.send(result);
        }

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

/// Returns true if `pid` should be attempted. Either no prior failure is
/// recorded, or the per-PID backoff (recorded alongside the failure
/// timestamp) has elapsed since the last recorded failure.
fn should_retry_etw(
    pid: u32,
    failures: &std::collections::HashMap<u32, (Instant, Duration)>,
) -> bool {
    match failures.get(&pid) {
        None => true,
        Some((last, backoff)) => last.elapsed() >= *backoff,
    }
}

/// Error kinds surfaced during [`build_share_context`]. Variants carve along
/// domain semantics per invariant #19a — third-party error types ride in the
/// `#[source]` chain for diagnostics, not as the public variant shape.
#[derive(Debug, thiserror::Error)]
enum BuildShareCtxError {
    #[error("worker URL invalid: {0}")]
    WorkerUrl(#[source] url::ParseError),
    #[error("identity load failed: {0}")]
    IdentityLoad(#[source] identity::IdentityError),
    #[error("guard init failed: {0}")]
    GuardInit(#[source] omni_guard_trait::GuardError),
    #[error("tofu store load failed: {0}")]
    TofuLoad(#[source] identity::IdentityError),
    #[error("registry load failed: {0}")]
    RegistryLoad(#[source] omni_host::share::registry::RegistryError),
}

/// Load the baseline theme CSS for an overlay at host startup. Used to seed
/// `ThemeSwapImpl::new` so `ThemeSwap::snapshot` / `revert` restore the
/// overlay's starting appearance after a preview session ends.
///
/// Mirrors the resolution logic in `omni::html_builder::load_theme_css`
/// (overlay-local first, shared `themes/` folder second). Missing `.omni`
/// files, missing `theme_src`, and unresolved theme paths all collapse to
/// an empty byte vector — the result is that `revert` is a no-op, which is
/// harmless but suppresses restoration. This is acceptable because preview
/// sessions are transient and the next overlay reload re-applies the
/// baseline anyway.
fn load_baseline_theme_css(data_dir: &std::path::Path, overlay_name: &str) -> Vec<u8> {
    let omni_path = workspace::structure::overlay_omni_path(data_dir, overlay_name);
    let Ok(omni_src) = std::fs::read_to_string(&omni_path) else {
        return Vec::new();
    };
    let (Some(parsed), _diag) = omni::parser::parse_omni_with_diagnostics(&omni_src) else {
        return Vec::new();
    };
    let Some(theme_src) = parsed.theme_src.as_deref() else {
        return Vec::new();
    };
    let Some(theme_path) =
        workspace::structure::resolve_theme_path(data_dir, overlay_name, theme_src)
    else {
        return Vec::new();
    };
    std::fs::read(&theme_path).unwrap_or_default()
}

/// Construct the `ShareContext` bundle consumed by `explorer.*` + `upload.*`
/// + `identity.*` + `config.*` + `report.*` WS handlers.
///
/// Non-fatal on failure — caller logs and the WS surface falls back to the
/// `service_unavailable` envelope per spec #021 §6.
///
/// Design notes:
/// - Sync function. Avoids spinning a second tokio runtime just for startup;
///   `BundleLimits::DEFAULT` is the conservative startup value per
///   `bundle::BundleLimits` doc. A periodic refresh driven from the
///   existing `share_runtime` is out of scope for this wave (plan #021 §6).
/// - Worker URL comes from `OMNI_WORKER_URL`; dev fallback matches the
///   wrangler default documented in `services/omni-themes-worker/README.md`.
/// - `theme_swap` is a `ThemeSwapImpl` that writes to the shared
///   `pending_theme_slot`; the main render loop drains the slot and emits
///   `__omni_set_theme(...)` via `UlRenderer::evaluate_script` (phase-2
///   followup #3 — replaces the prior `NoopThemeSwap`).
fn build_share_context(
    state: &ws_server::WsSharedState,
    overlay_name: &str,
    pending_theme_slot: omni_host::share::preview_impl::PendingSlot,
) -> Result<omni_host::share::ws_messages::ShareContext, BuildShareCtxError> {
    use arc_swap::ArcSwap;
    use omni_host::share::client::ShareClient;
    use omni_host::share::preview::{PreviewSlot, ThemeSwap};
    use omni_host::share::preview_impl::ThemeSwapImpl;
    use omni_host::share::registry::{RegistryHandle, RegistryKind};
    use omni_host::share::tofu::TofuStore;
    use omni_host::share::ws_messages::ShareContext;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    let worker_url_str =
        std::env::var("OMNI_WORKER_URL").unwrap_or_else(|_| "http://127.0.0.1:8787/".to_string());
    let worker_url = url::Url::parse(&worker_url_str).map_err(BuildShareCtxError::WorkerUrl)?;

    // `OMNI_IDENTITY_PATH` overrides the default for dev loops (see
    // docs/superpowers/specs/2026-04-18-local-dev-worker-design.md §3.2).
    // Production users don't set the var; the fallback is the shipped prod path.
    fn resolve_identity_path(
        data_dir: &std::path::Path,
        env_value: Option<&str>,
    ) -> std::path::PathBuf {
        match env_value {
            Some(s) if !s.is_empty() => std::path::PathBuf::from(s),
            _ => data_dir.join("identity.key"),
        }
    }
    let env_override = std::env::var("OMNI_IDENTITY_PATH").ok();
    let identity_path = resolve_identity_path(&state.data_dir, env_override.as_deref());
    // Wrap the loaded keypair in `Arc<ArcSwap<...>>` exactly once. Both
    // `ShareContext.identity` and the embedded `ShareClient.identity` clone
    // this same outer Arc — they observe the same swap slot, so a future
    // identity-rotate call (`ctx.identity.store(new_kp)`) atomically
    // retargets every signer in the host without re-threading the keypair.
    let identity = Arc::new(ArcSwap::new(Arc::new(
        identity::Keypair::load_or_create(&identity_path)
            .map_err(BuildShareCtxError::IdentityLoad)?,
    )));

    // `make_guard()` returns `Box<dyn Guard>`; convert to Arc for ShareContext.
    let guard_box = omni_host::guard::make_guard().map_err(BuildShareCtxError::GuardInit)?;
    let guard: Arc<dyn omni_guard_trait::Guard> = Arc::from(guard_box);

    let client = Arc::new(ShareClient::new(
        worker_url,
        identity.clone(),
        guard.clone(),
    ));

    let tofu = Arc::new(Mutex::new(
        TofuStore::open(&state.data_dir).map_err(BuildShareCtxError::TofuLoad)?,
    ));
    let bundles_registry = Arc::new(Mutex::new(
        RegistryHandle::load(&state.data_dir, RegistryKind::Bundles)
            .map_err(BuildShareCtxError::RegistryLoad)?,
    ));
    let themes_registry = Arc::new(Mutex::new(
        RegistryHandle::load(&state.data_dir, RegistryKind::Themes)
            .map_err(BuildShareCtxError::RegistryLoad)?,
    ));

    // Conservative startup value; Worker `/v1/config/limits` refresh is out
    // of scope for this wave per spec #021 §6.
    let limits = Arc::new(Mutex::new(bundle::BundleLimits::DEFAULT));

    // `CARGO_PKG_VERSION` is the crate's own Cargo.toml `version` field,
    // which the workspace guarantees is valid semver. An invalid value would
    // fail the build, so the expect below cannot fire at runtime.
    let current_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("CARGO_PKG_VERSION is valid semver (compile-time guarantee)");

    let preview_slot = Arc::new(PreviewSlot::new());
    let cancel_registry = Arc::new(Mutex::new(HashMap::new()));

    // Seed `ThemeSwapImpl` with the current overlay's theme CSS so
    // `snapshot()` → `revert()` restores the baseline. A missing/unresolved
    // theme returns empty bytes (see `load_baseline_theme_css`), which makes
    // revert a no-op but does not fail preview start — consistent with the
    // "preview never touches disk" contract in `share::preview`.
    let baseline_css = load_baseline_theme_css(&state.data_dir, overlay_name);
    let theme_swap: Arc<dyn ThemeSwap> =
        Arc::new(ThemeSwapImpl::new(baseline_css, pending_theme_slot));

    Ok(ShareContext {
        identity,
        guard,
        client,
        tofu,
        bundles_registry,
        themes_registry,
        limits,
        current_version,
        preview_slot,
        cancel_registry,
        theme_swap,
        data_dir: state.data_dir.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::should_retry_etw;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[test]
    fn should_retry_when_no_prior_failure() {
        let failures: HashMap<u32, (Instant, Duration)> = HashMap::new();
        assert!(should_retry_etw(1234, &failures));
    }

    #[test]
    fn should_not_retry_within_backoff() {
        let mut failures: HashMap<u32, (Instant, Duration)> = HashMap::new();
        failures.insert(1234, (Instant::now(), Duration::from_secs(5)));
        assert!(!should_retry_etw(1234, &failures));
    }

    #[test]
    fn should_retry_after_backoff_elapsed() {
        // Use a 10 ms backoff and sleep past it — avoids the `checked_sub`
        // edge case where fresh boots have Instant::now() < arbitrary
        // historical Duration, which would make a "long ago" construction
        // fall back to `now` and break the test.
        let mut failures: HashMap<u32, (Instant, Duration)> = HashMap::new();
        failures.insert(1234, (Instant::now(), Duration::from_millis(10)));
        std::thread::sleep(Duration::from_millis(20));
        assert!(should_retry_etw(1234, &failures));
    }
}

#[cfg(test)]
mod identity_path_env_tests {
    use std::path::PathBuf;

    /// Pure helper that mirrors the main.rs identity-path resolution.
    /// Keeping the env-var read isolated in a function lets the test assert
    /// the precedence without spinning up a full ShareContext.
    fn resolve_identity_path(data_dir: &std::path::Path, env_value: Option<&str>) -> PathBuf {
        match env_value {
            Some(s) if !s.is_empty() => PathBuf::from(s),
            _ => data_dir.join("identity.key"),
        }
    }

    #[test]
    fn env_override_wins_when_set() {
        let got = resolve_identity_path(std::path::Path::new("/data"), Some("/dev/custom.key"));
        assert_eq!(got, PathBuf::from("/dev/custom.key"));
    }

    #[test]
    fn falls_back_to_data_dir_when_unset() {
        let got = resolve_identity_path(std::path::Path::new("/data"), None);
        assert_eq!(got, PathBuf::from("/data/identity.key"));
    }

    #[test]
    fn falls_back_to_data_dir_when_empty() {
        let got = resolve_identity_path(std::path::Path::new("/data"), Some(""));
        assert_eq!(got, PathBuf::from("/data/identity.key"));
    }
}
