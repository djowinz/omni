import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('omni', {
  onHostStatus: (callback: (status: any) => void) => {
    ipcRenderer.on('host-status', (_event, status) => callback(status));
  },
  minimizeWindow: () => ipcRenderer.send('window-minimize'),
  maximizeWindow: () => ipcRenderer.send('window-maximize'),
  closeWindow: () => ipcRenderer.send('window-close'),
});
