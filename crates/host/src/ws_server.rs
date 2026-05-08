//! WebSocket server for Electron app communication.
//!
//! Runs on a dedicated thread, accepts one client at a time on localhost:9473.
//! Handles JSON messages with a "type" field for routing.
//! Shares sensor data with the main loop via Arc<Mutex<SensorSnapshot>>.

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use shared::SensorSnapshot;
use tracing::{debug, info, warn};
use tungstenite::{accept, Message};

use crate::share::ws_messages::is_share_message_type;

pub const WS_PORT: u16 = 9473;

/// Adapter from `RegistryHandle` to fork's `InstalledBundleLookup`.
struct RegistryBundleLookup<'a>(&'a crate::share::registry::RegistryHandle);

impl<'a> crate::workspace::fork::InstalledBundleLookup for RegistryBundleLookup<'a> {
    fn lookup(&self, slug: &str) -> Option<crate::workspace::fork::InstalledBundleView> {
        let entry = self.0.lookup_bundle(slug)?;
        // Legacy pre-schema entry — treat as not installed.
        if entry.installed_path.as_os_str().is_empty() {
            return None;
        }
        Some(crate::workspace::fork::InstalledBundleView {
            path: entry.installed_path.clone(),
            artifact_id: entry.artifact_id.clone(),
            content_hash: entry.content_hash.clone(),
            bundle_name: entry.display_name.clone(),
            author_pubkey: entry.author_pubkey.clone(),
            // Author display name lives in TofuStore, not the registry.
            author_display_name: None,
            author_fingerprint: entry.fingerprint_hex.clone(),
        })
    }
}

/// Shared state between the WebSocket server and the main loop.
pub struct WsSharedState {
    pub latest_snapshot: Mutex<SensorSnapshot>,
    pub active_omni_file: Mutex<Option<crate::omni::types::OmniFile>>,
    pub active_overlay: Mutex<String>,
    pub active_game: Mutex<Option<String>>,
    pub data_dir: std::path::PathBuf,
    pub running: AtomicBool,
    pub hwinfo_state: Mutex<crate::sensors::hwinfo::HwInfoState>,
    pub hwinfo_sensors_changed: Mutex<bool>,
    pub preview_subscribers: Mutex<Vec<mpsc::Sender<String>>>,
    pub latest_initial_html: Mutex<Option<(String, String)>>, // (html, css)
    /// Share-surface context (sub-spec #009). None when upload pipeline not configured.
    pub share_ctx: Mutex<Option<Arc<crate::share::ws_messages::ShareContext>>>,
    /// Tokio runtime used to drive async share-surface handlers from the sync WS loop.
    /// Created once on first share dispatch and reused.
    pub share_runtime: std::sync::OnceLock<tokio::runtime::Runtime>,
    /// Parsed editor-preview overlay. `Some` once the renderer has pushed
    /// a `preview.setEditorOverlay`; `None` triggers the mirror-by-default
    /// path in the per-frame loop (editor channel echoes in-game stream).
    pub editor_omni_file: Mutex<Option<crate::omni::types::OmniFile>>,
    /// Built initial HTML for the editor preview. Cached so the per-frame
    /// loop doesn't rebuild on every tick. Replaced on every
    /// `preview.setEditorOverlay`.
    pub editor_initial_html: Mutex<Option<crate::omni::html_builder::InitialHtml>>,
}

impl WsSharedState {
    /// Extract cloned HWiNFO values and units from the shared state.
    pub fn hwinfo_values_and_units(
        &self,
    ) -> (
        std::collections::HashMap<String, f64>,
        std::collections::HashMap<String, String>,
    ) {
        self.hwinfo_state
            .lock()
            .map(|s| (s.values.clone(), s.units.clone()))
            .unwrap_or_default()
    }

    pub fn new(data_dir: std::path::PathBuf) -> Self {
        Self {
            latest_snapshot: Mutex::new(SensorSnapshot::default()),
            active_omni_file: Mutex::new(None),
            active_overlay: Mutex::new("Default".to_string()),
            active_game: Mutex::new(None),
            data_dir,
            running: AtomicBool::new(true),
            hwinfo_state: Mutex::new(crate::sensors::hwinfo::HwInfoState::default()),
            hwinfo_sensors_changed: Mutex::new(false),
            preview_subscribers: Mutex::new(Vec::new()),
            latest_initial_html: Mutex::new(None),
            share_ctx: Mutex::new(None),
            share_runtime: std::sync::OnceLock::new(),
            editor_omni_file: Mutex::new(None),
            editor_initial_html: Mutex::new(None),
        }
    }

    /// Install the share-surface context. Call once during host startup after
    /// the identity keypair, guard, and ShareClient are constructed.
    pub fn set_share_ctx(&self, ctx: Arc<crate::share::ws_messages::ShareContext>) {
        if let Ok(mut slot) = self.share_ctx.lock() {
            *slot = Some(ctx);
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
    let mut preview_subscribed = false;
    let mut last_sensor_send = std::time::Instant::now();
    let (preview_tx, preview_rx) = mpsc::channel::<String>();

    'outer: while state.running.load(Ordering::Relaxed) {
        // Read incoming messages (non-blocking via read timeout)
        match ws.read() {
            Ok(msg) => {
                match msg {
                    Message::Text(text) => {
                        let text_str: &str = &text;

                        // Detect message type before handling
                        let msg_type = serde_json::from_str::<Value>(text_str).ok().and_then(|v| {
                            v.get("type")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
                        });

                        // Route share-surface messages to crates/host/src/share/ws_messages.rs
                        // (sub-spec #009). Falls through to `handle_message` for non-share types.
                        let share_handled = if is_share_message_type(msg_type.as_deref()) {
                            dispatch_share_message(text_str, state, preview_tx.clone())
                        } else {
                            false
                        };

                        if !share_handled {
                            if let Some(response) =
                                handle_message(text_str, state, &mut sensor_subscribed)
                            {
                                if ws.send(Message::Text(response.into())).is_err() {
                                    break; // Client disconnected
                                }
                            }
                        }

                        // Register as preview subscriber and send current HTML.
                        // Always re-sends HTML — the Electron renderer may have been
                        // destroyed and recreated while the WS connection stayed alive.
                        if msg_type.as_deref() == Some("preview.subscribe") {
                            if !preview_subscribed {
                                preview_subscribed = true;
                                state
                                    .preview_subscribers
                                    .lock()
                                    .unwrap()
                                    .push(preview_tx.clone());
                                info!("Client subscribed to preview updates");
                            }

                            // Send current HTML if available — dual-stream replay.
                            // Emits `preview.html.ingame` first (renamed from the
                            // old `preview.html` event), then `preview.html.editor`
                            // with mirror-by-default: if the renderer has pushed a
                            // `preview.setEditorOverlay` we replay that; otherwise
                            // we echo the in-game HTML so the editor iframe is
                            // never blank on first connect.
                            if let Some((ref html, ref css)) =
                                *state.latest_initial_html.lock().unwrap()
                            {
                                // In-game replay (renamed channel).
                                let ingame_msg = json!({
                                    "type": "preview.html.ingame",
                                    "html": html,
                                    "css": css,
                                })
                                .to_string();
                                if ws.send(Message::Text(ingame_msg.into())).is_err() {
                                    break;
                                }

                                // Editor replay: use the dedicated editor build if
                                // available, otherwise mirror the in-game HTML.
                                let editor_initial = state
                                    .editor_initial_html
                                    .lock()
                                    .ok()
                                    .and_then(|g| g.clone());
                                let (editor_html_owned, editor_css_owned, editor_overlay_name) =
                                    match editor_initial {
                                        Some(initial) => (
                                            initial.html.clone(),
                                            initial.css.clone(),
                                            state
                                                .active_overlay
                                                .lock()
                                                .map(|s| s.clone())
                                                .unwrap_or_else(|_| "Default".to_string()),
                                        ),
                                        None => (
                                            html.clone(),
                                            css.clone(),
                                            state
                                                .active_overlay
                                                .lock()
                                                .map(|s| s.clone())
                                                .unwrap_or_else(|_| "Default".to_string()),
                                        ),
                                    };
                                let editor_msg = json!({
                                    "type": "preview.html.editor",
                                    "html": editor_html_owned,
                                    "css": editor_css_owned,
                                    "overlay_name": editor_overlay_name,
                                })
                                .to_string();
                                if ws.send(Message::Text(editor_msg.into())).is_err() {
                                    break;
                                }
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

        // Push preview updates to client
        while let Ok(msg) = preview_rx.try_recv() {
            if ws.send(Message::Text(msg.into())).is_err() {
                break 'outer;
            }
        }

        // Push sensor data if subscribed (every 1 second)
        if sensor_subscribed && last_sensor_send.elapsed() >= Duration::from_secs(1) {
            let snapshot = *state.latest_snapshot.lock().unwrap();
            let hwinfo = state.hwinfo_state.lock().unwrap();
            let hwinfo_values: Vec<Value> = hwinfo
                .values
                .iter()
                .map(|(path, &v)| json!({"path": path, "value": v}))
                .collect();
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
                    "frame": frame_json(&snapshot.frame),
                    "hwinfo": {
                        "connected": hwinfo.connected,
                        "sensor_count": hwinfo.sensor_count,
                        "values": hwinfo_values,
                    },
                }
            });
            // Build hwinfo.sensors message while we still hold the lock
            let hwinfo_sensors_msg = {
                let sensor_list: Vec<Value> = hwinfo
                    .sensors
                    .iter()
                    .map(|s| json!({"path": s.path, "label": s.label, "unit": s.unit}))
                    .collect();
                Some(json!({
                    "type": "hwinfo.sensors",
                    "connected": hwinfo.connected,
                    "sensors": sensor_list,
                }))
            };
            drop(hwinfo);

            if ws.send(Message::Text(msg.to_string().into())).is_err() {
                break;
            }
            last_sensor_send = std::time::Instant::now();

            // Push hwinfo.sensors list when connected (every sensor cycle ensures
            // clients that connect after initial detection still receive the list)
            if let Some(sensors_msg) = hwinfo_sensors_msg {
                if ws
                    .send(Message::Text(sensors_msg.to_string().into()))
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    // Shutdown path — drain the cancel registry so in-flight installs observe
    // the cancellation. Runs on every WS exit (Close frame, read error, send
    // error, or the `running` flag flipping). No-op when `share_ctx` is `None`.
    if let Ok(guard) = state.share_ctx.lock() {
        if let Some(ctx) = guard.as_ref() {
            if let Ok(mut reg) = ctx.cancel_registry.lock() {
                for (_, token) in reg.drain() {
                    token.cancel();
                }
            }
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
        "preview.subscribe" => {
            let active = state.latest_initial_html.lock().unwrap().is_some();
            Some(json!({"type": "preview.subscribed", "active": active}).to_string())
        }
        "status" => {
            let active_overlay = state
                .active_overlay
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default();
            let active_game = state.active_game.lock().ok().and_then(|s| s.clone());
            Some(
                json!({
                    "type": "status.data",
                    "ws_port": WS_PORT,
                    "running": true,
                    "active_overlay": active_overlay,
                    "active_game": active_game,
                })
                .to_string(),
            )
        }
        "widget.parse" => {
            let source = msg.get("source")?.as_str()?;
            let hwinfo_connected = state
                .hwinfo_state
                .lock()
                .map(|s| s.connected)
                .unwrap_or(false);
            let (file, diagnostics) =
                crate::omni::parser::parse_omni_with_diagnostics_hwinfo(source, hwinfo_connected);
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
        "preview.setEditorOverlay" => {
            let source = msg.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let overlay_name = msg
                .get("overlay_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Parse source. Errors surface as a renderer-facing error envelope
            // (no state mutation on parse failure — keeps the editor stream
            // showing the previous successful build until the user fixes it).
            let hwinfo_connected = state
                .hwinfo_state
                .lock()
                .map(|s| s.connected)
                .unwrap_or(false);
            let (file, diagnostics) =
                crate::omni::parser::parse_omni_with_diagnostics_hwinfo(source, hwinfo_connected);
            let has_errors = diagnostics
                .iter()
                .any(|d| d.severity == crate::omni::parser::Severity::Error);
            if has_errors || file.as_ref().map_or(true, |f| f.widgets.is_empty()) {
                return Some(
                    serde_json::json!({
                        "type": "error",
                        "code": "EDITOR_OVERLAY_PARSE_FAILED",
                        "message": "preview.setEditorOverlay: source did not parse to a non-empty OmniFile",
                        "diagnostics": diagnostics
                            .iter()
                            .map(|d| serde_json::to_value(d).unwrap_or(serde_json::json!(null)))
                            .collect::<Vec<_>>(),
                    })
                    .to_string(),
                );
            }
            let parsed = file.expect("checked above — file is Some with non-empty widgets");

            // Build initial HTML for the editor stream. Same builder Ultralight
            // uses for the in-game stream; the iframe can render the output as-is.
            // For sensor inputs, use the latest available snapshot or zeroed defaults
            // — the per-frame loop (Task 3) will push live values within one tick.
            let (hwinfo_values, hwinfo_units) = state.hwinfo_values_and_units();
            let snapshot = state.latest_snapshot.lock().map(|s| *s).unwrap_or_default();
            let history = crate::omni::history::SensorHistory::new();
            // Use a sensible default viewport — the iframe will rebuild HTML
            // on next setEditorOverlay if the renderer sends a new size signal.
            // For now, mirror Ultralight's typical initial viewport.
            let (vw, vh) = (1920u32, 1080u32);
            let initial = crate::omni::html_builder::build_initial_html(
                &parsed,
                &snapshot,
                vw,
                vh,
                &state.data_dir,
                &overlay_name,
                &hwinfo_values,
                &hwinfo_units,
                &history,
                None, // editor preview uses logical scale = 1.0 by default
                crate::omni::view_trust::ViewTrust::LocalAuthored,
            );

            // Store and broadcast the initial HTML on the editor channel.
            if let Ok(mut slot) = state.editor_omni_file.lock() {
                *slot = Some(parsed);
            }
            if let Ok(mut slot) = state.editor_initial_html.lock() {
                *slot = Some(initial.clone());
            }
            broadcast_preview_html_editor(state, &initial.html, &initial.css, &overlay_name);

            Some(serde_json::json!({"type": "preview.setEditorOverlay.ack"}).to_string())
        }
        "widget.apply" => {
            let source = msg.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let hwinfo_connected = state
                .hwinfo_state
                .lock()
                .map(|s| s.connected)
                .unwrap_or(false);
            let (file, diagnostics) =
                crate::omni::parser::parse_omni_with_diagnostics_hwinfo(source, hwinfo_connected);
            let diag_json: Vec<Value> = diagnostics
                .iter()
                .map(|d| serde_json::to_value(d).unwrap_or(json!(null)))
                .collect();
            let has_errors = diagnostics
                .iter()
                .any(|d| d.severity == crate::omni::parser::Severity::Error);
            // Only apply if the parse produced a non-empty file with no errors.
            // Guard against clients that accidentally send unrelated content
            // (e.g. a CSS theme file) — the parser would return an OmniFile
            // with zero widgets, which would clobber the live preview.
            if !has_errors {
                if let Some(ref f) = file {
                    if !f.widgets.is_empty() {
                        if let Ok(mut active) = state.active_omni_file.lock() {
                            *active = Some(f.clone());
                        }
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
        "config.get" => {
            let config_path = crate::config::config_path();
            let config = crate::config::load_config(&config_path);
            Some(
                json!({
                    "type": "config.data",
                    "config": serde_json::to_value(&config).unwrap_or(json!(null)),
                })
                .to_string(),
            )
        }
        "config.update" => {
            let config_value = msg.get("config")?;
            match serde_json::from_value::<crate::config::Config>(config_value.clone()) {
                Ok(new_config) => {
                    let config_path = crate::config::config_path();
                    match crate::config::save_config(&config_path, &new_config) {
                        Ok(()) => {
                            info!("Config updated via WebSocket");
                            Some(json!({"type": "config.updated"}).to_string())
                        }
                        Err(e) => Some(
                            json!({
                                "type": "error",
                                "message": format!("Failed to save config: {}", e),
                            })
                            .to_string(),
                        ),
                    }
                }
                Err(e) => Some(
                    json!({
                        "type": "error",
                        "message": format!("Invalid config: {}", e),
                    })
                    .to_string(),
                ),
            }
        }
        "explorer.fork" => {
            use crate::share::registry::{RegistryHandle, RegistryKind};
            use crate::workspace::fork::{self, ForkRequest};

            // Wire-shape contract (renderer side: ExplorerForkParams /
            // ExplorerForkResultSchema in apps/desktop/renderer/lib/share-types.ts):
            //   request:  { id, type: "explorer.fork", params: { artifact_id, target_name } }
            //   response: { id, type: "explorer.forkResult", workspace_path, new_manifest }
            //
            // Earlier this arm read the top-level fields directly (`msg.bundle_slug`
            // / `msg.new_overlay_name`), which the share `send()` envelope never
            // sets — every share request nests the per-handler params under
            // `params`. The result was that fork ran with empty strings, errored
            // generically, AND returned a frame with no `id` field — so the
            // renderer's awaited promise never resolved (the `send` hook keys
            // matches by id) and the user saw the dialog hang silently.
            let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
            let artifact_id = params
                .get("artifact_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let target_name = params
                .get("target_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let overlays_root = state.data_dir.join("overlays");
            let registry = match RegistryHandle::load(&state.data_dir, RegistryKind::Bundles) {
                Ok(r) => r,
                Err(e) => {
                    return Some(
                        json!({
                            "id": id,
                            "type": "error",
                            "error": {
                                "code": "IO_ERROR",
                                "kind": "Io",
                                "message": format!(
                                    "installed-bundles registry load failed: {e}"
                                ),
                            },
                        })
                        .to_string(),
                    );
                }
            };

            // The registry key for a bundle is `<pubkey8>-<display_name>` (see
            // install.rs), but the renderer only carries `artifact_id`. Resolve
            // the slug at the call site so the rest of the fork API stays
            // unchanged. NOT_FOUND if no matching row — common when the user
            // uninstalled between the dialog opening and Submit.
            let slug = registry
                .entries()
                .iter()
                .find(|(_, e)| e.artifact_id == artifact_id)
                .map(|(k, _)| k.clone());
            let Some(slug) = slug else {
                return Some(
                    json!({
                        "id": id,
                        "type": "error",
                        "error": {
                            "code": "NOT_FOUND",
                            "kind": "Malformed",
                            "message": format!(
                                "no installed bundle for artifact_id `{artifact_id}` (was it just uninstalled?)"
                            ),
                        },
                    })
                    .to_string(),
                );
            };

            let lookup = RegistryBundleLookup(&registry);
            let req = ForkRequest {
                bundle_slug: slug,
                new_overlay_name: target_name,
            };
            match fork::fork_to_local(req, &overlays_root, &lookup) {
                Ok(res) => {
                    // Reload the forked manifest so the renderer can echo it
                    // (the schema requires a `new_manifest` object). Best-effort
                    // — fork already succeeded, so a manifest read failure is
                    // logged-and-empty, not a hard error.
                    let manifest = std::fs::read_to_string(res.path.join("overlay.omni"))
                        .map(|s| {
                            serde_json::from_str::<serde_json::Value>(&s)
                                .unwrap_or_else(|_| serde_json::json!({}))
                        })
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let workspace_path = res
                        .path
                        .strip_prefix(&state.data_dir)
                        .unwrap_or(&res.path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    Some(
                        json!({
                            "id": id,
                            "type": "explorer.forkResult",
                            "workspace_path": workspace_path,
                            "new_manifest": manifest,
                        })
                        .to_string(),
                    )
                }
                Err(e) => Some(
                    json!({
                        "id": id,
                        "type": "error",
                        "error": {
                            "code": e.ws_error_code(),
                            "kind": "Malformed",
                            "message": e.to_string(),
                        },
                    })
                    .to_string(),
                ),
            }
        }
        "explorer.install"
        | "explorer.preview"
        | "explorer.cancelPreview"
        | "explorer.list"
        | "explorer.get" => {
            // Fallback path: reached only when `share_ctx` is `None` (the share
            // dispatcher at `handle_client` returned `false`, letting the message
            // fall through to `handle_message`). Returns the D-004-J
            // `service_unavailable` envelope. When `share_ctx` is `Some`,
            // `dispatch_share_message` spawns the handler on `share_runtime`
            // and this arm is not exercised.
            let payload = crate::share::handlers::install_context_unavailable();
            let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("");
            Some(crate::share::handlers::error_frame(id, &payload))
        }
        "log.path" => {
            let log_path = state.data_dir.join("logs").join("omni-host.log");
            Some(
                json!({
                    "type": "log.path",
                    "path": log_path.to_string_lossy(),
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


/// Dispatch a share-surface message. Synchronous results (and all progress frames)
/// are pushed onto the provided mpsc sender; the WS loop drains it every tick and
/// forwards frames to the client. Returns true if the message was routed (i.e.
/// share context exists); false means caller should fall through to `handle_message`.
fn dispatch_share_message(
    text: &str,
    state: &Arc<WsSharedState>,
    send_tx: mpsc::Sender<String>,
) -> bool {
    let Some(ctx) = state
        .share_ctx
        .lock()
        .ok()
        .and_then(|g| g.as_ref().cloned())
    else {
        // No share context configured — treat as unrouted so normal error path runs.
        return false;
    };

    let Ok(msg) = serde_json::from_str::<Value>(text) else {
        return false;
    };

    // Get-or-init the tokio runtime that drives async share handlers.
    let rt = state.share_runtime.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("omni-share")
            .build()
            .expect("build share tokio runtime")
    });
    let handle = rt.handle().clone();

    let send_for_dispatch = send_tx.clone();
    let send_fn = move |s: String| {
        let _ = send_for_dispatch.send(s);
    };

    handle.spawn(async move {
        let reply = crate::share::ws_messages::dispatch(&ctx, &msg, send_fn).await;
        if let Some(text) = reply {
            let _ = send_tx.send(text);
        }
    });
    true
}

/// Broadcast a preview message to all active subscribers.
/// Removes disconnected subscribers automatically.
pub fn broadcast_preview(state: &WsSharedState, message: &str) {
    let mut subs = state.preview_subscribers.lock().unwrap();
    subs.retain(|tx| tx.send(message.to_string()).is_ok());
}

/// Broadcast the editor-stream initial HTML on the `preview.html.editor` channel.
///
/// Called by the `preview.setEditorOverlay` handler after building the initial
/// HTML. All current preview subscribers receive the editor envelope; the iframe
/// in `preview-panel.tsx` listens on this channel exclusively (not the in-game
/// channel) once the renderer has subscribed to the editor stream.
pub fn broadcast_preview_html_editor(
    state: &WsSharedState,
    html: &str,
    css: &str,
    overlay_name: &str,
) {
    let msg = serde_json::json!({
        "type": "preview.html.editor",
        "html": html,
        "css": css,
        "overlay_name": overlay_name,
    })
    .to_string();
    broadcast_preview(state, &msg);
}

/// Format f32 for JSON — NaN becomes null.
fn format_f32(v: f32) -> Value {
    if v.is_nan() {
        Value::Null
    } else {
        json!(v)
    }
}

/// Build the "frame" JSON object for the sensors.data payload.
///
/// Intentionally omits `render_width`/`render_height` — they're part of
/// `FrameData` for host-internal viewport sizing but no Electron consumer
/// reads them. Route all f32 fields through `format_f32` so NaN serializes
/// as JSON `null` (the SensorReadout renderer treats null as "N/A").
fn frame_json(frame: &shared::FrameData) -> Value {
    json!({
        "available": frame.available,
        "fps": format_f32(frame.fps),
        "frame_time_ms": format_f32(frame.frame_time_ms),
        "frame_time_avg_ms": format_f32(frame.frame_time_avg_ms),
        "frame_time_1percent_ms": format_f32(frame.frame_time_1percent_ms),
        "frame_time_01percent_ms": format_f32(frame.frame_time_01percent_ms),
    })
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
        assert_eq!(resp["active_overlay"], "Default");
        assert!(resp["active_game"].is_null());
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
    fn explorer_install_returns_service_unavailable_stub() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        let response = handle_message(
            r#"{"type": "explorer.install", "id": "req-42"}"#,
            &state,
            &mut subscribed,
        );

        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "error");
        assert_eq!(resp["id"], "req-42");
        assert_eq!(resp["error"]["code"], "service_unavailable");
        assert_eq!(resp["error"]["kind"], "HostLocal");
        assert_eq!(resp["error"]["detail"], "install_context_not_constructed");
    }

    #[test]
    fn explorer_preview_and_list_share_stub_envelope() {
        // Smoke-check the rest of the arms so a future split doesn't
        // silently regress one of them.
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        for msg_type in [
            "explorer.preview",
            "explorer.cancelPreview",
            "explorer.list",
            "explorer.get",
        ] {
            let msg = format!(r#"{{"type": "{msg_type}"}}"#);
            let response = handle_message(&msg, &state, &mut subscribed);
            let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
            assert_eq!(resp["type"], "error", "{msg_type} missing error type");
            assert_eq!(
                resp["error"]["code"], "service_unavailable",
                "{msg_type} wrong code"
            );
        }
    }

    #[test]
    fn share_message_gate_recognizes_all_inner_dispatcher_arms() {
        // Smoke test for the gate wrapper. The real drift guard lives in
        // `share::ws_messages::dispatch_has_arm_for_every_share_message_type`
        // (a parity test that calls dispatch for every entry in
        // `SHARE_MESSAGE_TYPES` and confirms no `_` arm fires). This test only
        // confirms the gate function delegates to the canonical slice.
        for ty in crate::share::ws_messages::SHARE_MESSAGE_TYPES {
            assert!(
                is_share_message_type(Some(ty)),
                "{ty} listed in SHARE_MESSAGE_TYPES but is_share_message_type rejects it",
            );
        }
        assert!(!is_share_message_type(Some("no.such.type")));
        assert!(!is_share_message_type(None));
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
            dpi_scale: None,
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
    fn handle_config_get() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;
        let response = handle_message(r#"{"type": "config.get"}"#, &state, &mut subscribed);
        let resp: Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(resp["type"], "config.data");
        assert!(resp["config"].is_object());
        assert!(resp["config"]["active_overlay"].is_string());
    }

    #[test]
    fn frame_json_serializes_all_fields_and_nan_to_null() {
        let mut snapshot = shared::SensorSnapshot::default();
        snapshot.frame.available = true;
        snapshot.frame.fps = 144.0;
        snapshot.frame.frame_time_ms = 6.9;
        snapshot.frame.frame_time_avg_ms = 7.2;
        snapshot.frame.frame_time_1percent_ms = 12.5;
        snapshot.frame.frame_time_01percent_ms = f32::NAN;

        let json = frame_json(&snapshot.frame);
        assert_eq!(json["available"], true);
        assert_eq!(json["fps"], 144.0);
        assert!((json["frame_time_ms"].as_f64().unwrap() - 6.9_f64).abs() < 1e-4);
        assert!((json["frame_time_avg_ms"].as_f64().unwrap() - 7.2_f64).abs() < 1e-4);
        assert!((json["frame_time_1percent_ms"].as_f64().unwrap() - 12.5_f64).abs() < 1e-4);
        assert!(
            json["frame_time_01percent_ms"].is_null(),
            "NaN should serialize as null"
        );
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

    #[test]
    fn shared_state_includes_editor_preview_slots() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let state = WsSharedState::new(tmp.path().to_path_buf());
        assert!(
            state.editor_omni_file.lock().unwrap().is_none(),
            "editor_omni_file starts empty"
        );
        assert!(
            state.editor_initial_html.lock().unwrap().is_none(),
            "editor_initial_html starts empty"
        );
    }

    #[test]
    fn preview_set_editor_overlay_parses_and_stores() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let state = Arc::new(WsSharedState::new(tmp.path().to_path_buf()));
        let mut subscribed = false;

        // Minimal valid `.omni` source — single widget, no theme, no fonts.
        let source = r#"<widget id="hello" name="hello"><template><div>Hello</div></template></widget>"#;
        let msg = serde_json::json!({
            "type": "preview.setEditorOverlay",
            "source": source,
            "overlay_name": "test-overlay",
        })
        .to_string();

        let reply = handle_message(&msg, &state, &mut subscribed).expect("handler returns reply");
        let parsed: serde_json::Value = serde_json::from_str(&reply).expect("reply is JSON");
        assert_eq!(parsed["type"], "preview.setEditorOverlay.ack", "reply: {parsed}");

        assert!(
            state.editor_omni_file.lock().unwrap().is_some(),
            "editor_omni_file populated after setEditorOverlay"
        );
        assert!(
            state.editor_initial_html.lock().unwrap().is_some(),
            "editor_initial_html populated after setEditorOverlay"
        );
    }

    #[test]
    fn preview_set_editor_overlay_rejects_malformed_source() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let state = Arc::new(WsSharedState::new(tmp.path().to_path_buf()));
        let mut subscribed = false;

        let msg = serde_json::json!({
            "type": "preview.setEditorOverlay",
            "source": "<widget>oops missing template</widget>",
            "overlay_name": "broken",
        })
        .to_string();

        let reply = handle_message(&msg, &state, &mut subscribed).expect("handler returns reply");
        let parsed: serde_json::Value = serde_json::from_str(&reply).expect("reply is JSON");
        assert_eq!(parsed["type"], "error", "reply: {parsed}");
        assert!(
            state.editor_omni_file.lock().unwrap().is_none(),
            "malformed source must not populate editor_omni_file"
        );
    }

    /// Verifies that `preview.subscribe` returns a `preview.subscribed`
    /// acknowledgement and that `broadcast_preview_html_editor` emits the
    /// correct `preview.html.editor` envelope type.
    ///
    /// NOTE: The dual-stream replay logic (ingame + editor messages) inside
    /// `handle_client` requires a live WebSocket connection and cannot be
    /// exercised in a unit test without a heavyweight fixture. The actual replay
    /// path is covered by the manual smoke test (Task 8) and the integration
    /// smoke check in the broader ws_server test suite.
    #[test]
    fn preview_subscribe_ack_and_editor_broadcast_type() {
        let state = Arc::new(WsSharedState::new(std::env::temp_dir()));
        let mut subscribed = false;

        // preview.subscribe must ack
        let reply = handle_message(r#"{"type": "preview.subscribe"}"#, &state, &mut subscribed)
            .expect("handler returns reply");
        let parsed: serde_json::Value = serde_json::from_str(&reply).expect("reply is JSON");
        assert_eq!(parsed["type"], "preview.subscribed");
        assert_eq!(
            parsed["active"], false,
            "active should be false when latest_initial_html is None"
        );

        // Populate latest_initial_html then check active flag
        *state.latest_initial_html.lock().unwrap() =
            Some(("html".to_string(), "css".to_string()));
        let reply2 = handle_message(r#"{"type": "preview.subscribe"}"#, &state, &mut subscribed)
            .expect("handler returns reply");
        let parsed2: serde_json::Value = serde_json::from_str(&reply2).expect("reply is JSON");
        assert_eq!(parsed2["active"], true, "active should be true when html is cached");

        // Verify broadcast_preview_html_editor emits the correct event type.
        // Subscribe a channel to capture the broadcast.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        state.preview_subscribers.lock().unwrap().push(tx);

        broadcast_preview_html_editor(&state, "<div/>", "body{}", "my-overlay");

        let broadcast_msg = rx.try_recv().expect("broadcast sent a message");
        let broadcast_val: serde_json::Value =
            serde_json::from_str(&broadcast_msg).expect("broadcast is JSON");
        assert_eq!(
            broadcast_val["type"], "preview.html.editor",
            "editor broadcast must use preview.html.editor event type"
        );
        assert_eq!(broadcast_val["html"], "<div/>");
        assert_eq!(broadcast_val["css"], "body{}");
        assert_eq!(broadcast_val["overlay_name"], "my-overlay");
    }
}
