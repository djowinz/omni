# Backend Integration Design Spec

## Overview

Wire the Electron desktop app's frontend to the existing `omni-host.exe` WebSocket backend, replacing all localStorage usage with backend API calls. The backend is authoritative — overlays are folders on disk, config is `config.json`, sensors are live data. The frontend derives its state from backend responses and the ts-rs generated types.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Data authority | Backend-authoritative | Overlays are folders in `%APPDATA%\Omni\overlays/`, not frontend objects. No synthetic IDs. |
| IPC pattern | `ipcRenderer.invoke` / `ipcMain.handle` | Promise-based request/response through the Electron bridge |
| Config updates | Full config read/merge/write | Frontend merges changes and sends full config. No partial update endpoints. No race conditions. |
| Save behavior | Save + auto-apply when active | `file.write` + `widget.apply` when editing the active overlay. Just `file.write` otherwise. |
| Preview metrics | Toggle Live/Simulate | Live uses `SensorSnapshot` from host. Simulate uses sliders. Default to Live when connected. |
| Parsing | Debounced on keystroke + on save | 400ms debounce for Monaco squiggles. Full parse on save for apply flow. |
| Type sharing | ts-rs generated types only | `SensorSnapshot`, `OmniFile`, `ParseError` etc. from `desktop/src/generated/`. No duplication. |

---

## Architecture: IPC Message Layer

The renderer cannot access the WebSocket directly (context isolation). All backend communication goes through an IPC bridge:

```
Renderer (window.omni.sendMessage)
  → preload.ts (ipcRenderer.invoke('ws-message', msg))
    → main.ts (ipcMain.handle('ws-message'))
      → hostManager.sendAndWait(msg)
        → WebSocket to omni-host.exe:9473
        ← WebSocket response (matched by type)
      ← Promise resolves
    ← IPC response
  ← Renderer receives typed response
```

### `hostManager.sendAndWait(msg)` behavior:
- Sends JSON message on the WebSocket
- Registers a one-time listener for the expected response type (e.g., `file.list` expects `file.list.result`)
- Returns a Promise that resolves with the response payload
- Rejects after 5 seconds if no response
- If WebSocket is disconnected, rejects immediately

### Response type mapping:
| Request `type` | Expected response `type` |
|----------------|--------------------------|
| `status` | `status.data` |
| `sensors.subscribe` | `sensors.subscribed` |
| `file.list` | `file.list.result` |
| `file.read` | `file.read.result` |
| `file.write` | `file.write.result` |
| `file.create` | `file.create.result` |
| `file.delete` | `file.delete.result` |
| `widget.parse` | `widget.parsed` |
| `widget.apply` | `widget.applied` |
| `config.get` | `config.data` |
| `config.update` | `config.updated` |

---

## New Backend Endpoints

Two new WebSocket message types in `host/src/ws_server.rs`:

### `config.get`
Request: `{ "type": "config.get" }`
Response: `{ "type": "config.data", "config": { "active_overlay": "Default", "overlay_by_game": {...}, "keybinds": {...}, "exclude": [...], "include": [...], "game_directories": [...] } }`

### `config.update`
Request: `{ "type": "config.update", "config": { ...full config object... } }`
Response: `{ "type": "config.updated" }`

The handler writes the config to `config.json` at the data_dir path. The host's existing file watcher detects the change and reloads config (existing hot-reload behavior).

### `ParseError` type export
Add `#[derive(TS)]` to `ParseError` and `Severity` in `host/src/omni/parser.rs` so the frontend gets type-safe diagnostics. The generated types go to `desktop/src/generated/`.

---

## Storage Layer Rewrite

### Current: `StorageAdapter` (localStorage)
```typescript
interface StorageAdapter {
  loadOverlays: () => Promise<Overlay[]>;
  saveOverlay: (overlay: Overlay) => Promise<void>;
  deleteOverlay: (id: string) => Promise<void>;
  loadGameAssignments: () => Promise<GameAssignment[]>;
  saveGameAssignments: (assignments: GameAssignment[]) => Promise<void>;
  getActiveOverlayId: () => Promise<string | null>;
  setActiveOverlayId: (id: string | null) => Promise<void>;
}
```

### New: `BackendApi`
```typescript
interface BackendApi {
  // File operations
  listFiles(): Promise<FileListResponse>;
  readFile(path: string): Promise<string>;
  writeFile(path: string, content: string): Promise<void>;
  createOverlay(name: string): Promise<void>;
  createTheme(name: string): Promise<void>;
  deleteFile(path: string): Promise<void>;

  // Overlay operations
  parseOverlay(source: string): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }>;
  applyOverlay(source: string): Promise<{ file: OmniFile | null; diagnostics: ParseError[] }>;

  // Config
  getConfig(): Promise<Config>;
  updateConfig(config: Config): Promise<void>;

  // Status
  getStatus(): Promise<StatusResponse>;
  subscribeSensors(): Promise<void>;
}
```

### Overlay identity
- An overlay IS its folder name: `"Default"`, `"Valorant Competitive"`, `"CS2 Minimal"`
- No synthetic IDs — `selectedOverlayId` becomes `selectedOverlayName`
- `isDefault` is just `name === "Default"`
- Content comes from `readFile("overlays/{name}/overlay.omni")`

### What gets refactored
- `types/omni.ts` — `Overlay` type simplifies to `{ name: string, content: string }`, `StorageAdapter` removed, `GameAssignment` moves to config
- `lib/storage-adapter.ts` — replaced entirely with `lib/backend-api.ts`
- `hooks/use-omni-state.tsx` — refactored to use `BackendApi`, overlay identity by name, config from backend
- All components that reference `overlay.id` switch to `overlay.name`
- Components import generated types from `@/src/generated/` where applicable

---

## Preload API Expansion

### Current `window.omni`:
```typescript
{
  onHostStatus: (callback) => void;
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  getResourcePath: (filename: string) => string;
}
```

### New `window.omni`:
```typescript
{
  // Window controls (unchanged)
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  getResourcePath: (filename: string) => string;

  // Host status (unchanged)
  onHostStatus: (callback: (status: HostStatus) => void) => void;

  // Backend communication (new)
  sendMessage: (msg: object) => Promise<any>;

  // Sensor data stream (new)
  onSensorData: (callback: (snapshot: SensorSnapshot) => void) => void;
}
```

The `sendMessage` method is the single entry point for all request/response communication. The `BackendApi` class in the renderer wraps it with typed methods.

`onSensorData` is a separate push channel (not request/response) — the host pushes sensor snapshots at 1Hz after `sensors.subscribe`.

---

## Sensor Data + Preview Toggle

### Sensor flow:
1. On connect, renderer calls `backendApi.subscribeSensors()` via `sendMessage`
2. Host streams `sensors.data` at 1Hz
3. Main process forwards to renderer via IPC channel `sensor-data`
4. `useSensorData()` hook receives updates, exposes latest `SensorSnapshot` (ts-rs generated type)

### Preview panel modes:
- **Live mode**: preview uses `SensorSnapshot` mapped to `MetricValues`. Sliders hidden.
- **Simulate mode**: preview uses slider-driven `MetricValues`. Sliders visible.
- Toggle switch in preview header. Default: Live when connected, Simulate when disconnected.
- When disconnected, forced to Simulate — toggle disabled.

### Mapping function:
```typescript
function sensorSnapshotToMetrics(snapshot: SensorSnapshot): MetricValues {
  return {
    fps: snapshot.frame.fps,
    frametime: snapshot.frame.frame_time_ms,
    'frame.1pct': snapshot.frame.frame_time_1percent_ms,
    'gpu.usage': snapshot.gpu.usage_percent,
    'gpu.temp': snapshot.gpu.temp_c,
    'gpu.clock': snapshot.gpu.core_clock_mhz,
    'gpu.vram.used': snapshot.gpu.vram_used_mb,
    'gpu.power': snapshot.gpu.power_draw_w,
    'cpu.usage': snapshot.cpu.total_usage_percent,
    'cpu.temp': snapshot.cpu.package_temp_c,
    'ram.usage': snapshot.ram.usage_percent,
    // ... remaining fields
  };
}
```

Uses the generated `SensorSnapshot` type — no duplication.

---

## Monaco Diagnostics

### Flow:
1. User types → 400ms debounce timer starts
2. On debounce fire: send content to host via `widget.parse`
3. Host returns `{ file: OmniFile | null, diagnostics: ParseError[] }`
4. Map `ParseError[]` to Monaco markers:
```typescript
diagnostics.map(d => ({
  startLineNumber: d.line,
  startColumn: d.column,
  endLineNumber: d.line,
  endColumn: d.column + 1,
  severity: d.severity === 'Error'
    ? monaco.MarkerSeverity.Error
    : monaco.MarkerSeverity.Warning,
  message: d.message + (d.suggestion ? ` — ${d.suggestion}` : ''),
}))
```
5. Set markers via `monaco.editor.setModelMarkers(model, 'omni', markers)`

### On save:
- Call `backendApi.applyOverlay(source)` (if editing the active overlay)
- Or call `backendApi.writeFile(path, content)` (if editing a non-active overlay)
- Set markers from the response diagnostics either way

### `ParseError` ts-rs export:
Add `#[derive(TS)]` with `#[ts(export)]` to `ParseError` and `Severity` in `host/src/omni/parser.rs`. The generated types are used directly in the frontend — no hand-rolled duplicate.

---

## Files Modified

### Rust (backend)
- `host/src/ws_server.rs` — add `config.get` and `config.update` handlers
- `host/src/omni/parser.rs` — add `#[derive(TS)]` to `ParseError`, `Severity`
- `host/Cargo.toml` — ts-rs already added (no change needed)

### Electron main process
- `desktop/main/preload.ts` — add `sendMessage`, `onSensorData`
- `desktop/main/main.ts` — add `ws-message` IPC handler, sensor data forwarding
- `desktop/main/host-manager.ts` — add `sendAndWait()` method

### Electron renderer
- `desktop/renderer/lib/backend-api.ts` — NEW: typed wrapper around `window.omni.sendMessage`
- `desktop/renderer/lib/storage-adapter.ts` — DELETE (replaced by backend-api.ts)
- `desktop/renderer/lib/sensor-mapping.ts` — NEW: `SensorSnapshot` → `MetricValues` mapping
- `desktop/renderer/types/omni.ts` — simplify `Overlay` type, remove `StorageAdapter`, update `AppState`/`AppAction`
- `desktop/renderer/types/electron.d.ts` — update `OmniIpcBridge` with new methods
- `desktop/renderer/hooks/use-omni-state.tsx` — refactor to use `BackendApi`, name-based overlay identity
- `desktop/renderer/hooks/use-sensor-data.ts` — NEW: hook for live sensor data subscription
- `desktop/renderer/components/omni/preview-panel.tsx` — add Live/Simulate toggle
- `desktop/renderer/components/omni/metric-simulator.tsx` — conditionally shown based on mode
- `desktop/renderer/components/omni/editor-panel.tsx` — add debounced parse + Monaco markers
- `desktop/renderer/components/omni/status-bar.tsx` — wire to real connection status
- `desktop/renderer/components/omni/header.tsx` — overlay selector uses backend file list
- `desktop/renderer/components/omni/game-assignments-dialog.tsx` — writes to backend config

### Generated types (regenerated via cargo ts-rs export)
- `desktop/src/generated/ParseError.ts` — NEW
- `desktop/src/generated/Severity.ts` — NEW

---

## Out of Scope

- Overlay drag/drop visual editor (Phase 13)
- Monaco IntelliSense/autocomplete for `.omni` format (Phase 13)
- CSS variable autocomplete (Phase 13)
- Multi-client WebSocket support (single Electron app connects)
- Offline mode with sync (if disconnected, editing is blocked with reconnection message)
