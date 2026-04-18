import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('omni', {
  // Window controls
  minimizeWindow: () => ipcRenderer.send('window-minimize'),
  maximizeWindow: () => ipcRenderer.send('window-maximize'),
  closeWindow: () => ipcRenderer.send('window-close'),

  // Host status
  onHostStatus: (callback: (status: any) => void) => {
    const handler = (_event: any, status: any) => callback(status);
    ipcRenderer.on('host-status', handler);
    return () => {
      ipcRenderer.removeListener('host-status', handler);
    };
  },

  // Backend communication
  sendMessage: (msg: object) => ipcRenderer.invoke('ws-message', msg),

  // Share-hub request-response bridge. Returns the raw host frame (success OR
  // D-004-J error envelope). Renderer's useShareWs.send() Zod-parses both.
  // Callers MUST include `id: string` for id-based correlation.
  sendShareMessage: (msg: { id: string; [k: string]: unknown }) =>
    ipcRenderer.invoke('share:ws-message', msg),

  // Sensor data stream
  onSensorData: (callback: (snapshot: any) => void) => {
    const handler = (_event: any, snapshot: any) => callback(snapshot);
    ipcRenderer.on('sensor-data', handler);
    return () => {
      ipcRenderer.removeListener('sensor-data', handler);
    };
  },

  // Preview HTML stream
  onPreviewHtml: (callback: (data: { html: string; css: string }) => void) => {
    const handler = (_event: any, data: any) => callback(data);
    ipcRenderer.on('preview-html', handler);
    return () => {
      ipcRenderer.removeListener('preview-html', handler);
    };
  },

  // Preview incremental updates
  onPreviewUpdate: (
    callback: (data: { diff: Record<string, { c?: string; t?: string }> }) => void,
  ) => {
    const handler = (_event: any, data: any) => callback(data);
    ipcRenderer.on('preview-update', handler);
    return () => {
      ipcRenderer.removeListener('preview-update', handler);
    };
  },

  // HWiNFO sensor list updates
  onHwInfoSensors: (callback: (data: any) => void) => {
    const handler = (_event: any, data: any) => callback(data);
    ipcRenderer.on('hwinfo-sensors', handler);
    return () => {
      ipcRenderer.removeListener('hwinfo-sensors', handler);
    };
  },

  // Auto-update
  onUpdateReady: (callback: (version: string, releaseDate: string) => void) => {
    const handler = (_event: any, version: string, releaseDate: string) =>
      callback(version, releaseDate);
    ipcRenderer.on('update-ready', handler);
    return () => {
      ipcRenderer.removeListener('update-ready', handler);
    };
  },
  installUpdate: () => ipcRenderer.send('install-update'),

  // Settings
  restartHost: () => ipcRenderer.invoke('restart-host'),
  getLoginItemSettings: () => ipcRenderer.invoke('get-login-item-settings'),
  setLoginItemSettings: (openAtLogin: boolean) =>
    ipcRenderer.invoke('set-login-item-settings', openAtLogin),

  // Share Hub streaming events (unsolicited progress + preview-result frames).
  // See SHARE_EVENT_TYPES in main.ts and useShareWs.subscribe() in renderer/hooks/use-share-ws.ts.
  onShareEvent: (cb: (frame: unknown) => void) => {
    const handler = (_event: Electron.IpcRendererEvent, frame: unknown) => cb(frame);
    ipcRenderer.on('share:event', handler);
    return () => {
      ipcRenderer.removeListener('share:event', handler);
    };
  },

  // Log tailing
  startLogTail: () => ipcRenderer.invoke('log:start'),
  stopLogTail: () => ipcRenderer.invoke('log:stop'),
  onLogData: (callback: (lines: string[], fileSize: number) => void) => {
    const handler = (_event: any, lines: string[], fileSize: number) => callback(lines, fileSize);
    ipcRenderer.on('log:data', handler);
    return () => {
      ipcRenderer.removeListener('log:data', handler);
    };
  },
  onLogError: (callback: (message: string) => void) => {
    const handler = (_event: any, message: string) => callback(message);
    ipcRenderer.on('log:error', handler);
    return () => {
      ipcRenderer.removeListener('log:error', handler);
    };
  },
});
