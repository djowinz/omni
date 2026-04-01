use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

struct HostState {
    omni_file: omni::OmniFile,
    omni_resolver: omni::resolver::OmniResolver,
    layout_version: u64,
    current_overlay: String,
    file_watcher: Option<watcher::FileWatcher>,
    data_dir: PathBuf,
    config_path: PathBuf,
}

impl HostState {
    fn new(overlay_name: String, data_dir: PathBuf, config_path: PathBuf) -> Self {
        Self {
            omni_file: omni::OmniFile::empty(),
            omni_resolver: omni::resolver::OmniResolver::new(),
            layout_version: 0,
            current_overlay: overlay_name,
            file_watcher: None,
            data_dir,
            config_path,
        }
    }

    /// Load and apply a theme CSS file for the current overlay.
    fn reload_theme(&mut self, theme_src: &str) {
        if let Some(theme_path) = workspace::structure::resolve_theme_path(
            &self.data_dir,
            &self.current_overlay,
            theme_src,
        ) {
            match std::fs::read_to_string(&theme_path) {
                Ok(css) => self.omni_resolver.load_theme(&css),
                Err(e) => warn!(path = %theme_path.display(), error = %e, "Failed to read theme file"),
            }
        } else {
            warn!(theme_src, "Theme file not found");
        }
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
                // Clone theme_src before moving new_file to avoid borrow conflict
                let theme_src = new_file.theme_src.clone();
                info!(widgets = new_file.widgets.len(), "Overlay loaded successfully");
                self.omni_file = new_file;
                self.layout_version += 1;
                if let Some(ts) = theme_src {
                    self.reload_theme(&ts);
                }
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

mod config;
mod error;
mod injector;
mod ipc;
mod omni;
mod scanner;
mod sensors;
mod watcher;
pub(crate) mod win32;
mod workspace;
mod ws_server;

static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "--stop" => run_stop(),
        "--service" => {
            let dll_path = discover_dll_path();
            run_host(&dll_path);
        }
        "--watch" => {
            if args.len() < 3 {
                eprintln!("Usage: omni-host --watch <DLL_PATH>");
                std::process::exit(1);
            }
            validate_dll_path(&args[2]);
            run_host(&args[2]);
        }
        _ => {
            if args.len() < 3 {
                print_usage();
                std::process::exit(1);
            }
            let pid: u32 = args[1].parse().unwrap_or_else(|_| {
                error!("Invalid PID: {}", args[1]);
                std::process::exit(1);
            });
            validate_dll_path(&args[2]);
            run_inject_once(pid, &args[2]);
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  omni-host <PID> <DLL_PATH>      Inject once into a specific process");
    eprintln!("  omni-host --watch <DLL_PATH>     Watch for new games and auto-inject");
    eprintln!("  omni-host --service              Service mode (auto-discover DLL, WebSocket API)");
    eprintln!("  omni-host --stop                 Stop all running omni-host instances");
}

fn validate_dll_path(dll_path: &str) {
    if !Path::new(dll_path).exists() {
        error!(dll_path, "DLL file not found");
        std::process::exit(1);
    }
}

/// Discover the overlay DLL path relative to the executable.
/// Resolution order:
/// 1. overlay/omni_overlay.dll (installed layout)
/// 2. target/debug/omni_overlay.dll (dev layout, relative to workspace root)
/// 3. target/release/omni_overlay.dll (dev layout, release build)
fn discover_dll_path() -> String {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // Installed layout: overlay/omni_overlay.dll next to the exe
    let installed = exe_dir.join("overlay").join("omni_overlay.dll");
    if installed.exists() {
        info!(path = %installed.display(), "DLL found (installed layout)");
        return installed.to_string_lossy().into_owned();
    }

    // Dev layout: look for target/debug or target/release relative to workspace root
    // The exe is typically at target/debug/omni-host.exe, so workspace root is ../../
    let workspace_root = exe_dir.parent().and_then(|p| p.parent());
    if let Some(root) = workspace_root {
        let debug_path = root.join("target").join("debug").join("omni_overlay.dll");
        if debug_path.exists() {
            info!(path = %debug_path.display(), "DLL found (dev debug layout)");
            return debug_path.to_string_lossy().into_owned();
        }

        let release_path = root.join("target").join("release").join("omni_overlay.dll");
        if release_path.exists() {
            info!(path = %release_path.display(), "DLL found (dev release layout)");
            return release_path.to_string_lossy().into_owned();
        }
    }

    error!("Could not find omni_overlay.dll. Searched:");
    error!("  {}", installed.display());
    if let Some(root) = workspace_root {
        error!("  {}", root.join("target/debug/omni_overlay.dll").display());
        error!(
            "  {}",
            root.join("target/release/omni_overlay.dll").display()
        );
    }
    error!("Use --watch <DLL_PATH> to specify the path manually.");
    std::process::exit(1);
}

fn run_inject_once(pid: u32, dll_path: &str) {
    info!(pid, dll_path, "Omni host starting — injecting overlay DLL");
    match injector::inject_dll(pid, dll_path) {
        Ok(()) => info!(pid, "DLL injection successful"),
        Err(e) => {
            error!(pid, error = %e, "DLL injection failed");
            std::process::exit(1);
        }
    }
}

fn run_stop() {
    let my_pid = std::process::id();
    let dll_name = "omni_overlay.dll";

    let processes = match win32::iter_processes() {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "Failed to enumerate processes");
            std::process::exit(1);
        }
    };

    // First, eject the DLL from any process that has it loaded.
    let mut ejected = 0u32;
    for entry in &processes {
        let pid = entry.th32ProcessID;
        if pid <= 4 {
            continue;
        }

        match win32::has_module(pid, dll_name) {
            Ok(true) => {
                info!(pid, "Ejecting overlay DLL");
                match injector::eject_dll(pid, dll_name) {
                    Ok(()) => {
                        info!(pid, "Ejection successful");
                        ejected += 1;
                    }
                    Err(e) => error!(pid, error = %e, "Ejection failed"),
                }
            }
            Ok(false) => {}
            Err(_) => {}
        }
    }

    if ejected == 0 {
        info!("No processes had the overlay DLL loaded");
    } else {
        info!(ejected, "Ejected overlay DLL from processes");
    }

    // Then, kill any running omni-host instances.
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
                    let _ = TerminateProcess(handle.raw(), 0);
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
fn run_host(dll_path: &str) {
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

    // Create shared memory for IPC with overlay DLL
    let mut shm_writer = match ipc::SharedMemoryWriter::create() {
        Ok(w) => w,
        Err(e) => {
            error!(error = %e, "Failed to create shared memory");
            std::process::exit(1);
        }
    };

    // Shared state for WebSocket server
    let ws_state = Arc::new(ws_server::WsSharedState::new(data_dir.clone()));

    // Start WebSocket server
    let ws_handle = ws_server::start(ws_state.clone());

    let sensor_running = std::sync::Arc::new(AtomicBool::new(true));

    info!(
        dll_path,
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

    let mut scanner_instance = scanner::Scanner::new(dll_path.to_string(), config.clone());

    let mut host = HostState::new(overlay_name, data_dir.clone(), config_path.clone());

    // Load the initial overlay
    host.reload_overlay();

    // Start sensor polling on background thread (uses poll_config from .omni file)
    let (mut sensor_poller, sensor_rx) =
        sensors::SensorPoller::start(host.omni_file.poll_config.clone(), sensor_running);

    // Start file watcher for hot-reload
    let current_overlay_dir =
        workspace::structure::overlay_dir(&host.data_dir, &host.current_overlay);
    let themes_dir = host.data_dir.join("themes");
    host.file_watcher =
        match watcher::FileWatcher::start(current_overlay_dir, themes_dir, host.config_path.clone())
        {
            Ok(w) => {
                info!("File watcher started for hot-reload");
                Some(w)
            }
            Err(e) => {
                warn!(error = %e, "Failed to start file watcher — hot-reload disabled");
                None
            }
        };

    let mut last_scan = Instant::now();
    let mut transitions_active = false;

    while RUNNING.load(Ordering::Relaxed) {
        if last_scan.elapsed() >= scan_interval {
            scanner_instance.poll();
            last_scan = Instant::now();

            // Re-resolve overlay based on current game
            let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                scanner_instance.last_injected_exe(),
                &config.overlay_by_game,
                &config.active_overlay,
                &data_dir,
            );

            if new_overlay != host.current_overlay {
                info!(
                    from = %host.current_overlay,
                    to = %new_overlay,
                    game = ?scanner_instance.last_injected_exe(),
                    "Game-specific overlay switch"
                );
                host.switch_overlay(&new_overlay);
            }
        }

        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;
        }

        // Read DLL frame data (FPS, frame time) from shared memory.
        // The DLL writes this each frame so the host can use it in
        // reactive class conditions (e.g., "fps > 60").
        let dll_frame = shm_writer.read_dll_frame_data();
        if dll_frame.available {
            latest_snapshot.frame = dll_frame;
        }

        // Update WebSocket shared state
        if let Ok(mut ws_snapshot) = ws_state.latest_snapshot.lock() {
            *ws_snapshot = latest_snapshot;
        }

        // Check for widget updates from WebSocket (Electron app)
        if let Ok(mut active) = ws_state.active_omni_file.lock() {
            if let Some(new_file) = active.take() {
                info!(
                    widget_count = new_file.widgets.len(),
                    enabled = new_file.widgets.iter().filter(|w| w.enabled).count(),
                    "Applied widget update from WebSocket"
                );
                let theme_src = new_file.theme_src.clone();
                host.omni_file = new_file;
                host.layout_version += 1;
                if let Some(ts) = theme_src {
                    host.reload_theme(&ts);
                }
            }
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
                }
                watcher::ReloadEvent::Theme => {
                    info!("Theme file changed — reloading");
                    if let Some(theme_src) = host.omni_file.theme_src.clone() {
                        host.reload_theme(&theme_src);
                        host.layout_version += 1;
                    }
                }
                watcher::ReloadEvent::Config => {
                    info!("Config changed — reloading");
                    let new_config = config::load_config(&host.config_path);

                    let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                        scanner_instance.last_injected_exe(),
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
                    }

                    config = new_config;
                }
            }
        }

        // Resolve widgets from .omni file
        let widgets = host.omni_resolver.resolve(&host.omni_file, &latest_snapshot);

        debug!(computed_widgets = widgets.len(), "Resolved overlay");

        // Write to shared memory
        shm_writer.write(&latest_snapshot, &widgets, host.layout_version);

        // Check if transitions need smooth updates
        transitions_active = host.omni_resolver.has_active_transitions();

        // Adaptive sleep: 120Hz during transitions, relaxed otherwise
        let timeout = if transitions_active {
            Duration::from_millis(8) // 120Hz for smooth animation
        } else {
            // Wake on scanner poll, watcher events, or sensor data
            let until_scan = scan_interval.saturating_sub(last_scan.elapsed());
            until_scan.min(Duration::from_millis(100))
        };

        // Block until sensor data arrives or timeout expires.
        // This is the key difference from thread::sleep — we process
        // sensor data immediately when it arrives instead of waiting.
        if let Ok(snapshot) = sensor_rx.recv_timeout(timeout) {
            latest_snapshot = snapshot;
        }
    }

    info!("Shutting down — ejecting DLLs from injected processes");
    scanner_instance.eject_all();
    sensor_poller.stop();
    ws_state.running.store(false, Ordering::Relaxed);
    let _ = ws_handle.join();
    info!("Omni host stopped");
}
