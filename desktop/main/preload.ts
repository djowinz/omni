import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("omni", {
  // Window controls
  minimizeWindow: () => ipcRenderer.send("window-minimize"),
  maximizeWindow: () => ipcRenderer.send("window-maximize"),
  closeWindow: () => ipcRenderer.send("window-close"),

  // Host status
  onHostStatus: (callback: (status: any) => void) => {
    const handler = (_event: any, status: any) => callback(status);
    ipcRenderer.on("host-status", handler);
    return () => { ipcRenderer.removeListener("host-status", handler); };
  },

  // Backend communication
  sendMessage: (msg: object) => ipcRenderer.invoke("ws-message", msg),

  // Sensor data stream
  onSensorData: (callback: (snapshot: any) => void) => {
    const handler = (_event: any, snapshot: any) => callback(snapshot);
    ipcRenderer.on("sensor-data", handler);
    return () => { ipcRenderer.removeListener("sensor-data", handler); };
  },
});
