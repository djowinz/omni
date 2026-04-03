import {
  app,
  BrowserWindow,
  Tray,
  Menu,
  nativeImage,
  ipcMain,
  protocol,
  net,
} from "electron";
import * as path from "path";
import * as fs from "fs";
import { HostManager } from "./host-manager";

const isProd = process.env.NODE_ENV === "production";

let mainWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let isQuitting = false;
const hostManager = new HostManager();

function createWindow(): BrowserWindow {
  const preloadPath = path.join(__dirname, "preload.js");

  const win = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    title: "Omni",
    frame: false,
    webPreferences: {
      preload: preloadPath,
      contextIsolation: true,
      nodeIntegration: false,
    },
    show: false,
    icon: path.join(__dirname, "../resources/omni-logo.png"),
  });

  win.once("ready-to-show", () => win.show());

  // Minimize to tray on close
  win.on("close", (e) => {
    if (!isQuitting) {
      e.preventDefault();
      win.hide();
    }
  });

  return win;
}

function createTray(): void {
  // Try to load icon, fall back to empty
  const iconPath = path.join(__dirname, "../resources/omni-logo.png");
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
  tray.setToolTip("Omni Overlay");

  const contextMenu = Menu.buildFromTemplate([
    {
      label: "Open Omni",
      click: () => {
        mainWindow?.show();
        mainWindow?.focus();
      },
    },
    { type: "separator" },
    {
      label: "Quit",
      click: () => {
        isQuitting = true;
        app.quit();
      },
    },
  ]);

  tray.setContextMenu(contextMenu);
  tray.on("click", () => {
    mainWindow?.show();
    mainWindow?.focus();
  });
}

// Window control IPC handlers
ipcMain.on("window-minimize", () => mainWindow?.minimize());
ipcMain.on("window-maximize", () => {
  if (mainWindow?.isMaximized()) {
    mainWindow.unmaximize();
  } else {
    mainWindow?.maximize();
  }
});
ipcMain.on("window-close", () => mainWindow?.close());

// IPC: request/response messages to host via WebSocket.
// Waits up to 10s for the host connection before rejecting.
ipcMain.handle("ws-message", async (_event, msg: any) => {
  const responseTypes: Record<string, string> = {
    status: "status.data",
    "sensors.subscribe": "sensors.subscribed",
    "file.list": "file.list",
    "file.read": "file.content",
    "file.write": "file.written",
    "file.create": "file.created",
    "file.delete": "file.deleted",
    "widget.parse": "widget.parsed",
    "widget.apply": "widget.applied",
    "widget.update": "widget.updated",
    "config.get": "config.data",
    "config.update": "config.updated",
  };
  const expectedType = responseTypes[msg.type];
  if (!expectedType) {
    throw new Error(`Unknown message type: ${msg.type}`);
  }

  // Wait for host connection if not yet ready
  if (!hostManager.isConnected()) {
    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        hostManager.removeListener("connected", onConnect);
        reject(new Error("Timed out waiting for host connection"));
      }, 10000);
      const onConnect = () => {
        clearTimeout(timeout);
        resolve();
      };
      hostManager.once("connected", onConnect);
    });
  }

  return hostManager.sendAndWait(msg, expectedType);
});

// Register omni:// protocol for serving local resources
protocol.registerSchemesAsPrivileged([
  {
    scheme: "omni",
    privileges: { standard: true, secure: true, supportFetchAPI: true },
  },
]);

app.on("ready", async () => {
  // Handle omni://resource/<filename> requests by serving from resources/
  // URL format: omni://resource/omni-logo.png
  //   hostname = "resource", pathname = "/omni-logo.png"
  protocol.handle("omni", (request) => {
    const url = new URL(request.url);
    const resourcesDir = isProd
      ? path.join(app.getAppPath(), "resources")
      : path.join(__dirname, "../resources");

    // Strip leading slash from pathname
    const filename = decodeURIComponent(url.pathname).replace(/^\/+/, "");
    const filePath = path.resolve(resourcesDir, filename);
    const fileUrl = `file:///${filePath.replace(/\\/g, "/")}`;

    // Debug logging in dev
    if (!isProd) {
      console.log("[omni://]", {
        requestUrl: request.url,
        hostname: url.hostname,
        pathname: url.pathname,
        filename,
        resourcesDir: path.resolve(resourcesDir),
        filePath,
        fileUrl,
        exists: fs.existsSync(filePath),
      });
    }

    // Prevent path traversal
    const resolvedResourcesDir = path.resolve(resourcesDir);
    if (!filePath.startsWith(resolvedResourcesDir)) {
      return new Response("Forbidden", { status: 403 });
    }

    if (!fs.existsSync(filePath)) {
      return new Response("Not Found", { status: 404 });
    }

    return net.fetch(fileUrl);
  });
  mainWindow = createWindow();
  createTray();

  if (isProd) {
    // Production: load the static Next.js export
    await mainWindow.loadFile(path.join(__dirname, "../app/home/index.html"));
  } else {
    // Development: load from Next.js dev server
    const port = process.argv[2] || "8888";
    await mainWindow.loadURL(`http://localhost:${port}/home`);
    mainWindow.webContents.openDevTools();
  }

  // Start host manager
  await hostManager.start();

  // Forward status to renderer via IPC
  hostManager.on("connected", () => {
    mainWindow?.webContents.send("host-status", hostManager.status);
  });
  hostManager.on("disconnected", () => {
    mainWindow?.webContents.send("host-status", { connected: false });
  });
  hostManager.on("status", (status) => {
    mainWindow?.webContents.send("host-status", status);
  });

  // Forward sensor data stream to renderer
  hostManager.on("message", (msg: any) => {
    if (msg.type === "sensors.data") {
      mainWindow?.webContents.send("sensor-data", msg.snapshot);
    }
  });
});

app.on("before-quit", async () => {
  await hostManager.shutdown();
});

app.on("window-all-closed", () => {
  // Don't quit — tray keeps us alive
});
