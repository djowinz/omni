# Phase 11a+11e: Electron Shell + Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Nextron desktop app that manages `omni-host.exe` lifecycle (spawn, connect, tray), with an NSIS installer for Windows distribution.

**Architecture:** Nextron (Next.js + Electron) app in `desktop/` alongside existing Rust workspace. Electron main process spawns/connects to `omni-host.exe` via WebSocket on port 9473. TypeScript interfaces generated from Rust types via `cargo ts-rs export`. NSIS bundles everything into `OmniSetup.exe`.

**Tech Stack:** Nextron, Next.js 14, React 18, TypeScript, Electron 33+, ts-rs, NSIS 3

**Spec:** `docs/superpowers/specs/2026-04-01-electron-shell-installer-design.md`

---

## File Structure

### New Files (desktop/)
- `desktop/package.json` — Nextron app dependencies
- `desktop/tsconfig.json` — TypeScript config
- `desktop/nextron.config.js` — Nextron build config
- `desktop/main/background.ts` — Electron main process: window, tray, lifecycle
- `desktop/main/host-manager.ts` — Spawn/connect to omni-host.exe, reconnecting WebSocket
- `desktop/main/auto-start.ts` — Scheduled task create/delete/query via schtasks
- `desktop/renderer/pages/index.tsx` — Single page: connection status + host info
- `desktop/renderer/components/ConnectionStatus.tsx` — Green/red dot + text
- `desktop/renderer/hooks/useWebSocket.ts` — IPC bridge to main process WebSocket
- `desktop/resources/icon.ico` — App icon (placeholder)
- `desktop/.gitignore` — node_modules, out, dist, src/generated

### New Files (installer/)
- `installer/installer.nsi` — NSIS script
- `installer/license.txt` — License text

### New Files (root)
- `build.ps1` — Orchestrates full build pipeline

### Modified Files (Rust — ts-rs integration)
- `host/Cargo.toml` — Add `ts-rs` dev-dependency
- `shared/Cargo.toml` — Add `ts-rs` dev-dependency
- `host/src/omni/types.rs` — Add `#[derive(TS)]` to OmniFile, Widget, HtmlNode, etc.
- `host/src/ws_server.rs` — Add WebSocket message types with `#[derive(TS)]`
- `shared/src/sensor_types.rs` — Add `#[derive(TS)]` to SensorSnapshot and sub-types

---

## Task 1: Add ts-rs to Rust crates and derive TypeScript types

**Files:**
- Modify: `host/Cargo.toml`
- Modify: `shared/Cargo.toml`
- Modify: `host/src/omni/types.rs`
- Modify: `shared/src/sensor_types.rs`

- [ ] **Step 1: Add ts-rs dependency to both crates**

In `shared/Cargo.toml`, add under `[dependencies]`:
```toml
ts-rs = { version = "10", features = ["serde-compat"] }
```

In `host/Cargo.toml`, add under `[dependencies]`:
```toml
ts-rs = { version = "10", features = ["serde-compat"] }
```

- [ ] **Step 2: Add `#[derive(TS)]` to shared types**

In `shared/src/sensor_types.rs`, add the derive and export attribute to the types that are sent over WebSocket. Add `use ts_rs::TS;` at the top, then add `#[derive(TS)]` and `#[ts(export, export_to = "../../desktop/src/generated/")]` to:
- `SensorSnapshot`
- `CpuData`
- `GpuData`
- `RamData`
- `FrameData`

Example for `SensorSnapshot`:
```rust
use ts_rs::TS;

#[repr(C)]
#[derive(Clone, Copy, Debug, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub struct SensorSnapshot {
    pub timestamp_ms: u64,
    pub cpu: CpuData,
    pub gpu: GpuData,
    pub ram: RamData,
    pub frame: FrameData,
}
```

Apply the same pattern to `CpuData`, `GpuData`, `RamData`, `FrameData`.

Note: The `per_core_usage: [f32; 32]` and `per_core_freq_mhz: [u32; 32]` fixed arrays in `CpuData` may need `#[ts(type = "number[]")]` annotation since ts-rs doesn't natively handle fixed-size arrays well.

- [ ] **Step 3: Add `#[derive(TS)]` to omni types**

In `host/src/omni/types.rs`, add `use ts_rs::TS;` and `#[derive(TS)]` + `#[ts(export, export_to = "../../desktop/src/generated/")]` to:
- `OmniFile`
- `Widget`
- `HtmlNode`
- `ConditionalClass`

These already have `#[derive(Serialize, Deserialize)]` so ts-rs's serde-compat feature will use the serde attributes for TypeScript generation.

- [ ] **Step 4: Install ts-rs CLI and test generation**

```bash
cargo install ts-rs-cli
cargo ts-rs export --output-directory desktop/src/generated
```

Verify that `.ts` files appear in `desktop/src/generated/`.

- [ ] **Step 5: Run Rust tests to ensure derives don't break anything**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add shared/Cargo.toml host/Cargo.toml shared/src/sensor_types.rs host/src/omni/types.rs
git commit -m "feat: add ts-rs derives for TypeScript type generation from Rust types"
```

---

## Task 2: Scaffold Nextron app

**Files:**
- Create: `desktop/package.json`
- Create: `desktop/tsconfig.json`
- Create: `desktop/nextron.config.js`
- Create: `desktop/.gitignore`
- Create: `desktop/renderer/pages/index.tsx`
- Create: `desktop/main/background.ts`

- [ ] **Step 1: Initialize the Nextron project**

```bash
cd C:/Users/DyllenOwens/Projects/omni
npx create-nextron-app desktop --example with-typescript
```

This scaffolds the basic Nextron structure. If the command creates files in unexpected locations, move them to match our planned structure.

- [ ] **Step 2: Verify the scaffold builds and runs**

```bash
cd desktop
npm install
npm run dev
```

Expected: Electron window opens with the default Nextron template page. Close it after confirming.

- [ ] **Step 3: Clean up scaffold to minimal state**

Replace the contents of `desktop/renderer/pages/index.tsx` with a minimal placeholder:

```tsx
export default function Home() {
  return (
    <div style={{
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      height: '100vh',
      background: '#0f0f1a',
      color: '#c0c0d0',
      fontFamily: 'system-ui, sans-serif',
    }}>
      <p>Omni — connecting...</p>
    </div>
  );
}
```

Remove any extra pages, components, or styles from the scaffold that we don't need.

- [ ] **Step 4: Add `.gitignore` for desktop**

Create `desktop/.gitignore`:
```
node_modules/
out/
dist/
.next/
src/generated/
```

- [ ] **Step 5: Verify dev mode still works**

```bash
cd desktop && npm run dev
```

Expected: Electron window opens showing "Omni — connecting..."

- [ ] **Step 6: Commit**

```bash
git add desktop/
git commit -m "feat(desktop): scaffold Nextron app with minimal placeholder"
```

---

## Task 3: Host Manager — spawn and connect to omni-host.exe

**Files:**
- Create: `desktop/main/host-manager.ts`
- Modify: `desktop/main/background.ts`

- [ ] **Step 1: Create host-manager.ts**

```typescript
import { ChildProcess, spawn } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import WebSocket from 'ws';
import { EventEmitter } from 'events';
import { app } from 'electron';

const WS_PORT = 9473;
const WS_URL = `ws://127.0.0.1:${WS_PORT}`;
const RECONNECT_INTERVAL_MS = 2000;

export interface HostStatus {
  connected: boolean;
  activeOverlay?: string;
  injectedGame?: string;
}

export class HostManager extends EventEmitter {
  private ws: WebSocket | null = null;
  private hostProcess: ChildProcess | null = null;
  private reconnectTimer: NodeJS.Timeout | null = null;
  private intentionalClose = false;
  private _status: HostStatus = { connected: false };

  get status(): HostStatus {
    return this._status;
  }

  /** Try to connect to an existing host, spawn one if not running. */
  async start(): Promise<void> {
    const connected = await this.tryConnect();
    if (!connected) {
      this.spawnHost();
      // Give host time to start, then connect
      await new Promise(resolve => setTimeout(resolve, 1500));
      await this.tryConnect();
    }
  }

  /** Attempt a WebSocket connection. Returns true if successful. */
  private tryConnect(): Promise<boolean> {
    return new Promise(resolve => {
      try {
        const ws = new WebSocket(WS_URL);
        const timeout = setTimeout(() => {
          ws.close();
          resolve(false);
        }, 2000);

        ws.on('open', () => {
          clearTimeout(timeout);
          this.onConnected(ws);
          resolve(true);
        });

        ws.on('error', () => {
          clearTimeout(timeout);
          resolve(false);
        });
      } catch {
        resolve(false);
      }
    });
  }

  private onConnected(ws: WebSocket): void {
    this.ws = ws;
    this._status = { connected: true };
    this.emit('connected');

    // Request initial status
    this.send({ type: 'status' });

    ws.on('message', (data: Buffer) => {
      try {
        const msg = JSON.parse(data.toString());
        this.handleMessage(msg);
      } catch { /* ignore malformed */ }
    });

    ws.on('close', () => {
      if (!this.intentionalClose) {
        this._status = { connected: false };
        this.emit('disconnected');
        this.scheduleReconnect();
      }
    });

    ws.on('error', () => {
      // close event will fire after this
    });
  }

  private handleMessage(msg: any): void {
    if (msg.type === 'status.data') {
      this._status = {
        connected: true,
        activeOverlay: msg.active_overlay,
        injectedGame: msg.injected_game,
      };
      this.emit('status', this._status);
    }
    this.emit('message', msg);
  }

  send(msg: object): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    this.reconnectTimer = setInterval(async () => {
      const connected = await this.tryConnect();
      if (connected && this.reconnectTimer) {
        clearInterval(this.reconnectTimer);
        this.reconnectTimer = null;
      }
    }, RECONNECT_INTERVAL_MS);
  }

  private spawnHost(): void {
    const hostPath = this.findHostExe();
    if (!hostPath) {
      this.emit('error', 'Could not find omni-host.exe');
      return;
    }

    const logDir = path.join(app.getPath('userData'), 'logs');
    if (!fs.existsSync(logDir)) {
      fs.mkdirSync(logDir, { recursive: true });
    }

    const logStream = fs.createWriteStream(
      path.join(logDir, 'omni-host.log'),
      { flags: 'a' }
    );

    this.hostProcess = spawn(hostPath, ['--service'], {
      detached: true,
      stdio: ['ignore', logStream, logStream],
    });

    this.hostProcess.on('exit', (code) => {
      if (!this.intentionalClose) {
        this.emit('host-crashed', code);
      }
      this.hostProcess = null;
    });

    // Don't let the child process prevent app exit
    this.hostProcess.unref();
  }

  private findHostExe(): string | null {
    // In installed layout: omni-host.exe is next to Omni.exe
    const installedPath = path.join(path.dirname(app.getPath('exe')), 'omni-host.exe');
    if (fs.existsSync(installedPath)) return installedPath;

    // Dev layout: look in target/debug or target/release
    const devDebug = path.resolve(__dirname, '../../target/debug/omni-host.exe');
    if (fs.existsSync(devDebug)) return devDebug;

    const devRelease = path.resolve(__dirname, '../../target/release/omni-host.exe');
    if (fs.existsSync(devRelease)) return devRelease;

    return null;
  }

  async shutdown(): Promise<void> {
    this.intentionalClose = true;

    if (this.reconnectTimer) {
      clearInterval(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }

    if (this.hostProcess) {
      // We spawned it, so kill it
      this.hostProcess.kill();
      this.hostProcess = null;
    }
  }
}
```

- [ ] **Step 2: Install `ws` dependency**

```bash
cd desktop
npm install ws
npm install --save-dev @types/ws
```

- [ ] **Step 3: Wire up host-manager in background.ts**

Replace `desktop/main/background.ts` with:

```typescript
import { app, BrowserWindow, Tray, Menu, nativeImage } from 'electron';
import * as path from 'path';
import serve from 'electron-serve';
import { HostManager } from './host-manager';

const isProd = process.env.NODE_ENV === 'production';

if (isProd) {
  serve({ directory: 'app' });
}

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
const hostManager = new HostManager();

function createWindow(): BrowserWindow {
  const win = new BrowserWindow({
    width: 480,
    height: 360,
    title: 'Omni',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
    show: false,
  });

  win.once('ready-to-show', () => win.show());

  // Minimize to tray on close
  win.on('close', (e) => {
    if (!app.isQuitting) {
      e.preventDefault();
      win.hide();
    }
  });

  return win;
}

function createTray(): void {
  // Use a simple 16x16 icon — replace with real icon later
  const iconPath = path.join(__dirname, '../resources/icon.png');
  const icon = nativeImage.createEmpty();
  tray = new Tray(icon);
  tray.setToolTip('Omni Overlay');

  const contextMenu = Menu.buildFromTemplate([
    {
      label: 'Open Omni',
      click: () => {
        mainWindow?.show();
        mainWindow?.focus();
      },
    },
    { type: 'separator' },
    {
      label: 'Quit',
      click: () => {
        (app as any).isQuitting = true;
        app.quit();
      },
    },
  ]);

  tray.setContextMenu(contextMenu);
  tray.on('click', () => {
    mainWindow?.show();
    mainWindow?.focus();
  });
}

app.on('ready', async () => {
  mainWindow = createWindow();
  createTray();

  if (isProd) {
    await mainWindow.loadURL('app://./');
  } else {
    const port = process.argv[2] || 8888;
    await mainWindow.loadURL(`http://localhost:${port}/`);
  }

  // Start host manager
  await hostManager.start();

  // Forward status to renderer via IPC
  hostManager.on('connected', () => {
    mainWindow?.webContents.send('host-status', hostManager.status);
  });
  hostManager.on('disconnected', () => {
    mainWindow?.webContents.send('host-status', { connected: false });
  });
  hostManager.on('status', (status) => {
    mainWindow?.webContents.send('host-status', status);
  });
});

app.on('before-quit', async () => {
  await hostManager.shutdown();
});

app.on('window-all-closed', () => {
  // Don't quit on window close — tray keeps us alive
});
```

- [ ] **Step 4: Create a preload script for IPC**

Create `desktop/main/preload.ts`:

```typescript
import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('omni', {
  onHostStatus: (callback: (status: any) => void) => {
    ipcRenderer.on('host-status', (_event, status) => callback(status));
  },
});
```

- [ ] **Step 5: Verify it compiles**

```bash
cd desktop && npm run build
```

Expected: Builds without TypeScript errors.

- [ ] **Step 6: Commit**

```bash
git add desktop/
git commit -m "feat(desktop): host manager — spawn/connect omni-host.exe with reconnecting WebSocket"
```

---

## Task 4: Renderer UI — connection status page

**Files:**
- Create: `desktop/renderer/components/ConnectionStatus.tsx`
- Create: `desktop/renderer/hooks/useHostStatus.ts`
- Modify: `desktop/renderer/pages/index.tsx`

- [ ] **Step 1: Create the useHostStatus hook**

Create `desktop/renderer/hooks/useHostStatus.ts`:

```typescript
import { useState, useEffect } from 'react';

interface HostStatus {
  connected: boolean;
  activeOverlay?: string;
  injectedGame?: string;
}

declare global {
  interface Window {
    omni?: {
      onHostStatus: (callback: (status: HostStatus) => void) => void;
    };
  }
}

export function useHostStatus(): HostStatus {
  const [status, setStatus] = useState<HostStatus>({ connected: false });

  useEffect(() => {
    window.omni?.onHostStatus((newStatus) => {
      setStatus(newStatus);
    });
  }, []);

  return status;
}
```

- [ ] **Step 2: Create ConnectionStatus component**

Create `desktop/renderer/components/ConnectionStatus.tsx`:

```tsx
import React from 'react';

interface Props {
  connected: boolean;
  activeOverlay?: string;
  injectedGame?: string;
}

export function ConnectionStatus({ connected, activeOverlay, injectedGame }: Props) {
  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 12,
    }}>
      <div style={{
        width: 10,
        height: 10,
        borderRadius: '50%',
        background: connected ? '#4ade80' : '#ef4444',
        animation: connected ? undefined : 'pulse 2s infinite',
      }} />
      <span style={{ color: '#c0c0d0', fontSize: 14 }}>
        {connected ? 'Connected to host' : 'Connecting to host...'}
      </span>
      {connected && activeOverlay && (
        <span style={{ color: '#505060', fontSize: 12 }}>
          Active overlay: {activeOverlay}
        </span>
      )}
      {connected && injectedGame && (
        <span style={{ color: '#505060', fontSize: 12 }}>
          Injected: {injectedGame}
        </span>
      )}
      {!connected && (
        <span style={{ color: '#505060', fontSize: 12 }}>
          Retrying...
        </span>
      )}
    </div>
  );
}
```

- [ ] **Step 3: Update index.tsx**

Replace `desktop/renderer/pages/index.tsx`:

```tsx
import React from 'react';
import { ConnectionStatus } from '../components/ConnectionStatus';
import { useHostStatus } from '../hooks/useHostStatus';

export default function Home() {
  const status = useHostStatus();

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100vh',
      background: '#0f0f1a',
      fontFamily: 'system-ui, -apple-system, sans-serif',
    }}>
      {/* Main content */}
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}>
        <ConnectionStatus
          connected={status.connected}
          activeOverlay={status.activeOverlay}
          injectedGame={status.injectedGame}
        />
      </div>

      {/* Status bar */}
      <div style={{
        background: '#1a1a2e',
        borderTop: '1px solid #2a2a40',
        padding: '6px 16px',
        fontSize: 11,
        color: '#505060',
      }}>
        v0.1.0
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Add pulse animation CSS**

Create `desktop/renderer/styles/globals.css`:

```css
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

body {
  margin: 0;
  padding: 0;
  overflow: hidden;
}
```

Import it in `desktop/renderer/pages/_app.tsx` (create if not exists):

```tsx
import type { AppProps } from 'next/app';
import '../styles/globals.css';

export default function App({ Component, pageProps }: AppProps) {
  return <Component {...pageProps} />;
}
```

- [ ] **Step 5: Test dev mode**

```bash
cd desktop && npm run dev
```

Expected: Electron window shows red pulsing dot with "Connecting to host..." (unless omni-host is running, in which case green dot with "Connected to host").

- [ ] **Step 6: Commit**

```bash
git add desktop/
git commit -m "feat(desktop): connection status UI with host status IPC"
```

---

## Task 5: Auto-start via scheduled task

**Files:**
- Create: `desktop/main/auto-start.ts`

- [ ] **Step 1: Create auto-start.ts**

```typescript
import { execSync } from 'child_process';
import { app } from 'electron';
import * as path from 'path';

const TASK_NAME = 'OmniOverlay';

/** Check if the auto-start scheduled task exists. */
export function isAutoStartEnabled(): boolean {
  try {
    execSync(`schtasks /query /tn "${TASK_NAME}"`, { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

/** Create a scheduled task to run omni-host.exe --service at user logon. */
export function enableAutoStart(): boolean {
  const hostPath = path.join(path.dirname(app.getPath('exe')), 'omni-host.exe');
  try {
    execSync(
      `schtasks /create /tn "${TASK_NAME}" /tr "\\"${hostPath}\\" --service" /sc ONLOGON /rl LIMITED /f`,
      { stdio: 'pipe' }
    );
    return true;
  } catch {
    return false;
  }
}

/** Remove the auto-start scheduled task. */
export function disableAutoStart(): boolean {
  try {
    execSync(`schtasks /delete /tn "${TASK_NAME}" /f`, { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/main/auto-start.ts
git commit -m "feat(desktop): auto-start module — scheduled task create/delete/query"
```

---

## Task 6: Tray icon with proper icon asset

**Files:**
- Create: `desktop/resources/icon.png`
- Modify: `desktop/main/background.ts`

- [ ] **Step 1: Create a placeholder tray icon**

Generate a simple 32x32 PNG icon (solid purple square with rounded corners matching the Omni brand color `#6366f1`). Save it as `desktop/resources/icon.png`.

If programmatic generation is difficult, create a 1x1 pixel PNG as a temporary placeholder — the icon can be replaced with a proper design later.

- [ ] **Step 2: Update background.ts to load the icon**

In the `createTray()` function in `desktop/main/background.ts`, update the icon loading:

```typescript
function createTray(): void {
  const iconPath = path.join(__dirname, '../resources/icon.png');
  let icon: nativeImage;
  try {
    icon = nativeImage.createFromPath(iconPath);
    // Resize for tray (16x16 on Windows)
    icon = icon.resize({ width: 16, height: 16 });
  } catch {
    icon = nativeImage.createEmpty();
  }
  tray = new Tray(icon);
  // ... rest unchanged
}
```

- [ ] **Step 3: Test tray icon**

```bash
cd desktop && npm run dev
```

Expected: Tray icon appears in system tray. Right-click shows "Open Omni" and "Quit". Closing the window hides to tray. "Quit" exits the app.

- [ ] **Step 4: Commit**

```bash
git add desktop/resources/ desktop/main/background.ts
git commit -m "feat(desktop): tray icon with context menu (Open/Quit)"
```

---

## Task 7: Update host status API to include overlay and game info

**Files:**
- Modify: `host/src/ws_server.rs`

- [ ] **Step 1: Enhance the status response**

In `host/src/ws_server.rs`, the `status` message handler currently returns only `ws_port` and `running`. Add `active_overlay` and `injected_game` fields.

This requires the `WsSharedState` to have access to the active overlay name and last injected exe. Add these fields:

In the `WsSharedState` struct, add:
```rust
pub active_overlay: Mutex<String>,
pub injected_game: Mutex<Option<String>>,
```

Update `WsSharedState::new()` to initialize them:
```rust
active_overlay: Mutex::new("Default".to_string()),
injected_game: Mutex::new(None),
```

Update the `status` handler:
```rust
"status" => {
    let active_overlay = state.active_overlay.lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let injected_game = state.injected_game.lock()
        .ok()
        .and_then(|s| s.clone());
    Some(json!({
        "type": "status.data",
        "ws_port": WS_PORT,
        "running": true,
        "active_overlay": active_overlay,
        "injected_game": injected_game,
    }).to_string())
},
```

- [ ] **Step 2: Update main.rs to write overlay/game info to WsSharedState**

In `host/src/main.rs`, in the main loop, after overlay switches and scanner polls, update the shared state:

After `scanner_instance.poll()`:
```rust
if let Ok(mut game) = ws_state.injected_game.lock() {
    *game = scanner_instance.last_injected_exe().map(|s| s.to_string());
}
```

After overlay switches (wherever `host.current_overlay` changes):
```rust
if let Ok(mut overlay) = ws_state.active_overlay.lock() {
    *overlay = host.current_overlay.clone();
}
```

- [ ] **Step 3: Update tests**

Update the `WsSharedState::new()` calls in existing tests to include the new fields.

Update the `handle_status` test to check for the new fields:
```rust
assert!(resp.get("active_overlay").is_some());
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add host/src/ws_server.rs host/src/main.rs
git commit -m "feat(host): status API returns active_overlay and injected_game"
```

---

## Task 8: NSIS installer script

**Files:**
- Create: `installer/installer.nsi`
- Create: `installer/license.txt`

- [ ] **Step 1: Create license.txt**

Create `installer/license.txt`:
```
MIT License

Copyright (c) 2026 Omni Overlay

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Create installer.nsi**

Create `installer/installer.nsi`:

```nsis
!include "MUI2.nsh"

; Product info
Name "Omni Overlay"
OutFile "..\dist\OmniSetup.exe"
InstallDir "$PROGRAMFILES64\Omni"
InstallDirRegKey HKCU "Software\OmniOverlay" "InstallDir"
RequestExecutionLevel admin

; MUI Settings
!define MUI_ICON "omni-icon.ico"
!define MUI_ABORTWARNING

; Pages
!insertmacro MUI_PAGE_LICENSE "license.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\Omni.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Omni"
!insertmacro MUI_PAGE_FINISH

; Uninstaller pages
!insertmacro MUI_UNPAGE_CONFIRM
UninstPage custom un.DataCleanupPage un.DataCleanupPageLeave
!insertmacro MUI_UNPAGE_INSTFILES

; Language
!insertmacro MUI_LANGUAGE "English"

; Variables
Var RemoveUserData

; Installer section
Section "Install"
  SetOutPath "$INSTDIR"

  ; Stop any running instances first
  nsExec::ExecToLog '"$INSTDIR\omni-host.exe" --stop'

  ; Electron app files
  File /r "..\desktop\dist\win-unpacked\*.*"

  ; Rust binaries
  File "..\target\release\omni-host.exe"
  SetOutPath "$INSTDIR\overlay"
  File "..\target\release\omni_overlay.dll"
  SetOutPath "$INSTDIR"

  ; Start Menu shortcut
  CreateDirectory "$SMPROGRAMS\Omni"
  CreateShortcut "$SMPROGRAMS\Omni\Omni.lnk" "$INSTDIR\Omni.exe"

  ; Write uninstaller
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Registry for Add/Remove Programs
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay" \
    "DisplayName" "Omni Overlay"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay" \
    "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay" \
    "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\OmniOverlay" "InstallDir" "$INSTDIR"
SectionEnd

; Uninstaller data cleanup page
Function un.DataCleanupPage
  nsDialogs::Create 1018
  Pop $0

  ${NSD_CreateCheckBox} 0 0 100% 12u "Also remove all user data (overlays, themes, configuration)"
  Pop $1
  ${NSD_SetState} $1 ${BST_UNCHECKED}

  nsDialogs::Show
FunctionEnd

Function un.DataCleanupPageLeave
  ${NSD_GetState} $1 $RemoveUserData
FunctionEnd

; Uninstaller section
Section "Uninstall"
  ; Stop running instances
  nsExec::ExecToLog '"$INSTDIR\omni-host.exe" --stop'

  ; Remove scheduled task if it exists
  nsExec::ExecToLog 'schtasks /delete /tn "OmniOverlay" /f'

  ; Remove files
  RMDir /r "$INSTDIR"

  ; Remove shortcuts
  RMDir /r "$SMPROGRAMS\Omni"
  Delete "$DESKTOP\Omni.lnk"

  ; Remove registry entries
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay"
  DeleteRegKey HKCU "Software\OmniOverlay"

  ; Conditionally remove user data
  ${If} $RemoveUserData == ${BST_CHECKED}
    RMDir /r "$APPDATA\Omni"
  ${EndIf}
SectionEnd
```

- [ ] **Step 3: Create a placeholder icon**

Create or copy a `.ico` file as `installer/omni-icon.ico`. A simple 256x256 purple square works as placeholder.

- [ ] **Step 4: Commit**

```bash
git add installer/
git commit -m "feat(installer): NSIS script with Start Menu shortcut, uninstaller, opt-in data cleanup"
```

---

## Task 9: Build script

**Files:**
- Create: `build.ps1`

- [ ] **Step 1: Create build.ps1**

```powershell
# Omni full build pipeline
# Usage: .\build.ps1

$ErrorActionPreference = "Stop"

Write-Host "=== Step 1: Build Rust (release) ===" -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Rust build failed" }

Write-Host "=== Step 2: Generate TypeScript types (ts-rs) ===" -ForegroundColor Cyan
cargo ts-rs export --output-directory desktop/src/generated
if ($LASTEXITCODE -ne 0) { throw "TypeScript generation failed" }

Write-Host "=== Step 3: Build Nextron app ===" -ForegroundColor Cyan
Push-Location desktop
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "Nextron build failed" }
Pop-Location

Write-Host "=== Step 4: Package installer (NSIS) ===" -ForegroundColor Cyan
if (-not (Test-Path "dist")) { New-Item -ItemType Directory -Path "dist" }
Push-Location installer
makensis installer.nsi
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "NSIS packaging failed" }
Pop-Location

Write-Host ""
Write-Host "=== Build complete ===" -ForegroundColor Green
Write-Host "Installer: dist\OmniSetup.exe"
```

- [ ] **Step 2: Commit**

```bash
git add build.ps1
git commit -m "feat: add build.ps1 — full pipeline (Rust + ts-rs + Nextron + NSIS)"
```

---

## Task 10: Update design spec with Phase 13 reorganization

**Files:**
- Modify: `docs/superpowers/specs/2026-03-27-omni-overlay-design.md`

- [ ] **Step 1: Update the Build Phases section**

In the design spec, update the "Upcoming Phases" section:

Move the following from Phase 11 to a new Phase 13:
- Visual widget editor with drag/drop
- Monaco editor with custom IntelliSense for `.omni` format
- Live HTML/CSS preview (browser-native WYSIWYG)
- Parser errors piped to Monaco squiggles
- Built-in themes (dark, cyberpunk, retro)

Update Phase 11 to reflect what we're actually building:
```
#### Phase 11a: Electron App Shell ✅ (in progress)
- Nextron (Next.js + Electron) desktop app
- Host process lifecycle management (spawn/connect)
- System tray with minimize-to-tray behavior
- Optional auto-start via scheduled task
- WebSocket connection with ts-rs generated types

#### Phase 11e: Installer + Distribution ✅ (in progress)
- NSIS Windows installer
- Start Menu shortcut, Add/Remove Programs
- Uninstaller with optional user data cleanup
- build.ps1 orchestration script
```

Add Phase 13:
```
#### Phase 13: Editor + Advanced UI
- Monaco code editor with .omni IntelliSense
- Live HTML/CSS preview
- Visual drag/drop widget editor
- Built-in themes (dark, cyberpunk, retro)
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-03-27-omni-overlay-design.md
git commit -m "docs: reorganize phases — Phase 11 is shell+installer, Phase 13 is editor UI"
```

---

## Final Verification

### Task 11: End-to-end smoke test

- [ ] **Step 1: Build Rust**
```bash
cargo build --release
```

- [ ] **Step 2: Generate types**
```bash
cargo ts-rs export --output-directory desktop/src/generated
```

- [ ] **Step 3: Build Nextron**
```bash
cd desktop && npm run build
```

- [ ] **Step 4: Run Rust tests**
```bash
cargo test --workspace
```

- [ ] **Step 5: Run dev mode end-to-end**
Start the host: `cargo run -p omni-host -- --service`
In another terminal: `cd desktop && npm run dev`
Expected: Electron window shows green dot, "Connected to host", active overlay name.

- [ ] **Step 6: Verify tray behavior**
Close the window → app stays in tray. Click tray → window reopens. Right-click "Quit" → app exits.

- [ ] **Step 7: Run cargo fmt and clippy**
```bash
cargo fmt --all
cargo clippy --workspace --all-targets
```

- [ ] **Step 8: Final commit if needed**
```bash
git add -A
git commit -m "cleanup: fmt + final verification"
```
