/** Type declarations for the Electron IPC bridge exposed via preload.ts */

interface OmniIpcBridge {
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  onHostStatus: (callback: (status: any) => void) => () => void;
  sendMessage: (msg: object) => Promise<any>;
  sendShareMessage: (msg: { id: string; [k: string]: unknown }) => Promise<any>;
  onSensorData: (callback: (snapshot: any) => void) => () => void;
  onPreviewHtml: (callback: (data: { html: string; css: string }) => void) => () => void;
  onPreviewUpdate: (
    callback: (data: { diff: Record<string, { c?: string; t?: string }> }) => void,
  ) => () => void;
  onHwInfoSensors: (callback: (data: any) => void) => () => void;
  onUpdateReady: (callback: (version: string, releaseDate: string) => void) => () => void;
  installUpdate: () => void;
  restartHost: () => Promise<{ success: boolean }>;
  getLoginItemSettings: () => Promise<{ openAtLogin: boolean }>;
  setLoginItemSettings: (openAtLogin: boolean) => Promise<{ success: boolean }>;
  startLogTail: () => Promise<{ path: string }>;
  stopLogTail: () => Promise<void>;
  onLogData: (callback: (lines: string[], fileSize: number) => void) => () => void;
  onLogError: (callback: (message: string) => void) => () => void;
  onShareEvent: (cb: (frame: unknown) => void) => () => void;
}

declare global {
  interface Window {
    omni?: OmniIpcBridge;
  }
}

export {};
