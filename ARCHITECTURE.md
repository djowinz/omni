# Game Overlay Hardware Monitor — Architecture Document

## Overview

A Rust-based hardware monitoring overlay that renders on top of any game, including exclusive fullscreen titles. Users can customize widget layout and styling using CSS-like configuration. The system uses DLL injection and graphics API hooking to render directly into the game's swap chain.

---

## High-Level Architecture

The system is split into two binaries that communicate over IPC:

```
┌─────────────────────┐         IPC            ┌──────────────────────┐
│   Host Process      │ ◄──────────────────►   │   Game Process       │
│   (host.exe)        │   (shared mem / pipe)   │                      │
│                     │                         │  ┌────────────────┐  │
│  - Sensor polling   │   sensor snapshots ──►  │  │ Injected DLL   │  │
│  - Config/CSS parse │   layout/style   ────►  │  │ (overlay.dll)  │  │
│  - Widget layout    │                         │  │                │  │
│  - Process picker   │                         │  │ - Hook Present │  │
│  - Widget editor UI │                         │  │ - Render overlay│  │
└─────────────────────┘                         │  └────────────────┘  │
                                                └──────────────────────┘
```

### Binary 1: Host Process (`host/`)

The main application the user launches. Responsibilities:

- **Sensor polling**: Reads CPU, GPU, RAM, and frame timing data on a background thread (500ms–1s interval).
- **Configuration**: Loads widget definitions (TOML) and user styles (CSS subset).
- **Layout computation**: Uses `taffy` (flexbox engine) to compute widget positions from CSS.
- **IPC writer**: Serializes sensor snapshots and computed layout into shared memory.
- **DLL injection**: Uses `CreateRemoteThread` + `LoadLibraryW` to inject the overlay DLL into the target game process.
- **UI**: Process picker, widget editor, style previewer (optional, can be a separate window or tray app).

### Binary 2: Overlay DLL (`overlay-dll/`)

A `cdylib` that gets injected into the game process. Must be minimal and rock-solid — a crash here crashes the game. Responsibilities:

- **Graphics API hooking**: Hooks `Present` / `vkQueuePresentKHR` / `wglSwapBuffers` to intercept each frame.
- **Overlay rendering**: Draws widgets onto the game's back buffer using D2D/DirectWrite (DX11), or equivalent per API.
- **IPC reader**: Reads sensor data and computed layout from shared memory every frame.

### Shared Library (`shared/`)

Types shared between host and DLL:

- `SensorSnapshot`, `CpuData`, `GpuData`, `RamData`, `FrameData`
- `WidgetDef`, computed style structs
- `SharedOverlayState` (the shared memory layout)
- IPC protocol constants (shared memory names, pipe names)

---

## Project Structure

```
repo/
  Cargo.toml                   # Workspace root

  host/                        # Main application (exe)
    Cargo.toml
    src/
      main.rs                  # UI, process picker, orchestrator
      sensors/
        mod.rs                 # SensorSnapshot, polling loop
        cpu.rs                 # sysinfo + WMI fallback
        gpu.rs                 # NVAPI / ADL FFI bindings
        ram.rs                 # sysinfo for usage, WMI for temps
        frames.rs              # PresentMon ETW or RTSS shared memory
      config/
        mod.rs                 # TOML config loading
        widget_defs.rs         # Widget definition structs
      style/
        mod.rs                 # CSS-subset parser (cssparser crate)
        layout.rs              # taffy-based layout computation
        theme.rs               # Resolved/computed styles per widget
      ipc/
        mod.rs                 # Shared memory writer, named pipe server
      injector/
        mod.rs                 # CreateRemoteThread + LoadLibraryW logic

  overlay-dll/                 # Injected library (cdylib)
    Cargo.toml
    src/
      lib.rs                   # DllMain entry point, init/teardown
      hooks/
        mod.rs                 # Hook manager, API detection
        dx11.rs                # IDXGISwapChain::Present hook (vtable index 8)
        dx12.rs                # DX12 Present hook
        vulkan.rs              # vkQueuePresentKHR hook
        opengl.rs              # wglSwapBuffers hook
      renderer/
        mod.rs                 # Renderer trait
        dx11_renderer.rs       # D2D/DirectWrite on DX11 back buffer
        dx12_renderer.rs       # DX12 rendering pipeline
        vk_renderer.rs         # Vulkan rendering pipeline
        gl_renderer.rs         # OpenGL rendering
      ipc/
        mod.rs                 # Shared memory reader
      widgets/
        mod.rs                 # Widget drawing from computed layout

  shared/                      # Shared types (lib crate)
    Cargo.toml
    src/
      lib.rs
      sensor_types.rs          # SensorSnapshot and sub-structs
      widget_types.rs          # WidgetDef, StyleProps, computed layout
      ipc_protocol.rs          # SharedOverlayState, constants
```

### Workspace Cargo.toml

```toml
[workspace]
members = ["host", "overlay-dll", "shared"]
resolver = "2"
```

### overlay-dll/Cargo.toml (must produce a .dll)

```toml
[lib]
crate-type = ["cdylib"]
```

---

## Core Data Types

### Sensor Data

```rust
// shared/src/sensor_types.rs

use std::time::Instant;

#[repr(C)]
pub struct SensorSnapshot {
    pub timestamp_ms: u64,       // millis since host start
    pub cpu: CpuData,
    pub gpu: GpuData,
    pub ram: RamData,
    pub frame: FrameData,
}

#[repr(C)]
pub struct CpuData {
    pub total_usage_percent: f32,
    pub per_core_usage: [f32; 32],   // fixed-size for shared mem (pad unused with -1.0)
    pub core_count: u32,
    pub per_core_freq_mhz: [u32; 32],
    pub package_temp_c: f32,         // NaN if unavailable
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
    pub timing_cl: u32,              // CAS latency
    pub temp_c: f32,                 // NaN if unavailable (needs SPD sensor)
}

#[repr(C)]
pub struct FrameData {
    pub fps: f32,
    pub frame_time_ms: f32,          // last frame
    pub frame_time_avg_ms: f32,      // rolling average
    pub frame_time_1percent_ms: f32, // 1% low
    pub frame_time_01percent_ms: f32,// 0.1% low
    pub available: bool,             // false if no frame data source active
}
```

All structs are `#[repr(C)]` because they cross process boundaries via shared memory.

### Shared Memory Layout

```rust
// shared/src/ipc_protocol.rs

use std::sync::atomic::AtomicU64;

pub const SHARED_MEM_NAME: &str = "GameOverlayMonitor_SharedState";
pub const MAX_WIDGETS: usize = 64;
pub const MAX_STYLE_SIZE: usize = 16384; // 16KB for serialized styles

#[repr(C)]
pub struct SharedOverlayState {
    pub write_sequence: AtomicU64,     // host increments on each write
    pub sensor_data: SensorSnapshot,
    pub layout_version: u64,           // bumped when widget config/style changes
    pub widget_count: u32,
    pub widgets: [ComputedWidget; MAX_WIDGETS],
}

/// A widget with its position/size already computed by taffy on the host side.
/// The DLL just reads these and draws.
#[repr(C)]
pub struct ComputedWidget {
    pub widget_type: WidgetType,       // text, graph, bar, etc.
    pub source: SensorSource,          // which sensor value to display
    pub x: f32,                        // computed position (pixels)
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub color_rgba: [u8; 4],
    pub bg_color_rgba: [u8; 4],
    pub border_color_rgba: [u8; 4],
    pub border_width: f32,
    pub opacity: f32,
    pub format_pattern: [u8; 64],      // e.g. "{value:.0f}°C" as null-terminated UTF-8
    pub critical_above: f32,           // threshold for critical color
    pub critical_color_rgba: [u8; 4],
    pub history_seconds: u32,          // for graph widgets
}

#[repr(C)]
pub enum WidgetType {
    Label,
    SensorValue,
    Graph,
    Bar,
    Spacer,
}

#[repr(C)]
pub enum SensorSource {
    CpuUsage,
    CpuTemp,
    CpuFreqCore(u8),
    GpuUsage,
    GpuTemp,
    GpuClock,
    GpuVram,
    GpuPower,
    RamUsage,
    RamTemp,
    RamFreq,
    Fps,
    FrameTime,
    FrameTime1Pct,
}
```

---

## DLL Injection

### Method: CreateRemoteThread + LoadLibraryW

```
Host calls OpenProcess(game_pid)
  → VirtualAllocEx (allocate memory in game process for DLL path string)
  → WriteProcessMemory (write DLL path)
  → GetProcAddress(kernel32, "LoadLibraryW")
  → CreateRemoteThread (call LoadLibraryW in the game with our DLL path)
  → Game process loads overlay.dll → DllMain runs → hooks installed
```

### Anti-Cheat Considerations

Games with kernel anti-cheat (EAC, BattlEye, Vanguard) will block or flag injection. This is an inherent limitation shared by RTSS, Steam Overlay, Discord Overlay, etc. Options:

- **Fallback mode**: Detect protected games and use a transparent `WS_EX_TOPMOST` borderless window overlay instead (works for borderless fullscreen, won't work for exclusive fullscreen on protected games).
- **Whitelist/exception**: Some anti-cheats allow users to whitelist processes.
- **Don't inject into protected games**: Maintain a list of games known to use kernel anti-cheat.

---

## Graphics API Hooking

### How It Works

1. DLL creates a temporary D3D device + swap chain to read the vtable.
2. `IDXGISwapChain::Present` is at vtable index 8.
3. Install a trampoline hook (via `retour` or `minhook` crate) that redirects Present to our detour function.
4. Our detour renders the overlay onto the back buffer, then calls the original Present.

### APIs to Hook

| API | Function to Hook | Detection |
|-----|-----------------|-----------|
| DirectX 11 | `IDXGISwapChain::Present` (vtable[8]) | Dummy device creation |
| DirectX 12 | `IDXGISwapChain::Present` (same COM interface) | Dummy device creation |
| Vulkan | `vkQueuePresentKHR` | `vkGetInstanceProcAddr` |
| OpenGL | `wglSwapBuffers` | `GetProcAddress(opengl32)` |

### API Detection Strategy

The DLL should detect which graphics API the game is using. Approaches:

1. **Check loaded modules**: On `DllMain` attach, check if `d3d11.dll`, `d3d12.dll`, `vulkan-1.dll`, or `opengl32.dll` are loaded in the process. Hook whichever is present.
2. **Lazy hooking**: Install hooks for all APIs on init. Only the one the game actually calls will fire. The others remain dormant.
3. **Host-side hint**: The host process can check the game's loaded modules before injection and pass a hint to the DLL via shared memory or a named pipe.

Option 2 is the most robust — some games load multiple graphics DLLs.

### Rendering in the Hook

**DX11 path** (most common, implement first):

1. `swap_chain.GetBuffer(0)` → get back buffer `ID3D11Texture2D`
2. Create a `ID3D11RenderTargetView` from it (cache after first frame)
3. Create a `ID2D1RenderTarget` that shares the DXGI surface (for text/2D drawing via Direct2D + DirectWrite)
4. Draw text labels, rectangles, graph lines using D2D
5. Return, let original Present submit the frame

**DX12 path**: More complex. Need own command allocator, command list, descriptor heap. Must synchronize with the game's command queue. Consider using D3D11On12 as a compatibility shim to reuse DX11-style D2D rendering on a DX12 swap chain.

**Vulkan path**: Need to create a render pass that targets the swap chain images. Requires managing descriptor sets, pipeline, and synchronization. Consider using a simple push-constant based pipeline that draws textured quads for widgets.

---

## Sensor Data Sources

### CPU & RAM (cross-platform baseline)

- **`sysinfo` crate**: CPU usage (total + per-core), core frequencies, memory used/total.
- Runs in the host process, no special permissions needed.

### Temperatures, Voltages, Fan Speeds (Windows)

- **LibreHardwareMonitor via WMI**: LHM exposes sensor data through WMI when running. Query with the `wmi` crate.
- WMI namespace: `root\LibreHardwareMonitor`
- Sensor types: Temperature, Clock, Load, Fan, Power, Voltage
- This covers CPU temp, GPU temp, fan RPM, VRM temps, RAM temps (if supported by motherboard).
- Fallback: read from `/sys/class/hwmon/` on Linux (future cross-platform support).

### GPU-Specific (NVAPI / ADL)

- **NVIDIA**: NVAPI SDK provides GPU usage, clock speeds, VRAM, temp, power draw. Write Rust FFI bindings to `nvapi64.dll`.
- **AMD**: ADL (AMD Display Library) provides equivalent data. Write FFI bindings to `atiadlxx.dll`.
- These provide more detailed/reliable GPU data than WMI.

### FPS & Frame Timing

- **PresentMon / ETW approach (recommended)**: Microsoft's PresentMon uses Event Tracing for Windows to capture GPU present events without injection. The host process runs an ETW consumer that watches for present events from the target game process. This gives FPS, frame time, and percentile data.
- **RTSS shared memory (alternative)**: If the user has RivaTuner running, read its shared memory segment for FPS/frame time. Well-documented format.
- **In-hook timing (simplest, less accurate)**: In the DLL, measure `Instant::now()` delta between consecutive Present calls. Gives you raw present-to-present time. Less accurate than ETW (doesn't account for present queue depth) but trivial to implement.

---

## CSS-Like Styling System

### Widget Definition (TOML)

Users define what widgets exist and what data they show:

```toml
[overlay]
class = "overlay"

[[widget]]
type = "sensor"
source = "cpu.temp"
label = "CPU"
class = "sensor-value"
format = "{value:.0f}°C"
critical_above = 85.0

[[widget]]
type = "sensor"
source = "gpu.usage"
label = "GPU"
class = "sensor-value"
format = "{value:.0f}%"

[[widget]]
type = "graph"
source = "cpu.total_usage"
class = "cpu-graph"
history_seconds = 60

[[widget]]
type = "bar"
source = "ram.usage"
class = "ram-bar"
```

### User Stylesheet (CSS subset)

Users style their widgets with a restricted CSS dialect:

```css
.overlay {
    position: fixed;
    top: 10px;
    right: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    opacity: 0.85;
    padding: 8px;
    background: rgba(0, 0, 0, 0.6);
    border-radius: 4px;
}

.sensor-value {
    font-family: "JetBrains Mono", monospace;
    font-size: 13px;
    color: #cccccc;
}

.sensor-value.critical {
    color: #ff4444;
}

.cpu-graph {
    width: 120px;
    height: 40px;
    background: rgba(0, 0, 0, 0.4);
    border: 1px solid #444444;
}

.ram-bar {
    width: 120px;
    height: 8px;
    background: #333333;
    border-radius: 4px;
}
```

### Supported CSS Properties (initial subset)

**Layout** (computed via `taffy`): `display`, `flex-direction`, `justify-content`, `align-items`, `gap`, `width`, `height`, `min-width`, `min-height`, `max-width`, `max-height`, `padding`, `margin`, `position`, `top`, `right`, `bottom`, `left`.

**Visual** (applied in DLL renderer): `color`, `background` (solid + rgba), `border`, `border-radius`, `opacity`, `font-family`, `font-size`, `font-weight`.

### Style Pipeline

```
User CSS file
  → cssparser crate (parse into property/value pairs)
  → Match selectors to widget tree nodes (class-based matching)
  → Resolve cascaded values (specificity, inheritance)
  → Feed layout props to taffy → compute positions/sizes
  → Package computed styles + positions into SharedOverlayState
  → DLL reads and renders
```

---

## Crate Dependencies

### Host Process

| Crate | Purpose |
|-------|---------|
| `windows` (windows-rs) | Win32 APIs, process management, shared memory |
| `sysinfo` | CPU usage, core frequencies, RAM usage |
| `wmi` | Query LibreHardwareMonitor for temps/fans/voltages |
| `cssparser` | Parse CSS subset (Mozilla/Servo's CSS parser) |
| `taffy` | Flexbox/grid layout engine (compute widget positions) |
| `serde` + `toml` | Deserialize widget config |
| `tokio` or `crossbeam` | Async sensor polling / channels |

### Overlay DLL

| Crate | Purpose |
|-------|---------|
| `windows` (windows-rs) | D3D11, D3D12, DXGI, D2D1, DirectWrite, Vulkan |
| `retour` or `minhook` | Function detouring / trampoline hooks |

### Shared

| Crate | Purpose |
|-------|---------|
| `serde` | (Optional) Serialization for non-fixed-size data |

Minimize DLL dependencies. Every crate in the DLL increases the risk of conflicts with the game process.

---

## Build Phases

### Phase 1: DLL Injection Proof of Concept

**Goal**: Inject a Rust DLL into a test process and confirm code runs inside it.

- Build a minimal `cdylib` with `DllMain` that writes to a log file on attach.
- Build the host-side injector using `CreateRemoteThread` + `LoadLibraryW`.
- Test against a simple DirectX 11 sample app (not a real game yet).
- **Success criteria**: Log file appears, confirming DLL loaded in target process.

### Phase 2: Hook DX11 Present

**Goal**: Hook `IDXGISwapChain::Present` and confirm the hook fires every frame.

- Use dummy swap chain vtable trick to find Present address.
- Install trampoline hook via `retour` or `minhook`.
- Detour function increments a counter, writes to log every 60 calls.
- **Success criteria**: Log shows hook firing ~60 times/sec in the test app.

### Phase 3: Render Text on Back Buffer (Critical Milestone)

**Goal**: Draw "Hello World" on the game's back buffer using D2D + DirectWrite.

- Get back buffer from swap chain in the Present hook.
- Create D2D render target sharing the DXGI surface.
- Draw text at a fixed position.
- **Success criteria**: Text visible on top of the test app's rendering. No crashes.

### Phase 4: Sensor Polling + IPC

**Goal**: Live hardware data displayed in the overlay.

- Build sensor polling in the host (`sysinfo` + `wmi`).
- Set up named shared memory between host and DLL.
- Host writes `SensorSnapshot` at 1Hz, DLL reads each frame and displays values.
- **Success criteria**: Real CPU temp/usage/RAM values shown in the overlay.

### Phase 5: Widget System + CSS Styling

**Goal**: User-configurable layout and appearance.

- TOML config for widget definitions.
- CSS-subset parser (`cssparser`) in the host.
- `taffy` computes layout from CSS properties.
- Host serializes computed `ComputedWidget` array to shared memory.
- DLL renders widgets at computed positions with computed styles.
- **Success criteria**: Changing the CSS file changes the overlay appearance.

### Phase 6: Additional Graphics APIs

**Goal**: Support DX12, Vulkan, OpenGL games.

- Implement DX12 Present hook + renderer (consider D3D11On12 bridge).
- Implement Vulkan `vkQueuePresentKHR` hook + renderer.
- Implement OpenGL `wglSwapBuffers` hook + renderer.
- Auto-detect which API the game uses and activate the correct hook.
- **Success criteria**: Overlay works on games using each API.

### Phase 7: Advanced Sensors

**Goal**: Complete sensor coverage.

- NVAPI FFI bindings for detailed NVIDIA GPU stats.
- ADL FFI bindings for AMD GPU stats.
- PresentMon ETW consumer for accurate FPS/frame timing.
- Fallback to in-hook timing for simpler FPS measurement.
- **Success criteria**: Full sensor dashboard with FPS/frame time data.

### Phase 8: Polish

- Tray icon / minimal host UI.
- Hot-reload CSS on file change (use `notify` crate for file watching).
- Graceful DLL unload (unhook, cleanup D2D resources, unmap shared memory).
- Error recovery (if game crashes, host detects process exit and cleans up).
- Anti-cheat detection and automatic fallback to borderless window overlay mode.
