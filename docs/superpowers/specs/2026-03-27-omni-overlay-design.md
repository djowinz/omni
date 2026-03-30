# Omni — Game Overlay Hardware Monitor Design Spec

## Overview

A Rust-based hardware monitoring overlay that renders on top of any game, including exclusive fullscreen titles. Users define widget layout and styling using a Vue-style single-file component format (`.omni` files). The system uses DLL injection and graphics API hooking to render directly into the game's swap chain.

---

## Architecture

Two binaries communicating over shared memory IPC:

```
┌─────────────────────┐         IPC            ┌──────────────────────┐
│   Host Process      │ ◄──────────────────►   │   Game Process       │
│   (host.exe)        │   shared mem + pipe     │                      │
│                     │                         │  ┌────────────────┐  │
│  - Sensor polling   │   sensor snapshots ──►  │  │ Injected DLL   │  │
│  - .omni parsing  │   computed layout  ──►  │  │ (overlay.dll)  │  │
│  - CSS resolution   │                         │  │                │  │
│  - taffy layout     │   ◄── control msgs      │  │ - Hook Present │  │
│  - Animation interp │   ◄── luminance data    │  │ - Sample lum.  │  │
│  - Process picker   │                         │  │ - Render overlay│ │
│  - Preview window   │                         │  └────────────────┘  │
└─────────────────────┘                         └──────────────────────┘
```

### Host Process (`host/`)

The main application the user launches. Responsibilities:

- **Sensor polling**: Reads CPU, GPU, RAM, and frame timing data on a background thread (500ms–1s interval).
- **Widget parsing**: Loads `.omni` single-file components (template + style blocks) via `quick-xml` and `cssparser`.
- **Style resolution**: Merges theme file → local styles → cascade. Resolves CSS variables.
- **Layout computation**: Uses `taffy` (flexbox engine) to compute widget positions from CSS within each panel.
- **Animation interpolation**: Evaluates transitions and keyframe animations per-frame, writes resolved values.
- **IPC writer**: Serializes sensor snapshots and computed layout into shared memory (double-buffered, lock-free).
- **DLL injection**: Uses `CreateRemoteThread` + `LoadLibraryW` to inject the overlay DLL into the target game.
- **Preview window**: Standalone window that renders the overlay layout without a game running.
- **Sensor binding ledger**: Structured logging of all sensor source initialization and state changes.

### Overlay DLL (`overlay-dll/`)

A `cdylib` injected into the game process. Must be minimal and rock-solid. Responsibilities:

- **Graphics API hooking**: Hooks `IDXGISwapChain::Present` to intercept each frame (DX11 and DX12 via D3D11On12).
- **Luminance sampling**: Samples back buffer regions behind widgets for adaptive color (small rect averages).
- **Overlay rendering**: Draws widgets onto the game's back buffer using D2D/DirectWrite.
- **IPC reader**: Reads sensor data and computed layout from shared memory every frame.

### Shared Library (`shared/`)

`#[repr(C)]` types shared between host and DLL:

- `SensorSnapshot`, `CpuData`, `GpuData`, `RamData`, `FrameData`
- `ComputedWidget`, `WidgetType`, `SensorSource`
- `SharedOverlayState` (double-buffered shared memory layout)
- IPC protocol constants

---

## Project Structure

```
repo/
  Cargo.toml                   # Workspace root

  host/
    Cargo.toml
    src/
      main.rs                  # UI, process picker, orchestrator
      sensors/
        mod.rs                 # SensorSource trait, polling loop, binding ledger
        cpu.rs                 # sysinfo + WMI fallback
        gpu.rs                 # NVAPI FFI bindings
        ram.rs                 # sysinfo + WMI for temps
        frames.rs              # ETW PresentMon consumer
      widget/
        mod.rs                 # .omni file parser (quick-xml + cssparser)
        template.rs            # Template block parsing → widget tree
        style.rs               # Style block + theme file parsing
        cascade.rs             # Selector matching, cascade resolution
        variables.rs           # CSS variable resolution
        layout.rs              # taffy-based layout computation
        animation.rs           # Transition + keyframe interpolation
        validate.rs            # Error reporting with locations and suggestions
      ipc/
        mod.rs                 # Shared memory writer (double-buffered), named pipe
      injector/
        mod.rs                 # CreateRemoteThread + LoadLibraryW logic
      preview/
        mod.rs                 # Standalone preview window renderer

  overlay-dll/
    Cargo.toml                 # [lib] crate-type = ["cdylib"]
    src/
      lib.rs                   # DllMain entry point, init/teardown
      hooks/
        mod.rs                 # Hook manager, API detection
        dx11.rs                # IDXGISwapChain::Present hook (vtable index 8)
        dx12.rs                # DX12 Present hook via D3D11On12
        resize.rs              # ResizeBuffers hook (vtable index 13)
      renderer/
        mod.rs                 # Renderer trait, resource caching
        d2d_renderer.rs        # D2D/DirectWrite rendering (shared by DX11 + DX12)
        adaptive.rs            # Back buffer luminance sampling
      ipc/
        mod.rs                 # Shared memory reader (double-buffered)
      widgets/
        mod.rs                 # Widget drawing from computed layout

  shared/
    Cargo.toml
    src/
      lib.rs
      sensor_types.rs          # SensorSnapshot and sub-structs
      widget_types.rs          # ComputedWidget, WidgetType, SensorSource
      ipc_protocol.rs          # SharedOverlayState, double-buffer, constants
```

### Workspace Cargo.toml

```toml
[workspace]
members = ["host", "overlay-dll", "shared"]
resolver = "2"
```

---

## IPC: Shared Memory Protocol

### Lock-Free Double Buffer

```rust
// shared/src/ipc_protocol.rs

use std::sync::atomic::AtomicU64;

pub const SHARED_MEM_NAME: &str = "OmniOverlay_SharedState";
pub const MAX_WIDGETS: usize = 64;

#[repr(C)]
pub struct SharedOverlayState {
    pub active_slot: AtomicU64,            // 0 or 1 — which slot the DLL should read
    pub slots: [OverlaySlot; 2],
}

#[repr(C)]
pub struct OverlaySlot {
    pub write_sequence: u64,               // incremented on each write
    pub sensor_data: SensorSnapshot,
    pub layout_version: u64,               // bumped on widget/style changes
    pub widget_count: u32,
    pub widgets: [ComputedWidget; MAX_WIDGETS],
}
```

- Host writes to the inactive slot, then atomically flips `active_slot`
- DLL reads from the active slot each frame
- No mutexes, no blocking, no torn reads

### Control Channel

Named pipe `\\.\pipe\OmniOverlay_Control` for low-frequency messages:

- Host → DLL: shutdown, config reload signal
- DLL → Host: hook status, errors, luminance data for adaptive color

---

## Sensor Data Types

```rust
// shared/src/sensor_types.rs

#[repr(C)]
pub struct SensorSnapshot {
    pub timestamp_ms: u64,
    pub cpu: CpuData,
    pub gpu: GpuData,
    pub ram: RamData,
    pub frame: FrameData,
}

#[repr(C)]
pub struct CpuData {
    pub total_usage_percent: f32,
    pub per_core_usage: [f32; 32],         // -1.0 for unused cores
    pub core_count: u32,
    pub per_core_freq_mhz: [u32; 32],
    pub package_temp_c: f32,               // NaN if unavailable
}

#[repr(C)]
pub struct GpuData {
    pub usage_percent: f32,
    pub temp_c: f32,
    pub core_clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
    pub fan_speed_rpm: u32,
    pub power_draw_w: f32,
}

#[repr(C)]
pub struct RamData {
    pub usage_percent: f32,
    pub used_mb: u64,
    pub total_mb: u64,
    pub frequency_mhz: u32,
    pub timing_cl: u32,
    pub temp_c: f32,                       // NaN if unavailable
}

#[repr(C)]
pub struct FrameData {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub frame_time_avg_ms: f32,
    pub frame_time_1percent_ms: f32,
    pub frame_time_01percent_ms: f32,
    pub available: bool,
}
```

All structs `#[repr(C)]` for shared memory safety.

---

## Computed Widget Types

```rust
// shared/src/widget_types.rs

#[repr(C)]
pub struct ComputedWidget {
    pub widget_type: WidgetType,
    pub source: SensorSource,
    pub x: f32,                            // absolute screen position
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub font_weight: u16,
    pub color_rgba: [u8; 4],
    pub bg_color_rgba: [u8; 4],
    pub bg_gradient: GradientDef,          // linear gradient support
    pub border_color_rgba: [u8; 4],
    pub border_width: f32,
    pub border_radius: [f32; 4],           // per-corner
    pub opacity: f32,
    pub box_shadow: ShadowDef,
    pub format_pattern: [u8; 128],         // null-terminated UTF-8
    pub label_text: [u8; 64],              // null-terminated UTF-8
    pub critical_above: f32,
    pub critical_color_rgba: [u8; 4],
    pub history_seconds: u32,              // for graph widgets
    pub history_interval_ms: u32,          // data point granularity
    pub adaptive_color: AdaptiveColorMode,
    pub adaptive_light_rgba: [u8; 4],
    pub adaptive_dark_rgba: [u8; 4],
}

#[repr(C)]
pub enum WidgetType {
    Label,
    SensorValue,
    Graph,
    Bar,
    Spacer,
    Group,
}

#[repr(C)]
pub enum SensorSource {
    None,                                  // for Label, Spacer, Group
    CpuUsage,
    CpuTemp,
    CpuFreqCore0,
    CpuFreqCore1,
    CpuFreqCore2,
    CpuFreqCore3,
    // ... up to core 31 (flat variants for repr(C) safety)
    GpuUsage,
    GpuTemp,
    GpuClock,
    GpuMemClock,
    GpuVram,
    GpuPower,
    GpuFan,
    RamUsage,
    RamTemp,
    RamFreq,
    Fps,
    FrameTime,
    FrameTimeAvg,
    FrameTime1Pct,
    FrameTime01Pct,
}

#[repr(C)]
pub enum AdaptiveColorMode {
    Off,
    Auto,                                  // black/white based on luminance
    Custom,                                // uses adaptive_light/dark_rgba
}

#[repr(C)]
pub struct GradientDef {
    pub enabled: bool,
    pub angle_deg: f32,
    pub start_rgba: [u8; 4],
    pub end_rgba: [u8; 4],
}

#[repr(C)]
pub struct ShadowDef {
    pub enabled: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub color_rgba: [u8; 4],
}
```

---

## Widget File Format (`.omni`)

### Structure

```xml
<theme src="./themes/dark.css" />

<template>
  <!-- Widget tree with panels, groups, sensors, graphs, bars, labels -->
</template>

<style>
  /* Local CSS styles, merged on top of theme */
</style>
```

All three blocks are optional. No `<theme>` uses built-in defaults. No `<style>` uses theme-only. Just `<template>` works with all defaults.

### Template Elements

| Element | Purpose | Key Attributes |
|---------|---------|---------------|
| `<panel>` | Top-level positioned container | `class`, `anchor` |
| `<group>` | Flex container for grouping | `class`, `direction` |
| `<sensor>` | Displays a sensor value | `source`, `label`, `format`, `critical-above`, `critical-below`, `class` |
| `<graph>` | Time-series line graph | `source`, `history`, `interval`, `class` |
| `<bar>` | Progress-style bar | `source`, `min`, `max`, `class` |
| `<label>` | Static text | `text`, `class` |
| `<spacer>` | Flex spacer | `class` |

### Source Strings

Dot-notation paths into sensor data:

```
cpu.usage       cpu.temp        cpu.freq.{n}
gpu.usage       gpu.temp        gpu.clock       gpu.mem-clock
gpu.vram        gpu.power       gpu.fan
ram.usage       ram.freq        ram.temp
fps             frame-time      frame-time.avg
frame-time.1pct frame-time.01pct
```

### Format Strings

- `{value:.0f}°C` — formatted sensor value
- `{label}: {value:.1f}%` — include the label
- `{value}/{max}MB` — for sources with a max (e.g., VRAM)
- Auto-format if omitted (temps → `°C`, percentages → `%`, clocks → `MHz`)

### Panel Positioning

Every `<panel>` is `position: fixed`. Two positioning modes:

**Anchor shorthand** (convenience):
```css
.cpu-panel { anchor: top-right; margin: 10px; }
```

**Pixel-precise** (full control):
```css
.gpu-panel { top: 47px; left: 312px; }
```

**Percentage-based**:
```css
.fps-panel { bottom: 5%; left: 50%; }
```

`anchor` is syntactic sugar for `top`/`bottom`/`left`/`right`. Explicit offsets take precedence. Percentages resolve against game render resolution.

### Supported CSS Properties

| Category | Properties |
|----------|-----------|
| **Position** | `top`, `right`, `bottom`, `left`, `anchor` |
| **Box model** | `width`, `height`, `min-width`, `min-height`, `max-width`, `max-height`, `padding`, `margin` |
| **Flexbox** | `display`, `flex-direction`, `justify-content`, `align-items`, `align-self`, `gap`, `flex-grow`, `flex-shrink` |
| **Background** | `background` (solid color, rgba, `linear-gradient`) |
| **Border** | `border`, `border-width`, `border-color`, `border-radius` (per-side, per-corner) |
| **Visual** | `opacity`, `box-shadow` |
| **Typography** | `font-family`, `font-size`, `font-weight`, `color`, `text-align` |
| **Animation** | `transition`, `animation` |
| **Adaptive** | `color: adaptive`, `color: adaptive(light, dark)`, `background: adaptive-tint` |

### CSS Selectors

- Class selectors: `.sensor-value`
- Type selectors: `sensor`, `graph`, `bar`, `panel`
- Compound selectors: `.sensor-value.critical`
- Descendant selectors: `.gpu-section .sensor-value`
- CSS variables: `--custom-property` on `:root` or any element, referenced via `var(--prop)`

### Style Cascade Order (lowest to highest)

1. Built-in defaults (hardcoded sensible fallbacks)
2. Theme file (`<theme src="...">`)
3. Local `<style>` block
4. (Future: inline attributes)

### Error Reporting

```
error: unknown element <grph> in template
  --> overlay.omni:8:5
   |
8  |     <grph source="cpu.usage" />
   |     ^^^^^ did you mean <graph>?

warning: unsupported CSS property "text-shadow"
  --> overlay.omni:18:3
   |
18 |   text-shadow: 1px 1px black;
   |   ^^^^^^^^^^^ this property is not supported
   |   supported visual properties: color, background, border, ...
```

Rust-compiler-style diagnostics with file locations, context, and suggestions.

---

## Animations

### CSS Transitions

Smooth interpolation between property states:

```css
sensor {
    color: #cccccc;
    transition: color 0.3s ease;
}
sensor.critical {
    color: #ff4444;
}
```

When a sensor crosses its critical threshold, the color transitions smoothly over 300ms.

### Keyframe Animations

```css
@keyframes pulse {
    0%   { opacity: 1.0; }
    50%  { opacity: 0.6; }
    100% { opacity: 1.0; }
}

sensor.critical {
    animation: pulse 1s infinite;
}
```

### Implementation

- **Host-side only**: The host evaluates all animation state (current time, keyframe progress, transition triggers).
- Each frame, the host interpolates animated properties and writes resolved values to shared memory.
- The DLL renders the values as-is — no animation logic in the DLL.
- Easing functions: `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `cubic-bezier(a,b,c,d)`.

### Animatable Properties (v1)

`opacity`, `color`, `background`, `border-color`, `transform` (translate only).

---

## Adaptive Color System

### Text Color

```css
sensor { color: adaptive; }                     /* auto black/white */
sensor { color: adaptive(#44ff88, #003311); }   /* custom light/dark pair */
```

### Background Tint

```css
.panel { background: adaptive-tint; }
```

Dynamically adjusts panel background opacity based on scene brightness to maintain readability.

### Implementation

1. In the Present hook, before rendering the overlay, sample back buffer pixels in widget regions.
2. Compute average luminance per region: `L = 0.299*R + 0.587*G + 0.114*B`.
3. If `L > threshold` → use dark color variant. If below → use light variant.
4. Luminance data sent to host via control pipe for animation interpolation.
5. Resample every 5 frames to reduce overhead (widget regions are small — ~4000 pixels for a label).

---

## Sensor Data Sources

| Data | Primary Source | Fallback |
|------|---------------|----------|
| CPU usage, per-core, frequency | `sysinfo` crate | — |
| CPU temperature | LibreHardwareMonitor via WMI | — |
| RAM usage/total | `sysinfo` crate | — |
| RAM frequency, timings, temp | WMI (LHM) | — |
| GPU usage, temp, clocks, VRAM, power, fan | NVAPI FFI bindings | WMI (LHM) |
| FPS, frame time, percentiles | ETW (PresentMon approach) | — |

### NVAPI Bindings

Thin Rust FFI bindings to `nvapi64.dll`:
- `NvAPI_Initialize`
- `NvAPI_GPU_GetUsages`
- `NvAPI_GPU_GetThermalSettings`
- `NvAPI_GPU_GetAllClockFrequencies`
- `NvAPI_GPU_GetMemoryInfo`

### ETW Frame Timing

Host runs an ETW consumer session subscribing to `Microsoft-Windows-DXGI` and `Microsoft-Windows-D3D9` providers, filtering by target game PID. Computes FPS, frame time, rolling average, 1%/0.1% lows from a ring buffer.

### Graceful Degradation

Unavailable sensors display **"N/A"** in the widget (not omitted). Widget stays in layout position. The sensor binding ledger logs initialization status and state changes via the `tracing` crate:

```
[INFO]  sysinfo: initialized (CPU 12 cores, RAM 32GB)
[INFO]  nvapi: initialized (NVIDIA RTX 4080)
[WARN]  wmi/lhm: LibreHardwareMonitor not detected — CPU temp unavailable
[INFO]  etw: PresentMon session started, watching PID 14320
```

---

## Graphics Hooking

### Hook Strategy

- On `DllMain` attach, check loaded modules (`GetModuleHandleW` for `d3d11.dll`, `d3d12.dll`)
- Install hooks only for detected APIs
- Both DX11 and DX12 share `IDXGISwapChain::Present` (vtable index 8)
- Hooking library: `minhook` (battle-tested in game overlay community)

### DX11 Rendering Path

1. `swap_chain.GetBuffer::<ID3D11Texture2D>(0)` → back buffer
2. Create `ID2D1RenderTarget` sharing the DXGI surface (cached after first frame)
3. Draw with Direct2D (rectangles, gradients, rounded rects) + DirectWrite (text)
4. All D2D/DWrite resources cached, recreated only on layout version change

### DX12 Rendering Path

- D3D11On12 compatibility layer to reuse all D2D/DirectWrite rendering code
- Minimal overhead for an overlay drawing a few widgets

### Resource Safety

- Hook `ResizeBuffers` (vtable index 13) — release and recreate render target on resize/alt-tab
- Device lost/removed — release all resources, reinitialize on next Present
- Clean unload — unhook → release D2D/D3D resources → unmap shared memory (in that order)

---

## Crate Dependencies

### Host Process

| Crate | Purpose |
|-------|---------|
| `windows` | Win32 APIs, process management, shared memory, ETW |
| `sysinfo` | CPU usage, frequencies, RAM |
| `wmi` | LibreHardwareMonitor queries |
| `quick-xml` | Parse `.omni` template blocks |
| `cssparser` | Parse CSS style blocks and theme files |
| `taffy` | Flexbox layout engine |
| `tracing` + `tracing-subscriber` | Structured logging, sensor binding ledger |
| `notify` | File watching for hot-reload |
| `crossbeam` | Channels for sensor polling threads |

### Overlay DLL

| Crate | Purpose |
|-------|---------|
| `windows` | D3D11, D3D12, DXGI, D2D1, DirectWrite |
| `minhook` | Function detouring / trampoline hooks |

Minimize DLL dependencies — every crate increases crash risk in the game process.

### Shared

| Crate | Purpose |
|-------|---------|
| (none) | Pure `#[repr(C)]` types, no external dependencies |

---

## Build Phases

### Phase 1: Workspace + Shared Types + DLL Injection PoC

- Cargo workspace with `host`, `overlay-dll`, `shared` crates
- `#[repr(C)]` shared types
- Minimal `cdylib` with `DllMain` that logs on attach
- Host-side injector (`CreateRemoteThread` + `LoadLibraryW`)
- **Success**: DLL loads in a game process, log file confirms

### Phase 2: DX11 Present Hook

- Dummy swap chain vtable trick to locate `Present`
- Trampoline hook via `minhook`
- Detour logs every 60 calls
- **Success**: Log shows hook firing at game's frame rate

### Phase 3: Render Text on DX11 Back Buffer

- Get back buffer, create D2D render target (cached)
- Draw hardcoded text with DirectWrite
- Hook `ResizeBuffers` for resize safety
- **Success**: Text visible on top of the game, no crashes

### Phase 4: Process Lifecycle + Graceful Shutdown

- Process scanner with configurable poll interval
- Two-tier injection strategy:
  - **New processes** (appear after host starts): visible window + graphics DLL + not excluded
  - **Pre-existing processes** (already running): additionally require a known game installation directory match, explicit include list entry, or overlay DLL already loaded
- Default game directory detection (Steam library folders via `libraryfolders.vdf`, Epic, GOG, Battle.net, Riot, EA, Ubisoft, Xbox)
- Configurable exclude list (browsers, system processes, GPU tools, launchers, etc.)
- Configurable include list (user allowlist for games in non-standard directories)
- `Ctrl+C` handler (`ctrlc` crate + `AtomicBool`) for clean poll loop exit
- Graceful DLL ejection via exported `omni_shutdown`:
  - Disables minhook trampolines (restores original vtable)
  - Drains in-flight hook calls (200ms sleep)
  - Releases D3D resources (renderer drop)
  - Atomically unloads DLL via `FreeLibraryAndExitThread`
- `--stop` command: ejects overlay DLL from all processes, then terminates running host instances
- Host restart resilience:
  - After `Ctrl+C` (clean eject): re-injects and re-hooks without crashing the game
  - After crash/Task Manager kill (DLL still loaded): detects existing DLL via `has_module` and reconnects without double-injection
- `inject_dll` hard gate: refuses injection if DLL already loaded in target
- DLL-side `HOOKS_INSTALLED` guard: prevents double-hooking regardless of host behavior
- Config persistence at `%APPDATA%\Omni\config.json` (exclude, include, game_directories, poll_interval_ms)
- **Success**: Host can start, stop, crash, and restart without crashing games; overlay persists or reconnects correctly

### Phase 5: Shared Memory IPC + First Sensor

- Named shared memory with double-buffer
- Named pipe control channel
- Host polls CPU usage via `sysinfo`, writes to shared memory
- DLL reads and renders live CPU usage
- `tracing` setup with sensor binding ledger
- **Success**: Real CPU usage displayed in the overlay

### Phase 6: DX12 Hook + D3D11On12 Renderer

- Hook `Present` for DX12 swap chains
- D3D11On12 compatibility layer, reuse D2D rendering
- **Success**: Overlay works on DX11 and DX12 games

### Phase 7: Full Sensor Suite

- `sysinfo` — CPU per-core, frequencies, RAM
- WMI/LHM — temperatures, fan speeds, voltages
- NVAPI FFI — GPU usage, temp, clocks, VRAM, power, fan
- N/A display for unavailable sensors
- **Success**: Full hardware dashboard in the overlay

### Phase 8: ETW Frame Timing

- ETW consumer session for DXGI present events
- Filter by target game PID
- FPS, frame time, rolling average, 1%/0.1% lows
- **Success**: Accurate frame timing data in the overlay

### Phase 9a-0: Host Service Infrastructure

- WebSocket server on host (localhost:9473) for Electron communication
- `--service` mode: auto-discover DLL from install directory, no CLI args needed
- Basic JSON message routing (parse, dispatch to handlers)
- Sensor data streaming over WebSocket
- **Success**: External client can connect via WebSocket and receive sensor data

### Phase 9a-1: Core Widget Format + Parser

- `.omni` file parser (`quick-xml` + `cssparser`) → `WidgetTree` data structure
- `WidgetTree` is JSON-serializable (for Electron communication)
- Template elements: `<panel>`, `<sensor>`, `<graph>`, `<bar>`, `<label>`, `<group>`, `<spacer>`
- Basic styling: position, color, font, opacity, background, border-radius
- Panel positioning (anchor + pixel-precise + percentage)
- Sensible defaults and auto-formatting for sensor values
- `WidgetTree` → `Vec<ComputedWidget>` replaces hardcoded `WidgetBuilder`
- WebSocket `widget.update` and `widget.parse` endpoints
- **Success**: Changing `.omni` file changes the overlay appearance

### Phase 9a-2: CSS Cascade + Themes + File Management

- Theme file loading (`<theme src="...">`) with full cascade
- CSS variables (`:root { --var: value }` + `var(--var)`) across theme → widget
- Selector matching: class, type, compound, descendant selectors
- Cascade resolution (built-in → theme → local style)
- Specificity calculation
- WebSocket file management API for Electron workspace:
  - `file.list` — list `.omni` and `.css` files in `%APPDATA%\Omni/`
  - `file.read` — read raw file content
  - `file.write` — write raw content to file
  - `file.create` — create new `.omni` or theme `.css` file
  - `file.delete` — delete a file
  - `widget.apply` — parse raw `.omni` source + apply to overlay (no disk write)
- **Success**: Theme files change appearance, CSS cascade works correctly, Electron can manage workspace files

### Phase 9a-3: Advanced Visuals + Flexbox

- Full visual properties: gradients (`linear-gradient`), `box-shadow`, per-corner `border-radius`
- `taffy` flexbox layout within panels (`flex-direction`, `justify-content`, `align-items`, `gap`)
- D2D renderer updates for gradients and shadows
- **Success**: Complex layouts with flexbox and advanced visual effects

### Phase 9a-4: Structured Error Reporting

- Parser returns structured error objects with line, column, severity, message, suggestion
- Rust-compiler-style CLI output for developer mode
- JSON error format for Electron/Monaco integration
- Element name suggestions ("did you mean <graph>?")
- CSS property validation with supported property hints
- WebSocket `widget.parse` returns structured errors
- **Success**: Parse errors include file locations and actionable suggestions

### Phase 9b: Animations + Adaptive Color

- CSS transitions (property interpolation on state change)
- Keyframe animations (pulse, fade, slide-in)
- Easing functions (ease, ease-in-out, cubic-bezier)
- Adaptive text color (luminance sampling in DLL)
- Adaptive background tint
- **Success**: Smooth transitions on threshold changes, readable text on any scene

### Phase 10: Hot-Reload + Preview

- `notify` crate file watching with 200ms debounce
- Live re-parse → re-layout → push to shared memory
- Host-side preview window (renders without a game running)
- Preview aware of animations and adaptive color
- **Success**: Edit, save, see changes instantly

### Phase 11: Electron App + Installer

- Electron app as main UI: visual widget editor, Monaco editor, live preview, settings
- Monaco integration with custom IntelliSense for `.omni` format
- Live parser errors piped from host → Monaco squiggles
- HTML/CSS preview (browser-native, matches in-game rendering)
- Electron Builder + NSIS Windows installer
- Tray icon / background service behavior
- 2-3 built-in themes (dark minimal, cyberpunk, retro)
- **Success**: Non-technical users can install, customize, and use the overlay

### Phase 12: Polish

- Error recovery (host detects game exit, cleans up)
- Anti-cheat detection with borderless window fallback
- Graph widget ring buffer data in shared memory
- Performance profiling and optimization

---

## Anti-Cheat Considerations

Games with kernel anti-cheat (EAC, BattlEye, Vanguard) will block injection. Mitigation:

- **Fallback mode**: Detect protected games, use a transparent `WS_EX_TOPMOST` borderless window overlay (works for borderless fullscreen, not exclusive fullscreen on protected games)
- **Known protected games list**: Maintain a list and warn users before attempting injection
