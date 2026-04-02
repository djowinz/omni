# Backend Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the Electron frontend to the `omni-host.exe` WebSocket backend, replacing all localStorage with backend API calls for overlay CRUD, config management, live sensor data, and Monaco diagnostics.

**Architecture:** A bidirectional IPC message layer (`ipcRenderer.invoke` / `ipcMain.handle`) bridges the renderer to the host's WebSocket. The `BackendApi` class in the renderer wraps typed messages. State management is refactored for name-based overlay identity (backend-authoritative). Sensor data streams via a separate push channel. Monaco markers are driven by the host's `widget.parse` diagnostics.

**Tech Stack:** TypeScript, Electron IPC, WebSocket (ws), Monaco Editor, ts-rs generated types

**Spec:** `docs/superpowers/specs/2026-04-01-backend-integration-design.md`

---

## File Structure

### Rust (backend) — modified
- `host/src/ws_server.rs` — add `config.get`, `config.update` handlers
- `host/src/omni/parser.rs` — add `#[derive(TS)]` to `ParseError`, `Severity`

### Electron main process — modified
- `desktop/main/host-manager.ts` — add `sendAndWait()` method
- `desktop/main/main.ts` — add `ws-message` IPC handler, sensor data forwarding
- `desktop/main/preload.ts` — add `sendMessage`, `onSensorData`

### Electron renderer — new files
- `desktop/renderer/lib/backend-api.ts` — typed wrapper around `window.omni.sendMessage`
- `desktop/renderer/lib/sensor-mapping.ts` — `SensorSnapshot` → `MetricValues` mapping
- `desktop/renderer/hooks/use-sensor-data.ts` — hook for live sensor subscription
- `desktop/renderer/hooks/use-backend.ts` — hook exposing `BackendApi` instance

### Electron renderer — modified files
- `desktop/renderer/types/omni.ts` — simplify Overlay, remove StorageAdapter, refactor AppState/AppAction
- `desktop/renderer/types/electron.d.ts` — update OmniIpcBridge with new methods
- `desktop/renderer/hooks/use-omni-state.tsx` — refactor for backend-authoritative state
- `desktop/renderer/components/omni/editor-panel.tsx` — debounced parse + Monaco markers
- `desktop/renderer/components/omni/preview-panel.tsx` — Live/Simulate toggle
- `desktop/renderer/components/omni/metric-simulator.tsx` — conditional visibility
- `desktop/renderer/components/omni/status-bar.tsx` — real connection status
- `desktop/renderer/components/omni/header.tsx` — overlay list from backend
- `desktop/renderer/components/omni/game-assignments-dialog.tsx` — config.update calls

### Electron renderer — deleted files
- `desktop/renderer/lib/storage-adapter.ts` — replaced by backend-api.ts

---

## Task 1: Add `config.get` and `config.update` to Rust backend + ts-rs exports

**Files:**
- Modify: `host/src/ws_server.rs`
- Modify: `host/src/omni/parser.rs`
- Modify: `host/src/config.rs`

- [ ] **Step 1: Add `config.get` and `config.update` message handlers**

In `host/src/ws_server.rs`, add two new match arms to the `handle_message` function, before the `_ =>` default arm:

```rust
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
```

Make `save_config` public if it isn't already (it is — confirmed `pub fn save_config`).

- [ ] **Step 2: Add `#[derive(TS)]` to `ParseError` and `Severity`**

In `host/src/omni/parser.rs`, add `use ts_rs::TS;` to the imports, then add the derive:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub struct ParseError {
    pub message: String,
    pub severity: Severity,
    pub line: usize,
    pub column: usize,
    pub suggestion: Option<String>,
}
```

- [ ] **Step 3: Add `#[derive(TS)]` to `Config` and `KeybindConfig`**

In `host/src/config.rs`, add `use ts_rs::TS;` and derive on the Config structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
#[serde(default)]
pub struct Config {
    // ... fields unchanged
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
#[serde(default)]
pub struct KeybindConfig {
    pub toggle_overlay: String,
}
```

- [ ] **Step 4: Regenerate TypeScript types**

```bash
cargo ts-rs export --output-directory desktop/src/generated
```

Verify new files: `ParseError.ts`, `Severity.ts`, `Config.ts`, `KeybindConfig.ts`

- [ ] **Step 5: Run tests and add a config handler test**

Add tests to `host/src/ws_server.rs`:

```rust
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
```

Run: `cargo test --workspace`

- [ ] **Step 6: Commit**

```bash
git add host/src/ws_server.rs host/src/omni/parser.rs host/src/config.rs
git commit -m "feat(host): add config.get/config.update WebSocket handlers, ts-rs exports for ParseError and Config"
```

---

## Task 2: IPC message layer — `sendAndWait` + IPC bridge

**Files:**
- Modify: `desktop/main/host-manager.ts`
- Modify: `desktop/main/main.ts`
- Modify: `desktop/main/preload.ts`
- Modify: `desktop/renderer/types/electron.d.ts`

- [ ] **Step 1: Add `sendAndWait` to `host-manager.ts`**

Add a method to `HostManager` that sends a WebSocket message and waits for a typed response:

```typescript
  /** Send a message and wait for a response with the matching type. */
  sendAndWait(msg: object, expectedType: string, timeoutMs = 5000): Promise<any> {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error('WebSocket not connected'));
        return;
      }

      const timer = setTimeout(() => {
        cleanup();
        reject(new Error(`Timeout waiting for ${expectedType}`));
      }, timeoutMs);

      const handler = (data: Buffer) => {
        try {
          const response = JSON.parse(data.toString());
          if (response.type === expectedType || response.type === 'error') {
            cleanup();
            if (response.type === 'error') {
              reject(new Error(response.message));
            } else {
              resolve(response);
            }
          }
        } catch { /* ignore malformed */ }
      };

      const cleanup = () => {
        clearTimeout(timer);
        this.ws?.removeListener('message', handler);
      };

      this.ws.on('message', handler);
      this.ws.send(JSON.stringify(msg));
    });
  }
```

- [ ] **Step 2: Add IPC handlers in `main.ts`**

In `desktop/main/main.ts`, add the `ws-message` handler and sensor data forwarding inside the `app.on("ready")` block, after host manager setup:

```typescript
  // IPC: request/response messages to host via WebSocket
  ipcMain.handle('ws-message', async (_event, msg: any) => {
    // Map request type to expected response type
    const responseTypes: Record<string, string> = {
      'status': 'status.data',
      'sensors.subscribe': 'sensors.subscribed',
      'file.list': 'file.list.result',
      'file.read': 'file.read.result',
      'file.write': 'file.write.result',
      'file.create': 'file.create.result',
      'file.delete': 'file.delete.result',
      'widget.parse': 'widget.parsed',
      'widget.apply': 'widget.applied',
      'widget.update': 'widget.updated',
      'config.get': 'config.data',
      'config.update': 'config.updated',
    };
    const expectedType = responseTypes[msg.type];
    if (!expectedType) {
      throw new Error(`Unknown message type: ${msg.type}`);
    }
    return hostManager.sendAndWait(msg, expectedType);
  });

  // Forward sensor data stream to renderer
  hostManager.on('message', (msg: any) => {
    if (msg.type === 'sensors.data') {
      mainWindow?.webContents.send('sensor-data', msg.snapshot);
    }
  });
```

Note: The `file.*` response types in the host currently use the same type names as the request (e.g., `file.list` returns `{ type: "file.list.result", ... }`). Check the actual response types from the Rust handler — they may just return the data directly without a `type` field. If so, the `sendAndWait` matching needs to be adapted. Read `host/src/workspace/file_api.rs` to confirm response formats and adjust the mapping accordingly.

- [ ] **Step 3: Update `preload.ts`**

Add `sendMessage` and `onSensorData` to the IPC bridge:

```typescript
import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("omni", {
  // Window controls
  minimizeWindow: () => ipcRenderer.send("window-minimize"),
  maximizeWindow: () => ipcRenderer.send("window-maximize"),
  closeWindow: () => ipcRenderer.send("window-close"),
  getResourcePath: (filename: string) =>
    ipcRenderer.sendSync("get-resource-path", filename),

  // Host status
  onHostStatus: (callback: (status: any) => void) => {
    ipcRenderer.on("host-status", (_event, status) => callback(status));
  },

  // Backend communication
  sendMessage: (msg: object) => ipcRenderer.invoke("ws-message", msg),

  // Sensor data stream
  onSensorData: (callback: (snapshot: any) => void) => {
    ipcRenderer.on("sensor-data", (_event, snapshot) => callback(snapshot));
  },
});
```

- [ ] **Step 4: Update `electron.d.ts` types**

Update `desktop/renderer/types/electron.d.ts`:

```typescript
interface OmniIpcBridge {
  // Window controls
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  getResourcePath: (filename: string) => string;

  // Host status
  onHostStatus: (callback: (status: any) => void) => void;

  // Backend communication
  sendMessage: (msg: object) => Promise<any>;

  // Sensor data stream
  onSensorData: (callback: (snapshot: any) => void) => void;
}

declare global {
  interface Window {
    omni?: OmniIpcBridge;
  }
}

export {};
```

- [ ] **Step 5: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 6: Commit**

```bash
git add desktop/main/ desktop/renderer/types/electron.d.ts
git commit -m "feat(desktop): IPC message layer — sendMessage, sendAndWait, sensor data forwarding"
```

---

## Task 3: BackendApi + sensor mapping

**Files:**
- Create: `desktop/renderer/lib/backend-api.ts`
- Create: `desktop/renderer/lib/sensor-mapping.ts`
- Create: `desktop/renderer/hooks/use-sensor-data.ts`
- Create: `desktop/renderer/hooks/use-backend.ts`

- [ ] **Step 1: Create `backend-api.ts`**

```typescript
import type { OmniFile } from '@/src/generated/OmniFile';
import type { ParseError } from '@/src/generated/ParseError';
import type { Config } from '@/src/generated/Config';

/** Typed wrapper around window.omni.sendMessage for all backend operations. */
export class BackendApi {
  private send(msg: object): Promise<any> {
    if (!window.omni?.sendMessage) {
      return Promise.reject(new Error('IPC bridge not available'));
    }
    return window.omni.sendMessage(msg);
  }

  // File operations
  async listFiles(): Promise<any> {
    return this.send({ type: 'file.list' });
  }

  async readFile(path: string): Promise<string> {
    const res = await this.send({ type: 'file.read', path });
    return res.content ?? '';
  }

  async writeFile(path: string, content: string): Promise<void> {
    await this.send({ type: 'file.write', path, content });
  }

  async createOverlay(name: string): Promise<void> {
    await this.send({ type: 'file.create', createType: 'overlay', name });
  }

  async createTheme(name: string): Promise<void> {
    await this.send({ type: 'file.create', createType: 'theme', name });
  }

  async deleteFile(path: string): Promise<void> {
    await this.send({ type: 'file.delete', path });
  }

  // Overlay operations
  async parseOverlay(source: string): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }> {
    const res = await this.send({ type: 'widget.parse', source });
    return { file: res.file ?? null, diagnostics: res.diagnostics ?? [] };
  }

  async applyOverlay(source: string): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }> {
    const res = await this.send({ type: 'widget.apply', source });
    return { file: res.file ?? null, diagnostics: res.diagnostics ?? [] };
  }

  // Config
  async getConfig(): Promise<Config> {
    const res = await this.send({ type: 'config.get' });
    return res.config;
  }

  async updateConfig(config: Config): Promise<void> {
    await this.send({ type: 'config.update', config });
  }

  // Status
  async getStatus(): Promise<any> {
    return this.send({ type: 'status' });
  }

  // Sensors
  async subscribeSensors(): Promise<void> {
    await this.send({ type: 'sensors.subscribe' });
  }
}
```

- [ ] **Step 2: Create `sensor-mapping.ts`**

```typescript
import type { SensorSnapshot } from '@/src/generated/SensorSnapshot';
import type { MetricValues } from '@/types/omni';

/** Map a SensorSnapshot (ts-rs generated from Rust) to the frontend MetricValues type. */
export function sensorSnapshotToMetrics(snapshot: SensorSnapshot): Partial<MetricValues> {
  return {
    fps: snapshot.frame.fps,
    frametime: snapshot.frame.frame_time_ms,
    'frame.1pct': snapshot.frame.frame_time_1percent_ms,
    'gpu.usage': snapshot.gpu.usage_percent,
    'gpu.temp': snapshot.gpu.temp_c,
    'gpu.clock': snapshot.gpu.core_clock_mhz,
    'gpu.vram.used': snapshot.gpu.vram_used_mb,
    'gpu.vram.total': snapshot.gpu.vram_total_mb,
    'gpu.power': snapshot.gpu.power_draw_w,
    'gpu.fan': snapshot.gpu.fan_speed_percent,
    'cpu.usage': snapshot.cpu.total_usage_percent,
    'cpu.temp': snapshot.cpu.package_temp_c,
    'ram.usage': snapshot.ram.usage_percent,
  };
}
```

- [ ] **Step 3: Create `use-sensor-data.ts` hook**

```typescript
import { useState, useEffect } from 'react';
import type { SensorSnapshot } from '@/src/generated/SensorSnapshot';

/** Hook that subscribes to live sensor data from the host. */
export function useSensorData(): SensorSnapshot | null {
  const [snapshot, setSnapshot] = useState<SensorSnapshot | null>(null);

  useEffect(() => {
    window.omni?.onSensorData((data) => {
      setSnapshot(data as SensorSnapshot);
    });
  }, []);

  return snapshot;
}
```

- [ ] **Step 4: Create `use-backend.ts` hook**

```typescript
import { useMemo } from 'react';
import { BackendApi } from '@/lib/backend-api';

/** Singleton BackendApi hook. */
export function useBackend(): BackendApi {
  return useMemo(() => new BackendApi(), []);
}
```

- [ ] **Step 5: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 6: Commit**

```bash
git add desktop/renderer/lib/backend-api.ts desktop/renderer/lib/sensor-mapping.ts desktop/renderer/hooks/use-sensor-data.ts desktop/renderer/hooks/use-backend.ts
git commit -m "feat(desktop): BackendApi, sensor mapping, and data hooks"
```

---

## Task 4: Refactor types and state management for backend-authoritative model

**Files:**
- Modify: `desktop/renderer/types/omni.ts`
- Modify: `desktop/renderer/hooks/use-omni-state.tsx`
- Delete: `desktop/renderer/lib/storage-adapter.ts`

This is the largest and most complex task. The `Overlay` type simplifies (no more synthetic IDs), `AppState` and `AppAction` are refactored, and `use-omni-state.tsx` loads/saves via `BackendApi` instead of localStorage.

- [ ] **Step 1: Refactor `types/omni.ts`**

Key changes:
- `Overlay` becomes `{ name: string; content: string }` — identity is the name (folder name on disk)
- Remove `StorageAdapter` interface
- `GameAssignment` no longer needed as a separate type — game assignments live in `Config.overlay_by_game`
- `AppState.selectedOverlayId` → `AppState.selectedOverlayName`
- `AppState.activeOverlayId` → removed (comes from config)
- `AppState.gameAssignments` → removed (comes from config)
- Add `AppState.config: Config | null` (loaded from backend)
- Add `AppState.connected: boolean`
- Keep `MetricValues`, `EditorTab`, `ParsedWidget`, `ThemeImport`, `DEFAULT_METRICS`
- Remove `SAMPLE_DEFAULT_OVERLAY` (overlays come from backend)

Update `AppAction` union to match new state shape.

- [ ] **Step 2: Refactor `use-omni-state.tsx`**

Key changes:
- On mount: call `backendApi.listFiles()` to get overlay names, `backendApi.readFile()` for each overlay content, `backendApi.getConfig()` for config
- `createOverlay(name)` → calls `backendApi.createOverlay(name)` then reloads list
- `deleteOverlay(name)` → calls `backendApi.deleteFile("overlays/{name}")` then reloads list
- `saveCurrentOverlay()` → calls `backendApi.writeFile("overlays/{name}/overlay.omni", content)`. If this overlay is the active one (from config), also calls `backendApi.applyOverlay(content)`.
- `setAsActive(name)` → reads config, sets `config.active_overlay = name`, calls `backendApi.updateConfig(config)`
- `assignToGame(overlayName, executable)` → reads config, sets `config.overlay_by_game[executable] = overlayName`, calls `backendApi.updateConfig(config)`
- `removeGameAssignment(executable)` → reads config, deletes `config.overlay_by_game[executable]`, calls `backendApi.updateConfig(config)`
- Remove all `getStorageAdapter()` references
- Listen for `window.omni.onHostStatus` to update `connected` state

- [ ] **Step 3: Delete `storage-adapter.ts`**

Confirm with user before deleting, but this file is fully replaced by `backend-api.ts`.

- [ ] **Step 4: Update all component imports**

Any component that references `overlay.id` must change to `overlay.name`. Any component that references `state.activeOverlayId` must change to `state.config?.active_overlay`. Any component that references `state.gameAssignments` must change to `state.config?.overlay_by_game`.

Files to update:
- `header.tsx` — overlay selector, set active, delete
- `widget-panel.tsx` — overlay name reference
- `status-bar.tsx` — active overlay check, game assignment count
- `game-assignments-dialog.tsx` — read/write game assignments via config
- `editor-panel.tsx` — save overlay calls

- [ ] **Step 5: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 6: Commit**

```bash
git add -A desktop/renderer/
git commit -m "refactor(desktop): backend-authoritative state — overlays by name, config from host, no localStorage"
```

---

## Task 5: Monaco diagnostics — debounced parse + error markers

**Files:**
- Modify: `desktop/renderer/components/omni/editor-panel.tsx`

- [ ] **Step 1: Add debounced parse and marker setting**

In `editor-panel.tsx`, add a debounced effect that sends content to `widget.parse` and sets Monaco markers from the response diagnostics.

Add imports:
```typescript
import { useBackend } from '@/hooks/use-backend';
import type { ParseError } from '@/src/generated/ParseError';
```

Add the debounced parse effect inside the `EditorPanel` component:

```typescript
  const backend = useBackend();
  const monacoRef = useRef<typeof import('monaco-editor') | null>(null);

  // Capture monaco instance on mount
  const handleBeforeMount: BeforeMount = useCallback((monaco) => {
    monaco.editor.defineTheme('omni-dark', omniDarkTheme);
    registerOmniLanguage(monaco);
    monacoRef.current = monaco;
  }, []);

  // Debounced parse for diagnostics
  useEffect(() => {
    const content = displayContent ?? '';
    if (!content || !editorRef.current || !monacoRef.current) return;

    const timer = setTimeout(async () => {
      try {
        const { diagnostics } = await backend.parseOverlay(content);
        const model = editorRef.current?.getModel();
        if (model && monacoRef.current) {
          const markers = diagnostics.map((d: ParseError) => ({
            startLineNumber: d.line,
            startColumn: d.column,
            endLineNumber: d.line,
            endColumn: d.column + 10,
            severity: d.severity === 'Error'
              ? monacoRef.current!.MarkerSeverity.Error
              : monacoRef.current!.MarkerSeverity.Warning,
            message: d.message + (d.suggestion ? ` — ${d.suggestion}` : ''),
          }));
          monacoRef.current.editor.setModelMarkers(model, 'omni', markers);
        }
      } catch {
        // Host not connected — clear markers
        const model = editorRef.current?.getModel();
        if (model && monacoRef.current) {
          monacoRef.current.editor.setModelMarkers(model, 'omni', []);
        }
      }
    }, 400);

    return () => clearTimeout(timer);
  }, [displayContent, backend]);
```

- [ ] **Step 2: Update save handler to apply when active**

Update `handleSave` to call `applyOverlay` when editing the active overlay:

```typescript
  const handleSave = useCallback(async () => {
    if (!currentOverlay) return;
    const content = displayContent ?? '';
    const overlayPath = `overlays/${currentOverlay.name}/overlay.omni`;

    await backend.writeFile(overlayPath, content);

    // If this is the active overlay, also push to live game
    const isActiveOverlay = state.config?.active_overlay === currentOverlay.name;
    if (isActiveOverlay) {
      await backend.applyOverlay(content);
    }

    dispatch({ type: 'SET_DIRTY', payload: false });
  }, [currentOverlay, displayContent, state.config, backend, dispatch]);
```

- [ ] **Step 3: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/components/omni/editor-panel.tsx
git commit -m "feat(desktop): Monaco diagnostics from backend parse + auto-apply on save"
```

---

## Task 6: Live/Simulate preview toggle + sensor data

**Files:**
- Modify: `desktop/renderer/components/omni/preview-panel.tsx`
- Modify: `desktop/renderer/components/omni/metric-simulator.tsx`

- [ ] **Step 1: Add Live/Simulate toggle to preview panel**

In `preview-panel.tsx`, add:
- A `mode` state: `'live' | 'simulate'`
- Use `useSensorData()` hook for live data
- Use `sensorSnapshotToMetrics()` to map snapshot to MetricValues
- Toggle button in the header
- When Live: use mapped sensor data for preview, hide MetricSimulator
- When Simulate: use `state.previewMetrics` (current behavior), show MetricSimulator

```typescript
import { useSensorData } from '@/hooks/use-sensor-data';
import { sensorSnapshotToMetrics } from '@/lib/sensor-mapping';
import { useState, useMemo } from 'react';
import type { MetricValues } from '@/types/omni';
import { DEFAULT_METRICS } from '@/types/omni';

// Inside PreviewPanel:
const [mode, setMode] = useState<'live' | 'simulate'>('live');
const sensorData = useSensorData();
const isConnected = sensorData !== null;

// Force simulate when disconnected
const effectiveMode = isConnected ? mode : 'simulate';

// Determine which metrics to use for preview
const previewMetrics: MetricValues = useMemo(() => {
  if (effectiveMode === 'live' && sensorData) {
    return { ...DEFAULT_METRICS, ...sensorSnapshotToMetrics(sensorData) };
  }
  return state.previewMetrics;
}, [effectiveMode, sensorData, state.previewMetrics]);
```

Add a toggle button in the header area:
```tsx
<button
  onClick={() => setMode(m => m === 'live' ? 'simulate' : 'live')}
  disabled={!isConnected}
  className={cn(
    "text-xs px-2 py-0.5 rounded transition-colors",
    effectiveMode === 'live'
      ? "bg-[#22C55E]/20 text-[#22C55E]"
      : "bg-[#A855F7]/20 text-[#A855F7]"
  )}
>
  {effectiveMode === 'live' ? 'Live' : 'Simulate'}
</button>
```

- [ ] **Step 2: Conditionally show MetricSimulator**

Only render `<MetricSimulator />` when in Simulate mode:

```tsx
{effectiveMode === 'simulate' && <MetricSimulator />}
```

- [ ] **Step 3: Subscribe to sensors on mount**

In `preview-panel.tsx` or `use-omni-state.tsx`, call `backendApi.subscribeSensors()` when connected. This can go in the state hook's initialization effect.

- [ ] **Step 4: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 5: Commit**

```bash
git add desktop/renderer/components/omni/preview-panel.tsx desktop/renderer/components/omni/metric-simulator.tsx
git commit -m "feat(desktop): Live/Simulate preview toggle with real sensor data"
```

---

## Task 7: Status bar connection status

**Files:**
- Modify: `desktop/renderer/components/omni/status-bar.tsx`

- [ ] **Step 1: Wire real connection status**

Replace the hardcoded "Ready" indicator with actual host connection state from `use-omni-state`:

```tsx
// Replace the hardcoded green dot with:
const connectionColor = state.connected ? '#22C55E' : '#EF4444';
const connectionLabel = state.connected ? 'CONNECTED' : 'DISCONNECTED';

// In the JSX:
<Circle className={`h-2 w-2`} style={{ fill: connectionColor, color: connectionColor }} />
<span className="text-[#71717A] uppercase tracking-wider">{connectionLabel}</span>
```

Also show the injected game from host status if available.

- [ ] **Step 2: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 3: Commit**

```bash
git add desktop/renderer/components/omni/status-bar.tsx
git commit -m "feat(desktop): real connection status in status bar"
```

---

## Task 8: Game assignments dialog → backend config

**Files:**
- Modify: `desktop/renderer/components/omni/game-assignments-dialog.tsx`

- [ ] **Step 1: Wire to config.update**

The game assignments dialog currently calls `assignToGame` and `removeGameAssignment` from the state hook. These methods need to be updated in Task 4 to read/merge/write config via `backendApi.updateConfig()`. This task verifies the dialog works with the refactored state.

If the dialog references `state.gameAssignments`, update it to read from `state.config?.overlay_by_game` (which is a `Record<string, string>` mapping `executable → overlayName`).

The dialog's "Add" button should add to this map, and "Remove" should delete from it, then persist via config update.

- [ ] **Step 2: Verify build**

```bash
cd desktop && npm run build
```

- [ ] **Step 3: Commit**

```bash
git add desktop/renderer/components/omni/game-assignments-dialog.tsx
git commit -m "feat(desktop): game assignments dialog reads/writes backend config"
```

---

## Task 9: Full verification

- [ ] **Step 1: Rust tests**
```bash
cargo test --workspace
```

- [ ] **Step 2: Clippy**
```bash
cargo clippy --workspace --all-targets
```

- [ ] **Step 3: Nextron build**
```bash
cd desktop && npm run build
```

- [ ] **Step 4: Cargo fmt**
```bash
cargo fmt --all
```

- [ ] **Step 5: Commit if needed**
```bash
git add -A && git commit -m "cleanup: fmt + verification"
```
