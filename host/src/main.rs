use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;

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

    if args[1] == "--stop" {
        run_stop();
    } else if args[1] == "--watch" {
        if args.len() < 3 {
            eprintln!("Usage: omni-host --watch <DLL_PATH>");
            std::process::exit(1);
        }
        validate_dll_path(&args[2]);
        run_watch_mode(&args[2]);
    } else {
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

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  omni-host <PID> <DLL_PATH>      Inject once into a specific process");
    eprintln!("  omni-host --watch <DLL_PATH>     Watch for new games and auto-inject");
    eprintln!("  omni-host --stop                 Stop all running omni-host instances");
}

fn validate_dll_path(dll_path: &str) {
    if !Path::new(dll_path).exists() {
        error!(dll_path, "DLL file not found");
        std::process::exit(1);
    }
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
    let dll_name = "omni_overlay_dll.dll";

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

        // Check if this process has our DLL loaded.
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
            Err(_) => {} // access denied, skip
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

fn run_watch_mode(dll_path: &str) {
    let config_path = config::config_path();
    let config = config::load_config(&config_path);
    let poll_interval = Duration::from_millis(config.poll_interval_ms);

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

    // Start sensor polling on background thread
    let sensor_running = std::sync::Arc::new(AtomicBool::new(true));
    let (mut sensor_poller, sensor_rx) = sensors::SensorPoller::start(
        Duration::from_millis(1000),
        sensor_running,
    );

    info!(
        dll_path,
        config_path = ?config_path,
        poll_ms = config.poll_interval_ms,
        exclude_count = config.exclude.len(),
        "Omni host starting in watch mode"
    );
    info!("Press Ctrl+C to stop");

    let mut scanner = scanner::Scanner::new(dll_path.to_string(), config);
    let mut latest_snapshot = omni_shared::SensorSnapshot::default();

    while RUNNING.load(Ordering::Relaxed) {
        scanner.poll();

        // Drain sensor channel — keep only the latest snapshot
        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;
        }

        // Build hardcoded CPU usage widget
        let widget = build_cpu_widget(&latest_snapshot);

        // Write to shared memory
        shm_writer.write(&latest_snapshot, &[widget], 1);

        std::thread::sleep(poll_interval);
    }

    info!("Shutting down — ejecting DLLs from injected processes");
    scanner.eject_all();
    sensor_poller.stop();
    info!("Omni host stopped");
}

/// Build a hardcoded CPU usage widget for this phase.
/// In later phases this is replaced by .widget file parsing + layout engine.
fn build_cpu_widget(snapshot: &omni_shared::SensorSnapshot) -> omni_shared::ComputedWidget {
    let mut widget = omni_shared::ComputedWidget::default();
    widget.widget_type = omni_shared::WidgetType::SensorValue;
    widget.source = omni_shared::SensorSource::CpuUsage;
    widget.x = 20.0;
    widget.y = 20.0;
    widget.width = 200.0;
    widget.height = 32.0;
    widget.font_size = 18.0;
    widget.font_weight = 700;
    widget.color_rgba = [255, 255, 255, 255]; // white text
    widget.bg_color_rgba = [20, 20, 20, 180]; // dark semi-transparent bg
    widget.border_radius = [4.0; 4];
    widget.opacity = 1.0;

    let text = format!("CPU: {:.0}%", snapshot.cpu.total_usage_percent);
    omni_shared::write_fixed_str(&mut widget.format_pattern, &text);

    widget
}
