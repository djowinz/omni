# Omni — Game Overlay Hardware Monitor Design Spec

## Overview

A Rust-based hardware monitoring overlay that renders on top of any game, including exclusive fullscreen titles. Users define overlay layout using `.omni` files — a component format with `<widget>` blocks containing standard HTML (`<div>`, `<span>`) + CSS with `{sensor.path}` data interpolation. The system uses DLL injection and graphics API hooking to render directly into the game's swap chain via D2D1/DirectWrite.

An Electron-based visual editor (future Phase 11) provides the main user interface, communicating with the host via a local WebSocket API.

---

## Architecture

```
┌─────────────────────┐    WebSocket     ┌──────────────────────┐
│   Electron App      │ ◄──────────────► │   Host Process       │
│   (future Phase 11) │   JSON on 9473   │   (omni-host.exe)    │
└─────────────────────┘                  │                      │
                                         │  - Sensor polling    │
                                         │  - .omni parsing     │
                                         │  - CSS resolution    │
                                         │  - Widget resolver   │
                                         │  - Process scanner   │
                                         │  - WebSocket server  │
                                         │  - Workspace mgmt    │
                                         └──────────┬───────────┘
                                                    │ Shared Memory
                                                    │ (double-buffered)
                                         ┌──────────▼───────────┐
                                         │   Game Process       │
                                         │  ┌────────────────┐  │
                                         │  │ Injected DLL   │  │
                                         │  │ (omni_overlay)  │  │
                                         │  │                │  │
                                         │  │ - Hook Present │  │
                                         │  │ - D2D render   │  │
                                         │  │ - Frame stats  │  │
                                         │  └────────────────┘  │
                                         └──────────────────────┘
```

### Host Process (`host/`)

Runs as a background service or CLI tool. Three modes:
- `omni-host --service` — Service mode for Electron (auto-discovers DLL, WebSocket on port 9473)
- `omni-host --watch <DLL_PATH>` — CLI mode for developers
- `omni-host --stop` — Eject DLL from all games, kill running host instances

Responsibilities:
- **Sensor polling**: Background thread reads CPU (sysinfo), GPU (NVML), RAM (sysinfo), CPU temp (WMI) at configurable intervals
- **Widget parsing**: Loads `.omni` files via `quick-xml`, parses CSS via hand-written parser (lightningcss available for future advanced features)
- **Widget resolution**: `OmniResolver` walks the HTML tree, resolves CSS (theme → scoped → inline), interpolates `{sensor.path}`, emits `Vec<ComputedWidget>`
- **Workspace management**: Organized overlay folders at `%APPDATA%\Omni/overlays/`, shared themes at `themes/`, game-specific overlay mapping
- **WebSocket server**: JSON API on localhost:9473 for Electron communication (sensor streaming, file CRUD, widget parse/apply)
- **IPC writer**: Writes sensor snapshots and computed widgets to shared memory (lock-free double-buffered)
- **DLL injection**: `CreateRemoteThread` + `LoadLibraryW`, with graceful ejection via `omni_shutdown` export
- **Process scanning**: Polls for new game processes, two-tier injection (new vs pre-existing), game directory detection

### Overlay DLL (`overlay-dll/`)

A `cdylib` named `omni_overlay.dll` injected into the game process. Minimal and rock-solid.

Responsibilities:
- **Graphics API hooking**: Hooks `IDXGISwapChain::Present/Present1/ResizeBuffers` via minhook. Supports DX11 (direct) and DX12 (via D3D11On12)
- **Overlay rendering**: D2D1/DirectWrite renders widgets from shared memory data
- **Frame timing**: `FrameStats` module measures FPS, frame time, percentiles from Present hook timestamps via `QueryPerformanceCounter` (no ETW, no admin privileges)
- **IPC reader**: Reads sensor data and computed layout from shared memory every frame
- **Graceful shutdown**: `omni_shutdown` export disables hooks, releases D3D resources, calls `FreeLibraryAndExitThread`

### Shared Library (`shared/`)

`#[repr(C)]` types shared between host and DLL:
- `SensorSnapshot`, `CpuData`, `GpuData`, `RamData`, `FrameData`
- `ComputedWidget`, `WidgetType`, `SensorSource`
- `SharedOverlayState` (double-buffered shared memory layout)
- IPC protocol constants, helper functions (`write_fixed_str`, `read_fixed_str`)

---

## Actual Project Structure

```
repo/
  Cargo.toml                   # Workspace root

  host/
    Cargo.toml
    src/
      main.rs                  # CLI modes, main loop orchestrator
      config.rs                # Config struct, load/save, game directories
      scanner.rs               # Process enumeration, DLL detection, injection decisions
      injector/
        mod.rs                 # CreateRemoteThread injection + PE export-based ejection
      sensors/
        mod.rs                 # SensorPoller background thread, mpsc channel
        cpu.rs                 # sysinfo CPU usage + frequencies
        cpu_temp.rs            # WMI MSAcpi_ThermalZoneTemperature
        gpu.rs                 # NVML FFI (nvml.dll) — all GPU sensors
        ram.rs                 # sysinfo RAM usage
      omni/
        mod.rs                 # Module re-exports
        types.rs               # OmniFile, Widget, HtmlNode, ResolvedStyle (JSON-serializable)
        parser.rs              # quick-xml .omni file parser
        css.rs                 # CSS parsing, selector matching, variable resolution
        resolver.rs            # OmniResolver: (OmniFile, SensorSnapshot) → Vec<ComputedWidget>
        interpolation.rs       # {sensor.path} text/style interpolation
        sensor_map.rs          # Maps "cpu.usage" strings to SensorSource + values
        default.rs             # Built-in default .omni content + dark.css theme
      workspace/
        mod.rs                 # Module declarations
        structure.rs           # Folder creation, migration, theme/overlay path resolution
        overlay_resolver.rs    # Game exe → overlay folder resolution chain
        file_api.rs            # File CRUD for WebSocket API (path-safe)
      ipc/
        mod.rs                 # SharedMemoryWriter (CreateFileMappingW)
      ws_server.rs             # WebSocket server (tungstenite, localhost:9473)

  overlay-dll/
    Cargo.toml                 # [lib] name = "omni_overlay", crate-type = ["cdylib"]
    src/
      lib.rs                   # DllMain + omni_shutdown export
      hook.rs                  # Vtable discovery (WARP adapter), hook installation
      present.rs               # Present/Present1/ResizeBuffers hooks, frame stats integration
      renderer.rs              # D2D1/DirectWrite renderer (DX11 + DX12 via D3D11On12)
      frame_stats.rs           # Ring buffer FPS/frame time computation from QPC
      logging.rs               # File-based logging to %TEMP%
      ipc/
        mod.rs                 # SharedMemoryReader (OpenFileMappingW)

  shared/
    Cargo.toml
    src/
      lib.rs
      sensor_types.rs          # SensorSnapshot, CpuData, GpuData, RamData, FrameData
      widget_types.rs          # ComputedWidget, WidgetType, SensorSource
      ipc_protocol.rs          # SharedOverlayState, double-buffer, constants
```

---

## Workspace & Config

### Folder Structure

```
%APPDATA%\Omni\
  config.json                    # Host settings
  themes/                        # Shared themes (available to all overlays)
    dark.css
  overlays/
    Default/                     # Built-in default overlay
      overlay.omni
    [User Overlay Name]/         # Each overlay = own folder
      overlay.omni
      [local-theme].css          # Optional local theme
```

### config.json

```json
{
  "active_overlay": "Default",
  "overlay_by_game": {
    "valorant.exe": "Valorant Competitive",
    "cs2.exe": "CS2 Minimal"
  },
  "keybinds": {
    "toggle_overlay": "F12"
  },
  "exclude": ["chrome.exe", "discord.exe", "...60+ defaults"],
  "include": [],
  "game_directories": ["steamapps\\common\\", "epic games\\", "...auto-detected"]
}
```

### Overlay Resolution Chain

1. Check `overlay_by_game` for running game's exe (case-insensitive)
2. Fall back to `active_overlay`
3. Fall back to `"Default"`

### Theme Resolution

1. Look in overlay's own folder first (local theme)
2. Fall back to `themes/` folder (shared theme)

---

## Widget File Format (`.omni`)

### Structure

A `.omni` file contains `<widget>` blocks, each with `<template>` (HTML) and `<style>` (CSS):

```xml
<theme src="dark.css" />

<widget id="system-stats" name="System Stats" enabled="true">
  <template>
    <div class="panel" style="position: fixed; top: 10px; right: 10px;">
      <span class="value">CPU: {cpu.usage}%</span>
      <span class="value">GPU: {gpu.usage}% | {gpu.temp}°C</span>
      <span class="value">RAM: {ram.usage}%</span>
    </div>
  </template>
  <style>
    .panel {
      background: var(--bg);
      border-radius: 8px;
      padding: 10px;
      display: flex;
      flex-direction: column;
      gap: 4px;
    }
    .value {
      color: var(--text);
      font-size: 14px;
    }
  </style>
</widget>

<widget id="fps-counter" name="FPS Counter" enabled="true">
  <template>
    <div style="position: fixed; bottom: 10px; left: 10px;">
      <span style="color: var(--accent); font-size: 24px; font-weight: bold;">{fps}</span>
    </div>
  </template>
  <style></style>
</widget>
```

### Widget Attributes

- `id` — unique identifier (required, used by Electron API)
- `name` — human-readable display name (required, shown in UI)
- `enabled` — toggle visibility (default `true`)

### HTML Elements

Standard HTML elements within `<template>`:
- `<div>` — block container (positioning, flexbox, backgrounds)
- `<span>` — inline text content

Both support `class`, `id`, and `style` (inline CSS) attributes.

### Sensor Interpolation

`{sensor.path}` expressions in text content and style attribute values:

```
{cpu.usage}         CPU usage percentage
{cpu.temp}          CPU temperature (°C or N/A)
{gpu.usage}         GPU utilization percentage
{gpu.temp}          GPU temperature (°C)
{gpu.clock}         GPU core clock (MHz)
{gpu.mem-clock}     GPU memory clock (MHz)
{gpu.vram}          VRAM used/total (e.g., "4096/12288")
{gpu.vram.used}     VRAM used (MB)
{gpu.vram.total}    VRAM total (MB)
{gpu.power}         GPU power draw (W)
{gpu.fan}           GPU fan speed (%)
{ram.usage}         RAM usage percentage
{ram.used}          RAM used (MB)
{ram.total}         RAM total (MB)
{fps}               Frames per second (computed by DLL)
{frame-time}        Latest frame time (ms)
{frame-time.avg}    Average frame time (ms)
{frame-time.1pct}   1% low frame time (ms)
{frame-time.01pct}  0.1% low frame time (ms)
```

### CSS Support (Current)

| Category | Properties |
|----------|-----------|
| **Position** | `position`, `top`, `right`, `bottom`, `left` |
| **Size** | `width`, `height` |
| **Visual** | `background` (solid color, rgba), `opacity`, `border-radius` |
| **Typography** | `font-family`, `font-size`, `font-weight`, `color` |
| **Flexbox** | `display`, `flex-direction`, `justify-content`, `align-items`, `gap` |
| **Spacing** | `padding`, `margin` |
| **Variables** | `--custom-property` on `:root`, referenced via `var(--prop)` |

### CSS Selectors (Current)

- Class selectors: `.value`
- ID selectors: `#fps`
- Element selectors: `div`, `span`
- `:root` pseudo-class (for CSS variables)

### Style Cascade Order (lowest to highest)

1. Theme file (`:root` variables from `<theme src="...">`)
2. Widget `<style>` block (scoped rules)
3. Inline `style` attribute

---

## Sensor Data Sources

| Data | Source | Notes |
|------|--------|-------|
| CPU usage, per-core, frequency | `sysinfo` crate | — |
| CPU temperature | WMI `MSAcpi_ThermalZoneTemperature` | N/A on many systems (needs kernel driver for reliable MSR access) |
| GPU usage, temp, clocks, VRAM, power, fan | NVML (`nvml.dll`) FFI | NVIDIA only, ships with driver |
| RAM usage/total | `sysinfo` crate | — |
| RAM frequency, timings, temp | Not implemented | Needs LHM/WMI |
| FPS, frame time, percentiles | Present hook + `QueryPerformanceCounter` | Measured in DLL, no admin required |

### Graceful Degradation

Unavailable sensors display `"N/A"`. Sensor binding ledger logs initialization status via `tracing`:

```
INFO sysinfo: CPU sensor initialized core_count=16
INFO NVML: initialized gpu_name="NVIDIA GeForce RTX 5090"
WARN WMI: failed to connect to root\WMI — CPU temperature unavailable
INFO Sensor polling started
```

---

## WebSocket API (localhost:9473)

| Endpoint | Direction | Purpose |
|----------|-----------|---------|
| `sensors.subscribe` | Client → Host | Subscribe to 1Hz sensor data stream |
| `sensors.data` | Host → Client | Sensor snapshot (CPU, GPU, RAM, frame) |
| `status` | Client → Host | Server status query |
| `widget.parse` | Client → Host | Parse .omni source, return OmniFile JSON + errors |
| `widget.update` | Client → Host | Apply parsed OmniFile JSON as active overlay |
| `widget.apply` | Client → Host | Parse raw .omni source + apply live (no disk write) |
| `file.list` | Client → Host | List overlay folders and theme files |
| `file.read` | Client → Host | Read file content by relative path |
| `file.write` | Client → Host | Write content to file |
| `file.create` | Client → Host | Create new overlay folder or theme file |
| `file.delete` | Client → Host | Delete overlay or theme (protects Default) |

---

## Graphics Hooking

### Hook Strategy

- WARP adapter for vtable discovery (avoids GPU init race conditions)
- Hooks `Present` (vtable 8), `Present1` (vtable 22), `ResizeBuffers` (vtable 13)
- Hooks `CreateSwapChainForHwnd` (vtable 15) for DX12 command queue capture
- Deferred `ExecuteCommandLists` hook (vtable 10) for re-injection queue capture
- Hooking library: `minhook`

### DX11 Rendering Path

1. `swap_chain.GetBuffer::<IDXGISurface>(0)` → DXGI surface
2. `CreateDxgiSurfaceRenderTarget` → D2D render target (cached)
3. D2D `BeginDraw` → draw widgets → `EndDraw`
4. Render target released on `ResizeBuffers`

### DX12 Rendering Path

1. Detect DX12 via `swap_chain.GetDevice::<ID3D12Device>()`
2. `D3D11On12CreateDevice` with captured command queue
3. Each frame: wrap current back buffer (`GetCurrentBackBufferIndex`), acquire, D2D render, release, flush
4. Handles swap chain transitions (splash screen → main game)

### Resilience

- Swap chain change detection (resets all D3D11On12 state)
- DX12 fail counter (suspends after 10 failures, retries on swap chain change)
- `HOOKS_INSTALLED` AtomicBool prevents double-hooking
- Clean shutdown: disable hooks → drain in-flight calls → release D3D resources → `FreeLibraryAndExitThread`

---

## Crate Dependencies

### Host Process

| Crate | Purpose |
|-------|---------|
| `windows` 0.58 | Win32 APIs, shared memory, process management |
| `sysinfo` 0.35 | CPU usage, frequencies, RAM |
| `wmi` 0.14 | CPU temperature via WMI |
| `quick-xml` 0.37 | Parse `.omni` file XML structure |
| `lightningcss` 1.0.0-alpha.71 | CSS parsing (available for future advanced features) |
| `tungstenite` 0.26 | WebSocket server |
| `serde` + `serde_json` | Config persistence, WebSocket JSON messages |
| `ctrlc` | Ctrl+C signal handling |
| `tracing` + `tracing-subscriber` | Structured logging |

### Overlay DLL

| Crate | Purpose |
|-------|---------|
| `windows` 0.58 | D3D11, D3D12, D3D11On12, DXGI, D2D1, DirectWrite |
| `minhook` 0.9 | Function detouring / trampoline hooks |
| `omni-shared` | Shared types for IPC |

### Shared

| Crate | Purpose |
|-------|---------|
| (none) | Pure `#[repr(C)]` types, no external dependencies |

---

## Build Phases

### Completed Phases

#### Phase 1: Workspace + Shared Types + DLL Injection PoC ✅
#### Phase 2: DX11 Present Hook ✅
#### Phase 3: Render on DX11 Back Buffer ✅
#### Phase 4: Process Lifecycle + Graceful Shutdown ✅
#### Phase 5: Shared Memory IPC + First Sensor ✅
#### Phase 6: DX12 Hook + D3D11On12 Renderer ✅
#### Phase 7: Full Sensor Suite (NVML, WMI, sysinfo) ✅
#### Phase 8: Frame Timing (Present Hook + QPC) ✅
#### Phase 9a-0: Host Service Infrastructure (WebSocket, --service mode) ✅
#### Phase 9a-1: Core Widget Format + Parser (.omni files, OmniResolver) ✅
#### Phase 9a-2a: Workspace + File Management + Config ✅

### Upcoming Phases

#### Phase 9a-2b: CSS Cascade + Per-Sensor Polling

- Descendant selectors (`.panel .label`)
- Compound selectors (`.label.critical`)
- Specificity calculation (ID > class > element)
- `<config>` block in `.omni` for per-sensor poll intervals
- Sensor poller refactor for variable-rate polling

#### Phase 9a-3: Advanced Visuals + Flexbox

- Gradients (`linear-gradient`), `box-shadow`, per-corner `border-radius`
- `taffy` flexbox layout engine
- D2D renderer updates for gradients and shadows

#### Phase 9a-4: Structured Error Reporting

- Parser errors with line/column, severity, suggestions
- JSON format for Monaco integration
- CSS property validation

#### Phase 9b: Animations + Adaptive Color

- CSS transitions, keyframe animations, easing functions
- Adaptive text color (luminance sampling)
- `transform: translate3d`, `scale`

#### Phase 10: Hot-Reload + Preview

- File watching with debounce
- Live re-parse → re-layout → push to shared memory
- Host-side preview window

#### Phase 11: Electron App + Installer

- Visual widget editor with drag/drop
- Monaco editor with custom IntelliSense for `.omni` format
- Live HTML/CSS preview (browser-native WYSIWYG)
- Parser errors piped to Monaco squiggles
- Electron Builder + NSIS Windows installer
- Tray icon / background service behavior
- Built-in themes (dark, cyberpunk, retro)

#### Phase 12: Polish

- Error recovery, anti-cheat detection
- Borderless window fallback for protected games
- Graph/bar widget types
- Performance profiling

---

## Distribution Architecture

```
C:\Program Files\Omni\
  Omni.exe              ← Electron app (main entry)
  omni-host.exe         ← Rust host (background service)
  overlay/
    omni_overlay.dll    ← Injected into games
  themes/
    dark.omni           ← Built-in themes
  resources/
    app.asar            ← Electron bundle
```

User data at `%APPDATA%\Omni\` (config, overlays, themes).

---

## Anti-Cheat Considerations

Games with kernel anti-cheat (EAC, BattlEye, Vanguard) will block injection.

- **Fallback mode**: Detect protected games, use transparent `WS_EX_TOPMOST` borderless window overlay
- **Known protected games list**: Warn users before attempting injection
