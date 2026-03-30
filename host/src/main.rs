use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn, error, debug};
use tracing_subscriber::EnvFilter;

mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;
mod ws_server;
mod omni;
mod workspace;

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
        error!("  {}", root.join("target/release/omni_overlay.dll").display());
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

    let processes = match scanner::enumerate_processes() {
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

        match scanner::has_module(pid, dll_name) {
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

        let name = scanner::wchar_to_string(&entry.szExeFile);
        if !name.eq_ignore_ascii_case("omni-host.exe") {
            continue;
        }

        info!(pid, "Terminating omni-host instance");
        unsafe {
            use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(handle, 0);
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                killed += 1;
            } else {
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
    let config = config::load_config(&config_path);
    let data_dir = config::data_dir();
    let poll_interval = Duration::from_millis(2000);

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

    // Start sensor polling on background thread
    let sensor_running = std::sync::Arc::new(AtomicBool::new(true));
    let (mut sensor_poller, sensor_rx) = sensors::SensorPoller::start(
        Duration::from_millis(1000),
        sensor_running,
    );
    info!("Sensor poller started, interval=1000ms");

    info!(
        dll_path,
        config_path = ?config_path,
        poll_ms = 2000,
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

    let mut scanner_instance = scanner::Scanner::new(dll_path.to_string(), config);

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

    while RUNNING.load(Ordering::Relaxed) {
        scanner_instance.poll();

        // Drain sensor channel — keep only the latest snapshot
        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;
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
                // Reload theme for the new file if it specifies one
                if let Some(theme_src) = &new_file.theme_src {
                    if let Some(theme_path) = workspace::structure::resolve_theme_path(&data_dir, &overlay_name, theme_src) {
                        if let Ok(css) = std::fs::read_to_string(&theme_path) {
                            omni_resolver.load_theme(&css);
                        }
                    }
                }
                omni_file = new_file;
            }
        }

        // Resolve widgets from .omni file
        let widgets = omni_resolver.resolve(&omni_file, &latest_snapshot);

        // Log widget count on changes (debug level to avoid spam)
        debug!(computed_widgets = widgets.len(), "Resolved overlay");

        // Write to shared memory
        shm_writer.write(&latest_snapshot, &widgets, 1);

        std::thread::sleep(poll_interval);
    }

    info!("Shutting down — ejecting DLLs from injected processes");
    scanner_instance.eject_all();
    sensor_poller.stop();
    ws_state.running.store(false, Ordering::Relaxed);
    let _ = ws_handle.join();
    info!("Omni host stopped");
}
