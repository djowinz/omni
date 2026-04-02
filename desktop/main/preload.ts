import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("omni", {
  // Window controls
  minimizeWindow: () => ipcRenderer.send("window-minimize"),
  maximizeWindow: () => ipcRenderer.send("window-maximize"),
  closeWindow: () => ipcRenderer.send("window-close"),

  // Host status
  onHostStatus: (callback: (status: any) => void) => {
    ipcRenderer.on("host-status", (_event, status) => callback(status));
  },

  // Backend communication
  sendMessage: (msg: object) => ipcRenderer.invoke("ws-message", msg),

  // Sensor data stream
  onSensorData: (callback: (snapshot: any) => void) => {
    ipcRenderer.on("sensor-data", (_event, snapshot) => callback(snapshot));
  },
});
