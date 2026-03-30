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
use tungstenite::{accept, Message};

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
                        let text_str: &str = &text;
                        if let Some(response) = handle_message(text_str, state, &mut sensor_subscribed) {
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

            if ws.send(Message::Text(msg.to_string().into())).is_err() {
                break;
            }
            last_sensor_send = std::time::Instant::now();
        }
    }
}

fn handle_message(
    text: &str,
    _state: &Arc<WsSharedState>,
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
