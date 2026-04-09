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

  // Sensor data stream
  onSensorData: (callback: (snapshot: any) => void) => {
    const handler = (_event: any, snapshot: any) => callback(snapshot);
    ipcRenderer.on('sensor-data', handler);
    return () => {
      ipcRenderer.removeListener('sensor-data', handler);
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

  // Log tailing
  startLogTail: () => ipcRenderer.invoke('log:start'),
  stopLogTail: () => ipcRenderer.invoke('log:stop'),
  onLogData: (callback: (lines: string[]) => void) => {
    const handler = (_event: any, lines: string[]) => callback(lines);
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
