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
use tracing::{debug, info, warn};
use tungstenite::{accept, Message};

pub const WS_PORT: u16 = 9473;

/// Shared state between the WebSocket server and the main loop.
pub struct WsSharedState {
    pub latest_snapshot: Mutex<SensorSnapshot>,
    pub active_omni_file: Mutex<Option<crate::omni::types::OmniFile>>,
    pub data_dir: std::path::PathBuf,
    pub running: AtomicBool,
}

impl WsSharedState {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        Self {
            latest_snapshot: Mutex::new(SensorSnapshot::default()),
            active_omni_file: Mutex::new(None),
            data_dir,
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
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .ok();
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
                        let text_str: &str = &text;
                        if let Some(response) =
                            handle_message(text_str, state, &mut sensor_subscribed)
                        {
                            if ws.send(Message::Text(response.into())).is_err() {
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
            let snapshot = *state.latest_snapshot.lock().unwrap();
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

            if ws.send(Message::Text(msg.to_string().into())).is_err() {
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
        "status" => Some(
            json!({
                "type": "status.data",
                "ws_port": WS_PORT,
                "running": true,
            })
            .to_string(),
        ),
        "widget.parse" => {
            let source = msg.get("source")?.as_str()?;
            let (file, diagnostics) = crate::omni::parser::parse_omni_with_diagnostics(source);
            let diag_json: Vec<Value> = diagnostics
                .iter()
                .map(|d| serde_json::to_value(d).unwrap_or(json!(null)))
                .collect();
            Some(
                json!({
                    "type": "widget.parsed",
                    "file": file.as_ref().map(|f| serde_json::to_value(f).unwrap_or(json!(null))),
                    "diagnostics": diag_json,
                })
                .to_string(),
            )
        }
        "widget.update" => {
            let file_value = msg.get("file")?;
            match serde_json::from_value::<crate::omni::types::OmniFile>(file_value.clone()) {
                Ok(file) => {
                    if let Ok(mut active) = state.active_omni_file.lock() {
                        *active = Some(file);
                    }
                    info!("Widget file updated via WebSocket");
                    Some(json!({"type": "widget.updated"}).to_string())
                }
                Err(e) => {
                    warn!(error = %e, "Failed to deserialize widget file");
                    Some(
                        json!({
                            "type": "error",
                            "message": format!("Invalid widget file: {}", e),
                        })
                        .to_string(),
                    )
                }
            }
        }
        "file.list" => Some(crate::workspace::file_api::handle_list(&state.data_dir).to_string()),
        "file.read" => {
            let path = msg.get("path").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::workspace::file_api::handle_read(&state.data_dir, path).to_string())
        }
        "file.write" => {
            let path = msg.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            Some(
                crate::workspace::file_api::handle_write(&state.data_dir, path, content)
                    .to_string(),
            )
        }
        "file.create" => {
            let create_type = msg.get("createType").and_then(|v| v.as_str()).unwrap_or("");
            let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            Some(
                crate::workspace::file_api::handle_create(&state.data_dir, create_type, name)
                    .to_string(),
            )
        }
        "file.delete" => {
            let path = msg.get("path").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::workspace::file_api::handle_delete(&state.data_dir, path).to_string())
        }
        "widget.apply" => {
            let source = msg.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let (file, diagnostics) = crate::omni::parser::parse_omni_with_diagnostics(source);
            let diag_json: Vec<Value> = diagnostics
                .iter()
                .map(|d| serde_json::to_value(d).unwrap_or(json!(null)))
                .collect();
            let has_errors = diagnostics
                .iter()
                .any(|d| d.severity == crate::omni::parser::Severity::Error);
            // Only apply if no errors
            if !has_errors {
                if let Some(ref f) = file {
                    if let Ok(mut active) = state.active_omni_file.lock() {
                        *active = Some(f.clone());
                    }
                }
            }
            Some(
                json!({
                    "type": "widget.applied",
                    "file": file.as_ref().map(|f| serde_json::to_value(f).unwrap_or(json!(null))),
                    "diagnostics": diag_json,
                })
                .to_string(),
            )
        }
        _ => {
            debug!(msg_type, "Unknown WebSocket message type");
            Some(
                json!({
                    "type": "error",
                    "message": format!("Unknown message type: {}", msg_type),
                })
                .to_string(),
            )
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

    #[test]
    fn handle_sensors_subscribe() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let response = handle_message(r#"{"type": "sensors.subscribe"}"#, &state, &mut subscribed);

        assert!(subscribed, "Should be subscribed after message");
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "sensors.subscribed");
    }

    #[test]
    fn handle_status() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let response = handle_message(r#"{"type": "status"}"#, &state, &mut subscribed);

        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "status.data");
        assert_eq!(resp["ws_port"], WS_PORT);
    }

    #[test]
    fn handle_unknown_message() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let response = handle_message(r#"{"type": "foo.bar"}"#, &state, &mut subscribed);

        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "error");
    }

    #[test]
    fn format_f32_handles_nan() {
        assert_eq!(format_f32(42.5), json!(42.5));
        assert_eq!(format_f32(f32::NAN), Value::Null);
    }

    #[test]
    fn handle_widget_parse_valid() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let source = r#"<widget id="fps" name="FPS"><template><div>hello</div></template><style>#fps { color: white; }</style></widget>"#;
        let msg = serde_json::to_string(&json!({
            "type": "widget.parse",
            "source": source,
        }))
        .unwrap();

        let response = handle_message(&msg, &state, &mut subscribed);
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "widget.parsed");
        assert!(resp["file"].is_object(), "Should return parsed file");
        assert!(
            resp["diagnostics"].is_array(),
            "Should return diagnostics array"
        );
    }

    #[test]
    fn handle_widget_parse_empty() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let msg = serde_json::to_string(&json!({
            "type": "widget.parse",
            "source": "",
        }))
        .unwrap();

        let response = handle_message(&msg, &state, &mut subscribed);
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "widget.parsed");
    }

    #[test]
    fn handle_widget_update_valid() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let file = crate::omni::types::OmniFile {
            theme_src: None,
            poll_config: std::collections::HashMap::new(),
            widgets: vec![],
        };

        let msg = serde_json::to_string(&json!({
            "type": "widget.update",
            "file": file,
        }))
        .unwrap();

        let response = handle_message(&msg, &state, &mut subscribed);
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "widget.updated");

        // Verify the file was stored in state
        let active = state.active_omni_file.lock().unwrap();
        assert!(active.is_some(), "Should have stored the file");
    }

    #[test]
    fn handle_widget_update_invalid() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let msg = serde_json::to_string(&json!({
            "type": "widget.update",
            "file": "not a valid file",
        }))
        .unwrap();

        let response = handle_message(&msg, &state, &mut subscribed);
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "error");
        assert!(resp["message"]
            .as_str()
            .unwrap()
            .contains("Invalid widget file"));
    }

    #[test]
    fn server_starts_and_accepts_connection() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let _handle = start(state.clone());

        // Give server time to bind
        thread::sleep(Duration::from_millis(200));

        // Connect as a TCP client (don't do full WebSocket handshake — just verify port is open)
        let result = TcpStream::connect(format!("127.0.0.1:{}", WS_PORT));
        assert!(
            result.is_ok(),
            "Should be able to connect to WebSocket port"
        );

        // Shutdown
        state.running.store(false, Ordering::Relaxed);
    }
}
