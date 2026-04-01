# Phase 11a+11e: Electron Shell + Installer Design Spec

## Overview

A Nextron (Next.js + Electron) desktop app that wraps the existing `omni-host.exe` backend. Phase 11a provides the app shell (window management, tray icon, host process lifecycle, WebSocket connection). Phase 11e provides the NSIS Windows installer. The renderer UI is deliberately minimal — a connection status indicator and basic host info — to avoid throwaway code before the Phase 13 editor features.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Framework | Nextron (Next.js + Electron + TypeScript) | File-based routing, full React ecosystem, foundation for Phase 13 editor |
| Repo structure | Monorepo — `desktop/` folder in existing repo | Single clone, versioned together, shared build pipeline |
| Type sharing | `ts-rs` with `cargo ts-rs export` CLI | Rust types are source of truth, TypeScript generated on demand, no test side effects |
| Host lifecycle | Electron-managed child process | No admin required, no Windows service complexity |
| Auto-start | Optional scheduled task via `schtasks` | User-level, no admin, runs `omni-host.exe --service` at logon |
| Window close | Minimize to tray | Host stays alive, tray menu for Open/Quit |
| Installer | NSIS "Next, Next, Finish" wizard | Standard Windows installer UX |
| Uninstaller | Opt-in user data removal | Checkbox: "Also remove all user data" (unchecked by default) |

---

## Project Structure

```
omni/
  Cargo.toml                    # Rust workspace (add ts-rs dev-dependency)
  shared/                       # omni-shared (+ #[derive(TS)] on exported types)
  host/                         # omni-host (+ #[derive(TS)] on WS message types)
  overlay-dll/                  # unchanged

  desktop/                      # NEW: Nextron app
    package.json
    tsconfig.json
    nextron.config.js
    main/                       # Electron main process
      background.ts               App lifecycle, tray, window management
      host-manager.ts              Spawn/connect omni-host.exe
      auto-start.ts                Scheduled task registration
    renderer/                   # Next.js renderer process
      pages/
        index.tsx                  Connection status + host info
      components/
        ConnectionStatus.tsx       Green/red dot + status text
      hooks/
        useWebSocket.ts            Type-safe reconnecting WS hook
    src/
      generated/                # ts-rs output (gitignored)
        OmniFile.ts
        SensorSnapshot.ts
        ...
    resources/                  # App icon, tray icon

  installer/                    # NEW: NSIS installer config
    installer.nsi
    omni-icon.ico
    license.txt
```

The Rust workspace (`Cargo.toml`) is unchanged — `desktop/` is a sibling directory with its own `package.json`, not a Cargo workspace member.

---

## Type Generation (ts-rs)

Rust types are the single source of truth for WebSocket message shapes. TypeScript interfaces are generated via `ts-rs` using the dedicated CLI command:

```bash
cargo ts-rs export --output-directory desktop/src/generated
```

This is an explicit build step — never triggered by `cargo test`. The `desktop/src/generated/` directory is gitignored and regenerated as needed.

Types that get `#[derive(TS)]` with `#[ts(export)]`:
- `OmniFile`, `Widget`, `HtmlNode` (from `host/src/omni/types.rs`)
- WebSocket message request/response types (from `host/src/ws_server.rs`)
- `SensorSnapshot`, `CpuData`, `GpuData`, `RamData`, `FrameData` (from `shared/src/sensor_types.rs`) — only the ones used in WebSocket responses

---

## Electron Main Process

### App Lifecycle (`background.ts`)

1. **On app launch:**
   - Create the main `BrowserWindow` (loads Next.js `index.tsx`)
   - Create the system tray icon with context menu
   - Call `HostManager.connect()` — attempts WebSocket to `localhost:9473`
     - If connected: reuse existing host (user has "Run on startup" enabled)
     - If not connected: spawn `omni-host.exe --service` as child process, then connect

2. **Window close (X button):**
   - Hide the window (do not destroy it)
   - App stays alive in tray
   - Host process keeps running

3. **Tray icon:**
   - Left-click: show/focus the main window
   - Right-click context menu:
     - "Open Omni" — show/focus window
     - "Quit" — kill host, destroy window, exit app

4. **App quit (from tray "Quit"):**
   - If host was spawned by us: kill the child process
   - If host was pre-existing (connected to running instance): leave it running
   - Destroy window, exit app

### Host Manager (`host-manager.ts`)

- **Locates `omni-host.exe`**: relative to the Electron app's install directory (same pattern as `discover_dll_path` in the Rust host — looks for `omni-host.exe` next to `Omni.exe`)
- **Spawns as detached child process**: `child_process.spawn('omni-host.exe', ['--service'], { detached: true, stdio: 'pipe' })`, stdout/stderr piped to a log file in `%APPDATA%\Omni\logs\`
- **WebSocket connection**: reconnecting wrapper around `ws` (npm package)
  - On connect: emit "connected" event with host status
  - On disconnect: emit "disconnected", retry every 2 seconds
  - On message: parse JSON, dispatch to subscribers
  - Typed with ts-rs generated interfaces
- **Host crash detection**: if child process exits unexpectedly, emit "crashed" event. Renderer shows a notification.

### Auto-Start (`auto-start.ts`)

- **Enable**: `schtasks /create /tn "OmniOverlay" /tr "\"C:\Program Files\Omni\omni-host.exe\" --service" /sc ONLOGON /rl LIMITED /f`
- **Disable**: `schtasks /delete /tn "OmniOverlay" /f`
- **Query**: `schtasks /query /tn "OmniOverlay"` — returns whether the task exists
- All commands run at user privilege level (no UAC prompt)

---

## Renderer UI

Single page (`index.tsx`) with minimal content:

### Connected state:
- Green dot
- "Connected to host"
- Active overlay name (from `status` API response)
- Currently injected game, if any (from `status` API response)
- Version number in footer

### Disconnected state:
- Red pulsing dot
- "Connecting to host..."
- "Retrying in Ns" countdown
- Version number in footer

### Components:
- `ConnectionStatus.tsx` — renders the dot + text based on WebSocket state
- `useWebSocket.ts` hook — exposes `{ connected, status, send }` to components, uses ts-rs generated types for message shapes

No overlay browser, no settings page, no file management. The single page is a placeholder that Phase 13 will replace.

---

## NSIS Installer

### Installed files (`C:\Program Files\Omni\`)

```
Omni.exe                    # Electron app executable
omni-host.exe               # Rust host binary
overlay/
  omni_overlay.dll          # Injected DLL
resources/
  app.asar                  # Electron bundle
  ...                       # Electron runtime files
```

### Installer behavior

1. License agreement page
2. Install directory selection (default: `C:\Program Files\Omni`)
3. Install files
4. Create Start Menu shortcut → `Omni.exe`
5. Register uninstaller in Add/Remove Programs
6. Finish page with optional "Create desktop shortcut" checkbox
7. Optional "Launch Omni" checkbox on finish

### Uninstaller behavior

1. Run `omni-host.exe --stop` to eject DLLs from games and kill running host instances
2. Remove the `OmniOverlay` scheduled task if it exists
3. Delete all files from the install directory
4. Remove Start Menu and desktop shortcuts
5. Remove Add/Remove Programs registry entry
6. **Checkbox: "Also remove all user data (overlays, themes, configuration)"** — unchecked by default. If checked, deletes `%APPDATA%\Omni\` entirely.

### Build pipeline

```
1. cargo build --release
   → target/release/omni-host.exe
   → target/release/omni_overlay.dll

2. cargo ts-rs export --output-directory desktop/src/generated
   → desktop/src/generated/*.ts

3. cd desktop && npm run build
   → Nextron packages Electron app (dist/)

4. makensis installer/installer.nsi
   → OmniSetup.exe
```

A `build.ps1` script orchestrates all four steps. CI/CD is out of scope for this phase.

---

## Phase Reorganization

The design spec (`2026-03-27-omni-overlay-design.md`) should be updated to reflect:

**Phase 11 (this spec):**
- 11a: Electron app shell (Nextron, tray, host lifecycle, WebSocket connection)
- 11e: NSIS installer + distribution

**Phase 13 (deferred, future specs):**
- Monaco code editor with `.omni` IntelliSense
- Live HTML/CSS preview
- Visual drag/drop widget editor
- Built-in themes (dark, cyberpunk, retro)

---

## Out of Scope

- Monaco editor, code editing, IntelliSense (Phase 13)
- Live preview (Phase 13)
- Visual drag/drop editor (Phase 13)
- Built-in themes beyond what already ships as `dark.css` (Phase 13)
- CI/CD pipeline (future)
- Auto-update mechanism (future)
- macOS/Linux support (Windows only)
