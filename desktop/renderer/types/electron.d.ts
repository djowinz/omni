/** Type declarations for the Electron IPC bridge exposed via preload.ts */

interface OmniIpcBridge {
  onHostStatus: (callback: (status: any) => void) => void;
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  getResourcePath: (filename: string) => string;
}

declare global {
  interface Window {
    omni?: OmniIpcBridge;
  }
}

export {};
