import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, protocol, dialog, net } from 'electron';
import { autoUpdater } from 'electron-updater';
import * as path from 'path';
import { pathToFileURL } from 'url';
import * as fs from 'fs';
import { HostManager } from './host-manager';
import { LogTailer } from './log-tailer';
import { isAutoStartEnabled, enableAutoStart, disableAutoStart } from './auto-start';

const isProd = process.env.NODE_ENV === 'production';

// Enforce single instance — if another instance is already running,
// focus it and exit this one.
const gotTheLock = app.requestSingleInstanceLock();
if (!gotTheLock) {
  app.quit();
}

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let isQuitting = false;
const hostManager = new HostManager();

app.on('second-instance', () => {
  if (mainWindow) {
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.show();
    mainWindow.focus();
  }
});

function createWindow(): BrowserWindow {
  const preloadPath = path.join(__dirname, 'preload.js');

  const win = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    title: 'Omni',
    frame: false,
    webPreferences: {
      preload: preloadPath,
      contextIsolation: true,
      nodeIntegration: false,
    },
    show: false,
    icon: isProd
      ? path.join(process.resourcesPath, 'omni-logo.ico')
      : path.join(__dirname, '../resources/omni-logo.ico'),
  });

  // Show window on ready — unless minimize_to_tray is enabled
  win.once('ready-to-show', () => {
    let shouldMinimize = false;
    try {
      const configPath = path.join(app.getPath('userData'), 'config.json');
      const raw = fs.readFileSync(configPath, 'utf-8');
      const cfg = JSON.parse(raw);
      if (cfg.minimize_to_tray) {
        shouldMinimize = true;
      }
    } catch {
      // Config not available — show normally
    }
    if (!shouldMinimize) {
      win.show();
      win.focus();
    }
  });

  // Minimize to tray on close
  win.on('close', (e) => {
    if (!isQuitting) {
      e.preventDefault();
      win.hide();
    }
  });

  return win;
}

function createTray(): void {
  // Try to load icon, fall back to empty
  const iconPath = isProd
    ? path.join(process.resourcesPath, 'omni-logo.png')
    : path.join(__dirname, '../resources/omni-logo.png');
  let icon: Electron.NativeImage;
  try {
    icon = nativeImage.createFromPath(iconPath);
    if (!icon.isEmpty()) {
      icon = icon.resize({ width: 16, height: 16 });
    } else {
      icon = nativeImage.createEmpty();
    }
  } catch {
    icon = nativeImage.createEmpty();
  }

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
        isQuitting = true;
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

// Window control IPC handlers
ipcMain.on('window-minimize', () => mainWindow?.minimize());
ipcMain.on('window-maximize', () => {
  if (mainWindow?.isMaximized()) {
    mainWindow.unmaximize();
  } else {
    mainWindow?.maximize();
  }
});
ipcMain.on('window-close', () => mainWindow?.close());

// IPC: renderer requests install
ipcMain.on('install-update', () => {
  autoUpdater.quitAndInstall(true, true);
});

// Settings: restart the omni-host service
ipcMain.handle('restart-host', async () => {
  await hostManager.restart();
  return { success: true };
});

// Settings: get / set "Start with Windows" toggle.
// Backed by a Windows scheduled task (see ./auto-start.ts) — the prior
// `app.setLoginItemSettings` approach reported the toggle as enabled while
// Windows ran nothing at logon, because the `Run` registry value was tied to
// `process.execPath` (dev electron.exe in dev mode; potentially stale after
// Squirrel/electron-builder updates).
ipcMain.handle('get-login-item-settings', () => {
  return { openAtLogin: isAutoStartEnabled() };
});

ipcMain.handle('set-login-item-settings', (_event, openAtLogin: boolean) => {
  const success = openAtLogin ? enableAutoStart() : disableAutoStart();
  return { success };
});

// Identity backup: show a native save dialog and write the encrypted bytes
// to the chosen path atomically. Called by the IdentityBackupDialog's
// defaultSaveBackup() after the host returns an identity.backupResult with
// the argon2+XChaCha20Poly1305 ciphertext. Single IPC round-trip so the
// renderer doesn't need to juggle a separate fs:writeFile call.
//
// Returns:
//   - absolute path (string) if the user chose a location and write succeeded
//   - undefined if the user cancelled the dialog
// Throws:
//   - fs write errors bubble up to the renderer as IPC errors
ipcMain.handle(
  'identity:save-backup',
  async (_event, bytes: Uint8Array): Promise<string | undefined> => {
    if (!(bytes instanceof Uint8Array) || bytes.byteLength === 0) {
      throw new Error('identity:save-backup requires non-empty Uint8Array');
    }
    const result = await dialog.showSaveDialog({
      title: 'Save identity backup',
      defaultPath: 'omni-identity.omniid',
      filters: [
        { name: 'Omni identity backup', extensions: ['omniid'] },
        { name: 'All files', extensions: ['*'] },
      ],
    });
    if (result.canceled || !result.filePath) return undefined;
    await fs.promises.writeFile(result.filePath, Buffer.from(bytes));
    return result.filePath;
  },
);

// Unsolicited share-hub streaming frames forwarded to the renderer via the
// 'share:event' ipc channel for useShareWs.subscribe() consumers. Keep this
// list to REAL streaming types only — response frames (e.g. previewResult) are
// delivered via the 'share:ws-message' request path, and fork-progress isn't
// emitted by the host yet (#016 adds it alongside fork-from-Discover).
const SHARE_EVENT_TYPES = new Set<string>([
  'explorer.installProgress',
  'upload.publishProgress',
  'upload.packProgress',
]);

// Share-hub request-response IPC. Separate from the generic 'ws-message'
// channel because share messages:
//   - use id-based correlation (concurrent requests; no queue serialization)
//   - need their D-004-J error envelope preserved verbatim (useShareWs.send()
//     Zod-parses the raw frame, so the renderer sees `{ code, kind, detail,
//     message }` instead of a flattened Error).
// Renderer hook: apps/desktop/renderer/hooks/use-share-ws.ts.
ipcMain.handle('share:ws-message', async (_event, msg: any) => {
  if (!msg || typeof msg !== 'object' || typeof msg.id !== 'string') {
    throw new Error('share:ws-message requires { id: string, type: string, ... }');
  }
  if (!hostManager.isConnected()) {
    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        hostManager.removeListener('connected', onConnect);
        reject(new Error('Timed out waiting for host connection'));
      }, 10000);
      const onConnect = () => {
        clearTimeout(timeout);
        resolve();
      };
      hostManager.once('connected', onConnect);
    });
  }
  return hostManager.sendAndWaitById(msg);
});

// IPC: request/response messages to host via WebSocket.
// Waits up to 10s for the host connection before rejecting.
ipcMain.handle('ws-message', async (_event, msg: any) => {
  const responseTypes: Record<string, string> = {
    status: 'status.data',
    'sensors.subscribe': 'sensors.subscribed',
    'file.list': 'file.list',
    'file.read': 'file.content',
    'file.write': 'file.written',
    'file.create': 'file.created',
    'file.delete': 'file.deleted',
    'widget.parse': 'widget.parsed',
    'widget.apply': 'widget.applied',
    'widget.update': 'widget.updated',
    'config.get': 'config.data',
    'config.update': 'config.updated',
    'log.path': 'log.path',
    'preview.subscribe': 'preview.subscribed',
  };
  const expectedType = responseTypes[msg.type];
  if (!expectedType) {
    throw new Error(`Unknown message type: ${msg.type}`);
  }

  // Wait for host connection if not yet ready
  if (!hostManager.isConnected()) {
    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        hostManager.removeListener('connected', onConnect);
        reject(new Error('Timed out waiting for host connection'));
      }, 10000);
      const onConnect = () => {
        clearTimeout(timeout);
        resolve();
      };
      hostManager.once('connected', onConnect);
    });
  }

  return hostManager.sendAndWait(msg, expectedType);
});

// Register omni:// protocol for serving local resources, plus
// omni-preview:// for save-time preview thumbnails read from the host's
// data_dir (`%APPDATA%/Omni`). The renderer can't load `file://` URLs
// directly under `webSecurity: true`, so the upload dialog routes preview
// reads through this scheme — see `omni-preview` handler below for the
// path-traversal guard and URL → on-disk-path mapping.
protocol.registerSchemesAsPrivileged([
  {
    scheme: 'omni',
    privileges: { standard: true, secure: true, supportFetchAPI: true },
  },
  {
    scheme: 'omni-preview',
    privileges: { secure: true, supportFetchAPI: true, bypassCSP: true },
  },
]);

app.on('ready', async () => {
  // Handle omni:// protocol for serving local files.
  // In production, this serves the entire app (Next.js static export + resources).
  // URL formats:
  //   omni://app/home/index.html  → serves from the app directory
  //   omni://app/_next/static/... → serves Next.js static assets
  //   omni://resources/logo.png   → serves from resources directory
  protocol.handle('omni', (request) => {
    const url = new URL(request.url);
    const fullPath = decodeURIComponent(url.hostname + url.pathname);

    let filePath: string;
    if (isProd) {
      // Production layout:
      //   C:\Program Files\Omni\              ← install dir
      //   C:\Program Files\Omni\resources\    ← Electron resources (app.asar, icons, fonts)
      //   C:\Program Files\Omni\resources\app.asar\app\  ← Next.js export inside asar
      //
      // omni://app/home/index.html  → asar/app/home/index.html
      // omni://resources/logo.png   → install_dir/resources/logo.png
      const installDir = path.join(path.dirname(app.getAppPath()), '..');
      const asarPath = path.join(app.getAppPath(), fullPath);
      if (fs.existsSync(asarPath)) {
        filePath = asarPath;
      } else {
        filePath = path.join(installDir, fullPath);
      }
    } else {
      // Dev: resolve relative to desktop/ root
      filePath = path.resolve(path.join(__dirname, '..'), fullPath);
    }

    if (!fs.existsSync(filePath)) {
      return new Response('Not Found', { status: 404 });
    }

    // Read the file content directly — net.fetch with file:// doesn't
    // work for asar paths. Reading via fs handles asar transparently.
    try {
      const data = fs.readFileSync(filePath);
      const ext = path.extname(filePath).toLowerCase();
      const mimeTypes: Record<string, string> = {
        '.html': 'text/html',
        '.css': 'text/css',
        '.js': 'application/javascript',
        '.json': 'application/json',
        '.png': 'image/png',
        '.jpg': 'image/jpeg',
        '.ico': 'image/x-icon',
        '.svg': 'image/svg+xml',
        '.ttf': 'font/ttf',
        '.woff': 'font/woff',
        '.woff2': 'font/woff2',
        '.map': 'application/json',
      };
      const contentType = mimeTypes[ext] || 'application/octet-stream';
      return new Response(data, {
        headers: { 'Content-Type': contentType },
      });
    } catch {
      return new Response('Internal Error', { status: 500 });
    }
  });

  // omni-preview:// — serves save-time preview thumbnails from the host's
  // data_dir (the Rust host's `config::data_dir()` is `%APPDATA%/Omni`,
  // which matches Electron's `app.getPath('userData')` because the
  // electron-builder productName is "Omni"). Renderer call sites:
  //   - apps/desktop/renderer/components/omni/upload-dialog/steps/source-picker-list-row.tsx
  //   - apps/desktop/renderer/components/omni/upload-dialog/steps/review.tsx
  //
  // URL shape:
  //   omni-preview://overlays/<name>/.omni-preview.png
  //   omni-preview://themes/<name>.preview.png
  //
  // url.host       → 'overlays' | 'themes'
  // url.pathname   → '/<rest of path>' (URL-decoded before joining)
  //
  // Security: the only allowed segment hosts are 'overlays' and 'themes';
  // the resolved on-disk path must remain inside `<dataDir>/<segment>` or
  // we return 403 (defends against `..` traversal in pathname).
  const dataDir = app.getPath('userData');
  protocol.handle('omni-preview', async (request) => {
    try {
      const url = new URL(request.url);
      const segment = url.host;
      if (segment !== 'overlays' && segment !== 'themes') {
        return new Response('Bad Request', { status: 400 });
      }
      const tail = decodeURIComponent(url.pathname);
      const safeRoot = path.normalize(path.join(dataDir, segment));
      const target = path.normalize(path.join(safeRoot, tail));
      // Path-traversal guard: target must remain inside safeRoot.
      if (target !== safeRoot && !target.startsWith(safeRoot + path.sep)) {
        return new Response('Forbidden', { status: 403 });
      }
      try {
        await fs.promises.access(target, fs.constants.R_OK);
      } catch {
        return new Response('Not Found', { status: 404 });
      }
      return net.fetch(pathToFileURL(target).toString());
    } catch {
      return new Response('Internal Error', { status: 500 });
    }
  });

  mainWindow = createWindow();
  createTray();

  if (isProd) {
    // Production: load via omni:// protocol so absolute paths (/_next/...) resolve correctly
    await mainWindow.loadURL('omni://app/home/index.html');
  } else {
    // Development: load from Next.js dev server
    const port = process.argv[2] || '8888';
    await mainWindow.loadURL(`http://localhost:${port}/home`);
    mainWindow.webContents.openDevTools();
  }

  // minimize_to_tray is now handled in the ready-to-show callback above

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

  // Forward sensor data stream to renderer
  hostManager.on('message', (msg: any) => {
    if (msg.type === 'sensors.data') {
      mainWindow?.webContents.send('sensor-data', {
        snapshot: msg.snapshot,
        hwinfo: msg.snapshot?.hwinfo,
      });
    }
    if (msg.type === 'hwinfo.sensors') {
      mainWindow?.webContents.send('hwinfo-sensors', msg);
    }
    if (msg.type === 'preview.html') {
      mainWindow?.webContents.send('preview-html', { html: msg.html, css: msg.css });
    }
    if (msg.type === 'preview.update') {
      mainWindow?.webContents.send('preview-update', { diff: msg.diff });
    }
    // Forward unsolicited share-hub frames to the renderer's 'share:event' channel.
    // Silently drops if mainWindow is null (window closed). Does not interfere with
    // the ws-message invoke path above — these type strings don't overlap with responseTypes.
    if (typeof msg.type === 'string' && SHARE_EVENT_TYPES.has(msg.type)) {
      mainWindow?.webContents.send('share:event', msg);
    }
  });

  // Log tailing
  let logTailer: LogTailer | null = null;

  try {
    ipcMain.removeHandler('log:start');
  } catch {}
  try {
    ipcMain.removeHandler('log:stop');
  } catch {}

  ipcMain.handle('log:start', async () => {
    // Stop any existing tailer
    if (logTailer) {
      logTailer.stop();
      logTailer = null;
    }

    // Get log path from host
    let logPath: string;
    try {
      const response = await hostManager.sendAndWait({ type: 'log.path' }, 'log.path');
      logPath = response.path;
    } catch {
      throw new Error('Failed to get log path from host');
    }

    logTailer = new LogTailer(logPath);

    logTailer.on('lines', (lines: string[], fileSize: number) => {
      mainWindow?.webContents.send('log:data', lines, fileSize);
    });

    logTailer.on('error', (err: Error) => {
      mainWindow?.webContents.send('log:error', err.message);
    });

    logTailer.start();
    return { path: logPath };
  });

  ipcMain.handle('log:stop', () => {
    if (logTailer) {
      logTailer.stop();
      logTailer = null;
    }
  });

  // --- Auto-updater (production only) ---
  if (!isProd) {
    autoUpdater.autoDownload = false;
    autoUpdater.autoInstallOnAppQuit = false;
  } else {
    autoUpdater.autoDownload = true;
    autoUpdater.autoInstallOnAppQuit = true;
  }

  autoUpdater.on('update-downloaded', (info) => {
    mainWindow?.webContents.send('update-ready', info.version, info.releaseDate);
  });

  autoUpdater.on('error', (err) => {
    console.error('[auto-updater] Error:', err.message);
  });

  if (isProd) {
    // Check on startup (delay 10s to let the app finish loading)
    setTimeout(() => {
      autoUpdater.checkForUpdates().catch(() => {});
    }, 10_000);

    // Check every 4 hours
    setInterval(
      () => {
        autoUpdater.checkForUpdates().catch(() => {});
      },
      4 * 60 * 60 * 1000,
    );
  }
});

app.on('before-quit', async () => {
  await hostManager.shutdown();
});

app.on('window-all-closed', () => {
  // Don't quit — tray keeps us alive
});
