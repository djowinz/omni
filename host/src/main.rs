use std::path::Path;
use std::time::Duration;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod injector;
mod config;
mod scanner;

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

    if args[1] == "--watch" {
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

fn run_watch_mode(dll_path: &str) {
    let config_path = config::config_path();
    let config = config::load_config(&config_path);
    let poll_interval = Duration::from_millis(config.poll_interval_ms);

    info!(
        dll_path,
        config_path = ?config_path,
        poll_ms = config.poll_interval_ms,
        exclude_count = config.exclude.len(),
        "Omni host starting in watch mode"
    );
    info!("Press Ctrl+C to stop");

    let mut scanner = scanner::Scanner::new(dll_path.to_string(), config);
    loop {
        scanner.poll();
        std::thread::sleep(poll_interval);
    }
}
