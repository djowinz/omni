# Phase 9a-0: Host Service Infrastructure

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the host from a CLI-only tool into a service with a WebSocket API, add `--service` mode with DLL auto-discovery, and rename the overlay DLL from `omni_overlay_dll.dll` to `omni_overlay.dll`.

**Architecture:** The host gains a WebSocket server thread (tungstenite, sync) on localhost:9473. Both `--watch` and `--service` modes share a common `run_host` core. The `--service` mode auto-discovers the DLL relative to the executable. Sensor data is shared between the main loop and WebSocket thread via `Arc<Mutex<SensorSnapshot>>`.

**Tech Stack:** Rust, `tungstenite` (sync WebSocket), `serde_json` (message serialization).

**Testing notes:** WebSocket server tested with a client in unit tests. DLL path resolution tested with path logic. Full pipeline tested manually.

**Depends on:** Phases 1–8 complete.

---

## File Map

```
overlay-dll/
  Cargo.toml                         # Add name = "omni_overlay" to [lib]
  src/
    lib.rs                           # Update GetModuleHandleA to "omni_overlay.dll"

host/
  Cargo.toml                         # Add tungstenite dependency
  src/
    main.rs                          # Add --service mode, refactor to run_host, update DLL name refs
    injector/
      mod.rs                         # Update fallback DLL name constant
    scanner.rs                       # Update fallback DLL name constant
    ws_server.rs                     # NEW: WebSocket server thread + message handling
```

---

### Task 1: Rename DLL from omni_overlay_dll to omni_overlay

**Files:**
- Modify: `overlay-dll/Cargo.toml`
- Modify: `overlay-dll/src/lib.rs`
- Modify: `host/src/main.rs`
- Modify: `host/src/scanner.rs`
- Modify: `host/src/injector/mod.rs`

- [ ] **Step 1: Update overlay-dll/Cargo.toml**

Add `name = "omni_overlay"` to the `[lib]` section:

```toml
[lib]
name = "omni_overlay"
crate-type = ["cdylib"]
```

- [ ] **Step 2: Update overlay-dll/src/lib.rs**

Change the `GetModuleHandleA` call in `omni_shutdown`:

Replace:
```rust
    if let Ok(hmod) = GetModuleHandleA(windows::core::s!("omni_overlay_dll.dll")) {
```

With:
```rust
    if let Ok(hmod) = GetModuleHandleA(windows::core::s!("omni_overlay.dll")) {
```

- [ ] **Step 3: Update host/src/main.rs**

In `run_stop`, change the DLL name:

Replace:
```rust
    let dll_name = "omni_overlay_dll.dll";
```

With:
```rust
    let dll_name = "omni_overlay.dll";
```

- [ ] **Step 4: Update host/src/scanner.rs**

Change the fallback DLL name:

Replace:
```rust
            .unwrap_or("omni_overlay_dll.dll")
```

With:
```rust
            .unwrap_or("omni_overlay.dll")
```

- [ ] **Step 5: Update host/src/injector/mod.rs**

Change the fallback DLL name and doc comment:

Replace:
```rust
        .unwrap_or("omni_overlay_dll.dll");
```

With:
```rust
        .unwrap_or("omni_overlay.dll");
```

And update the doc comment:
Replace:
```rust
/// * `dll_name` - Filename of the DLL to eject (e.g. "omni_overlay_dll.dll")
```

With:
```rust
/// * `dll_name` - Filename of the DLL to eject (e.g. "omni_overlay.dll")
```

- [ ] **Step 6: Verify everything compiles and tests pass**

Run: `cargo test`
Expected: All tests pass. The DLL is now built as `omni_overlay.dll` in the target directory.

- [ ] **Step 7: Verify the DLL is correctly named**

Run: `cargo build -p omni-overlay-dll && ls target/debug/omni_overlay.dll`
Expected: File exists at `target/debug/omni_overlay.dll`.

- [ ] **Step 8: Commit**

```bash
git add overlay-dll/Cargo.toml overlay-dll/src/lib.rs host/src/main.rs host/src/scanner.rs host/src/injector/mod.rs
git commit -m "refactor: rename overlay DLL from omni_overlay_dll.dll to omni_overlay.dll"
```

---

### Task 2: Add tungstenite Dependency

**Files:**
- Modify: `host/Cargo.toml`

- [ ] **Step 1: Add tungstenite to dependencies**

Add `tungstenite = "0.26"` to the `[dependencies]` section:

```toml
[dependencies]
omni-shared = { path = "../shared" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ctrlc = "3"
sysinfo = "0.35"
wmi = "0.14"
tungstenite = "0.26"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Downloads `tungstenite`, compiles.

- [ ] **Step 3: Commit**

```bash
git add host/Cargo.toml Cargo.lock
git commit -m "feat(host): add tungstenite dependency for WebSocket server"
```

---

### Task 3: WebSocket Server Module

**Files:**
- Create: `host/src/ws_server.rs`
- Modify: `host/src/main.rs` (add `mod ws_server;`)

- [ ] **Step 1: Create host/src/ws_server.rs**

```rust
//! WebSocket server for Electron app communication.
//!
//! Runs on a dedicated thread, accepts one client at a time on localhost:9473.
//! Handles JSON messages with a "type" field for routing.
//! Shares sensor data with the main loop via Arc<Mutex<SensorSnapshot>>.

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use omni_shared::SensorSnapshot;
use serde_json::{json, Value};
use tracing::{info, warn, debug};
use tungstenite::{accept, Message, WebSocket};

pub const WS_PORT: u16 = 9473;

/// Shared state between the WebSocket server and the main loop.
pub struct WsSharedState {
    pub latest_snapshot: Mutex<SensorSnapshot>,
    pub running: AtomicBool,
}

impl WsSharedState {
    pub fn new() -> Self {
        Self {
            latest_snapshot: Mutex::new(SensorSnapshot::default()),
            running: AtomicBool::new(true),
        }
    }
}

/// Starts the WebSocket server on a background thread.
/// Returns a handle that can be used to signal shutdown.
pub fn start(state: Arc<WsSharedState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run_server(state);
    })
}

fn run_server(state: Arc<WsSharedState>) {
    let addr = format!("127.0.0.1:{}", WS_PORT);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => {
            info!(addr = %addr, "WebSocket server listening");
            l
        }
        Err(e) => {
            warn!(addr = %addr, error = %e, "WebSocket server failed to bind");
            return;
        }
    };

    // Non-blocking so we can check the running flag
    listener.set_nonblocking(true).ok();

    while state.running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                info!(client = %addr, "WebSocket client connected");
                stream.set_nonblocking(false).ok();
                stream.set_read_timeout(Some(Duration::from_millis(100))).ok();
                handle_client(stream, &state);
                info!("WebSocket client disconnected");
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No incoming connection — sleep briefly and retry
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                debug!(error = %e, "WebSocket accept error");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    info!("WebSocket server stopped");
}

fn handle_client(stream: TcpStream, state: &Arc<WsSharedState>) {
    let mut ws = match accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            warn!(error = %e, "WebSocket handshake failed");
            return;
        }
    };

    let mut sensor_subscribed = false;
    let mut last_sensor_send = std::time::Instant::now();

    while state.running.load(Ordering::Relaxed) {
        // Read incoming messages (non-blocking via read timeout)
        match ws.read() {
            Ok(msg) => {
                match msg {
                    Message::Text(text) => {
                        if let Some(response) = handle_message(&text, state, &mut sensor_subscribed) {
                            if ws.send(Message::Text(response)).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(data) => {
                        if ws.send(Message::Pong(data)).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Timeout — no message, continue
            }
            Err(_) => break, // Connection error
        }

        // Push sensor data if subscribed (every 1 second)
        if sensor_subscribed && last_sensor_send.elapsed() >= Duration::from_secs(1) {
            let snapshot = state.latest_snapshot.lock().unwrap().clone();
            let msg = json!({
                "type": "sensors.data",
                "snapshot": {
                    "timestamp_ms": snapshot.timestamp_ms,
                    "cpu": {
                        "total_usage_percent": snapshot.cpu.total_usage_percent,
                        "core_count": snapshot.cpu.core_count,
                        "package_temp_c": format_f32(snapshot.cpu.package_temp_c),
                    },
                    "gpu": {
                        "usage_percent": snapshot.gpu.usage_percent,
                        "temp_c": format_f32(snapshot.gpu.temp_c),
                        "core_clock_mhz": snapshot.gpu.core_clock_mhz,
                        "mem_clock_mhz": snapshot.gpu.mem_clock_mhz,
                        "vram_used_mb": snapshot.gpu.vram_used_mb,
                        "vram_total_mb": snapshot.gpu.vram_total_mb,
                        "fan_speed_percent": snapshot.gpu.fan_speed_percent,
                        "power_draw_w": snapshot.gpu.power_draw_w,
                    },
                    "ram": {
                        "usage_percent": snapshot.ram.usage_percent,
                        "used_mb": snapshot.ram.used_mb,
                        "total_mb": snapshot.ram.total_mb,
                    },
                    "frame": {
                        "available": snapshot.frame.available,
                        "fps": snapshot.frame.fps,
                    },
                }
            });

            if ws.send(Message::Text(msg.to_string())).is_err() {
                break;
            }
            last_sensor_send = std::time::Instant::now();
        }
    }
}

fn handle_message(
    text: &str,
    state: &Arc<WsSharedState>,
    sensor_subscribed: &mut bool,
) -> Option<String> {
    let msg: Value = serde_json::from_str(text).ok()?;
    let msg_type = msg.get("type")?.as_str()?;

    match msg_type {
        "sensors.subscribe" => {
            *sensor_subscribed = true;
            info!("Client subscribed to sensor data");
            Some(json!({"type": "sensors.subscribed"}).to_string())
        }
        "status" => {
            Some(json!({
                "type": "status.data",
                "ws_port": WS_PORT,
                "running": true,
            }).to_string())
        }
        _ => {
            debug!(msg_type, "Unknown WebSocket message type");
            Some(json!({
                "type": "error",
                "message": format!("Unknown message type: {}", msg_type),
            }).to_string())
        }
    }
}

/// Format f32 for JSON — NaN becomes null.
fn format_f32(v: f32) -> Value {
    if v.is_nan() {
        Value::Null
    } else {
        json!(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn handle_sensors_subscribe() {
        let state = Arc::new(WsSharedState::new());
        let mut subscribed = false;

        let response = handle_message(
            r#"{"type": "sensors.subscribe"}"#,
            &state,
            &mut subscribed,
        );

        assert!(subscribed, "Should be subscribed after message");
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "sensors.subscribed");
    }

    #[test]
    fn handle_status() {
        let state = Arc::new(WsSharedState::new());
        let mut subscribed = false;

        let response = handle_message(
            r#"{"type": "status"}"#,
            &state,
            &mut subscribed,
        );

        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "status.data");
        assert_eq!(resp["ws_port"], WS_PORT);
    }

    #[test]
    fn handle_unknown_message() {
        let state = Arc::new(WsSharedState::new());
        let mut subscribed = false;

        let response = handle_message(
            r#"{"type": "foo.bar"}"#,
            &state,
            &mut subscribed,
        );

        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "error");
    }

    #[test]
    fn format_f32_handles_nan() {
        assert_eq!(format_f32(42.5), json!(42.5));
        assert_eq!(format_f32(f32::NAN), Value::Null);
    }

    #[test]
    fn server_starts_and_accepts_connection() {
        let state = Arc::new(WsSharedState::new());
        let _handle = start(state.clone());

        // Give server time to bind
        thread::sleep(Duration::from_millis(200));

        // Connect as a TCP client (don't do full WebSocket handshake — just verify port is open)
        let result = TcpStream::connect(format!("127.0.0.1:{}", WS_PORT));
        assert!(result.is_ok(), "Should be able to connect to WebSocket port");

        // Shutdown
        state.running.store(false, Ordering::Relaxed);
    }
}
```

- [ ] **Step 2: Add mod declaration to main.rs**

Add `mod ws_server;` after the existing module declarations in `host/src/main.rs`:

```rust
mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;
mod widget_builder;
mod ws_server;
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- ws_server`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/ws_server.rs host/src/main.rs
git commit -m "feat(host): add WebSocket server module on localhost:9473"
```

---

### Task 4: Refactor main.rs — Shared run_host + Service Mode + DLL Discovery

**Files:**
- Modify: `host/src/main.rs`

This is the largest task — refactors the CLI, adds `--service` mode, wires in the WebSocket server, and shares sensor data with both the shared memory writer and the WebSocket thread.

- [ ] **Step 1: Rewrite host/src/main.rs**

Replace the entire file:

```rust
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;
mod widget_builder;
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

    // Shared state for WebSocket server
    let ws_state = Arc::new(ws_server::WsSharedState::new());

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
        poll_ms = config.poll_interval_ms,
        ws_port = ws_server::WS_PORT,
        exclude_count = config.exclude.len(),
        "Omni host starting"
    );
    info!("Press Ctrl+C to stop");

    let mut scanner_instance = scanner::Scanner::new(dll_path.to_string(), config);
    let mut latest_snapshot = omni_shared::SensorSnapshot::default();
    let widget_builder = widget_builder::WidgetBuilder::new();

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

        // Build sensor widgets
        let widgets = widget_builder.build(&latest_snapshot);

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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles with no errors.

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Verify --service mode with DLL discovery**

Build the overlay DLL first:
```bash
cargo build -p omni-overlay-dll
```

Then test service mode:
```bash
cargo run -p omni-host -- --service
```

Expected: Host discovers `omni_overlay.dll` in the target/debug directory and starts normally with WebSocket server.

- [ ] **Step 5: Verify --watch mode still works**

```bash
cargo run -p omni-host -- --watch target/debug/omni_overlay.dll
```

Expected: Same behavior as before, plus WebSocket server is running.

- [ ] **Step 6: Commit**

```bash
git add host/src/main.rs
git commit -m "feat(host): add --service mode, DLL auto-discovery, WebSocket integration in main loop"
```

---

### Task 5: Integration Test — WebSocket + Service Mode

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Start in service mode**

```bash
cargo run -p omni-host -- --service
```

Expected logs:
```
INFO DLL found (dev debug layout) path=...
INFO WebSocket server listening addr=127.0.0.1:9473
INFO Sensor poller started, interval=1000ms
INFO Omni host starting dll_path=... ws_port=9473
```

- [ ] **Step 3: Test WebSocket connection**

Open a browser console (or use websocat/wscat) and connect:

```javascript
const ws = new WebSocket('ws://localhost:9473');
ws.onmessage = (e) => console.log(JSON.parse(e.data));
ws.onopen = () => ws.send(JSON.stringify({type: 'sensors.subscribe'}));
```

Expected: Receive `{"type": "sensors.subscribed"}` followed by `{"type": "sensors.data", ...}` every second with live sensor values.

- [ ] **Step 4: Test status endpoint**

```javascript
ws.send(JSON.stringify({type: 'status'}));
```

Expected: `{"type": "status.data", "ws_port": 9473, "running": true}`

- [ ] **Step 5: Launch a game and verify overlay**

With the host running in service mode, launch a DX11 or DX12 game. The overlay should appear with all sensors + FPS as before.

- [ ] **Step 6: Verify Ctrl+C shutdown**

Ctrl+C should cleanly shut down: eject DLLs, stop sensors, stop WebSocket.

- [ ] **Step 7: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9a-0 integration test"
```

---

## Phase 9a-0 Complete — Summary

At this point you have:

1. **WebSocket server** on localhost:9473 — accepts connections, routes JSON messages
2. **Sensor streaming** — `sensors.subscribe` pushes live data to connected clients
3. **Service mode** — `--service` auto-discovers DLL, starts everything automatically
4. **DLL renamed** — `omni_overlay.dll` (not `omni_overlay_dll.dll`)
5. **Shared run_host** — `--watch` and `--service` share the same core loop
6. **Electron-ready** — an Electron app can spawn `omni-host --service` and connect via WebSocket

**Next:** Phase 9a-1 adds the core widget file format and parser.
