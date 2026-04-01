import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('omni', {
  onHostStatus: (callback: (status: any) => void) => {
    ipcRenderer.on('host-status', (_event, status) => callback(status));
  },
});
