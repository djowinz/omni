/** Type declarations for the Electron IPC bridge exposed via preload.ts */

interface OmniIpcBridge {
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  onHostStatus: (callback: (status: any) => void) => (() => void);
  sendMessage: (msg: object) => Promise<any>;
  onSensorData: (callback: (snapshot: any) => void) => (() => void);
}

declare global {
  interface Window {
    omni?: OmniIpcBridge;
  }
}

export {};
