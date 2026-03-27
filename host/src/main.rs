use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod injector;
mod config;

fn main() {
    // Initialize tracing with file + console output
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: omni-host <PID> <DLL_PATH>");
        eprintln!("  PID       - Process ID of the target game");
        eprintln!("  DLL_PATH  - Absolute path to omni_overlay_dll.dll");
        std::process::exit(1);
    }

    let pid: u32 = args[1].parse().unwrap_or_else(|_| {
        error!("Invalid PID: {}", args[1]);
        std::process::exit(1);
    });

    let dll_path = &args[2];

    if !std::path::Path::new(dll_path).exists() {
        error!(dll_path, "DLL file not found");
        std::process::exit(1);
    }

    info!(pid, dll_path, "Omni host starting — injecting overlay DLL");

    match injector::inject_dll(pid, dll_path) {
        Ok(()) => info!(pid, "DLL injection successful"),
        Err(e) => {
            error!(pid, error = %e, "DLL injection failed");
            std::process::exit(1);
        }
    }
}
