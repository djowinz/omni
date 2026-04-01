import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain } from "electron";
import * as path from "path";
import { HostManager } from "./host-manager";

const isProd = process.env.NODE_ENV === 'production';

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let isQuitting = false;
const hostManager = new HostManager();

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
  });

  win.once('ready-to-show', () => win.show());

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
  const iconPath = path.join(__dirname, '../resources/icon.png');
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

app.on('ready', async () => {
  mainWindow = createWindow();
  createTray();

  if (isProd) {
    // Production: load the static Next.js export
    await mainWindow.loadFile(path.join(__dirname, '../app/home/index.html'));
  } else {
    // Development: load from Next.js dev server
    const port = process.argv[2] || '8888';
    await mainWindow.loadURL(`http://localhost:${port}/home`);
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
  // Don't quit — tray keeps us alive
});
