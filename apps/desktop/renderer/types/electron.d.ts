/** Type declarations for the Electron IPC bridge exposed via preload.ts */

interface OmniIpcBridge {
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  onHostStatus: (callback: (status: any) => void) => () => void;
  sendMessage: (msg: object) => Promise<any>;
  onSensorData: (callback: (snapshot: any) => void) => () => void;
  onHwInfoSensors: (callback: (data: any) => void) => () => void;
  onUpdateReady: (callback: (version: string, releaseDate: string) => void) => () => void;
  installUpdate: () => void;
  restartHost: () => Promise<{ success: boolean }>;
  getLoginItemSettings: () => Promise<{ openAtLogin: boolean }>;
  setLoginItemSettings: (openAtLogin: boolean) => Promise<{ success: boolean }>;
}

declare global {
  interface Window {
    omni?: OmniIpcBridge;
  }
}

export {};
