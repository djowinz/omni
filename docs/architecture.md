# Architecture

Omni is a Windows game overlay system with three main components: a desktop editor, a host service, and an overlay renderer.

## Repository Structure

```
omni/
├── apps/desktop/          Electron + Next.js desktop editor
│   ├── main/              Electron main process (IPC, host management, auto-update)
│   ├── renderer/          Next.js renderer (editor UI, preview, settings)
│   └── resources/         Fonts, icons
├── crates/
│   ├── host/              omni-host service binary
│   ├── overlay-exe/       External overlay process (anti-cheat fallback)
│   ├── shared/            Shared types (sensor data, IPC structs)
│   └── ultralight-sys/    Ultralight C FFI bindings
├── vendor/ultralight/     Pre-built Ultralight SDK (DLLs, headers, resources)
├── scripts/               Build and release automation
└── Cargo.toml             Rust workspace root
```

## Component Overview

### Desktop Editor (`apps/desktop/`)

A Nextron app (Next.js + Electron) providing the visual editor for creating overlays.

- **Main process** — Manages the host service lifecycle, WebSocket connection, IPC bridge, auto-updates via electron-updater, and the system tray
- **Renderer** — Monaco editor with custom .omni language support (tokenizer, CSS/HTML intellisense via vscode-languageservice), live preview panel with DOM-preserving updates, settings management

Communication between renderer and main process uses Electron IPC. The main process forwards messages to the host over WebSocket.

### Host Service (`crates/host/`)

A Rust service (`omni-host.exe`) that runs with administrator privileges. Responsible for:

- **Sensor polling** — CPU, GPU, RAM, and frame metrics via WMI, sysinfo, and ETW
- **Overlay parsing** — Parses .omni files (XML-based format with `<widget>`, `<template>`, `<style>` blocks) and validates element names and sensor paths
- **HTML rendering** — Builds HTML from parsed overlays, injects feather icon font via base64 data URI, renders to bitmap via Ultralight
- **Game detection** — Scans for running games using process enumeration, path-based heuristics, and configurable include/exclude lists
- **Overlay injection** — Writes rendered bitmap to shared memory for the overlay process
- **WebSocket API** — Serves the desktop editor on `ws://127.0.0.1:9473`

### Overlay Process (`crates/overlay/`)

A lightweight process (`omni-overlay.exe`) that renders the overlay on top of games:

- Creates a transparent, click-through, always-on-top window using DirectComposition
- Reads BGRA bitmap from shared memory (written by the host's Ultralight renderer)
- Uploads to a D3D11 staging texture and presents via swap chain
- Targets the game window by HWND, matching its position and size

This external overlay approach works with anti-cheat protected games since it doesn't inject into the game process.

### Shared Library (`crates/shared/`)

Common types used across crates:

- `SensorSnapshot` — Frame, CPU, GPU, RAM sensor data structs (`#[repr(C)]` for shared memory)
- `BitmapHeader` — Shared memory layout for host → overlay bitmap IPC
- TypeScript bindings auto-generated via [ts-rs](https://github.com/Aleph-Alpha/ts-rs) into `apps/desktop/renderer/generated/`

### Ultralight FFI (`crates/ultralight-sys/`)

Minimal FFI bindings to the Ultralight HTML/CSS rendering engine. The build script copies DLLs and resources from `vendor/ultralight/` to the target directory.

## Data Flow

```
User edits .omni file in Monaco
       │
       ▼
Renderer ──IPC──► Main Process ──WebSocket──► omni-host
                                                │
                                    ┌───────────┴───────────┐
                                    │                       │
                              Parse .omni             Poll sensors
                                    │                       │
                                    ▼                       ▼
                              Build HTML ◄──── Interpolate {metrics}
                                    │
                                    ▼
                            Ultralight render → BGRA bitmap
                                    │
                                    ▼
                            Shared memory write
                                    │
                                    ▼
                          omni-overlay.exe reads bitmap
                                    │
                                    ▼
                          D3D11 texture → DirectComposition
                                    │
                                    ▼
                            Overlay on game window
```

## Configuration

All user data lives in `%APPDATA%\Omni\`:

- `config.json` — Scanner settings, keybinds, active overlay, game assignments
- `overlays/{name}/overlay.omni` — Overlay definitions
- `themes/*.css` — Shared CSS themes
- `logs/omni-host.log` — Host service log (capped at 5MB)

## Build System

The project uses a Makefile as the unified entry point:

| Command | Description |
|---------|-------------|
| `make build` | Build Rust + desktop |
| `make test` | Run all tests |
| `make installer` | Full installer (builds everything first) |
| `make release INCREMENT=patch` | Full release pipeline |
| `make dev` | Start desktop dev server |

See [Contributing](contributing.md) for development setup details.
