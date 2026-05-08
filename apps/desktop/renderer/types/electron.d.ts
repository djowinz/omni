/** Type declarations for the Electron IPC bridge exposed via preload.ts */

import type { PreviewDiff, PreviewValues } from '../lib/preview-updater';

interface OmniIpcBridge {
  minimizeWindow: () => void;
  maximizeWindow: () => void;
  closeWindow: () => void;
  onHostStatus: (callback: (status: any) => void) => () => void;
  sendMessage: (msg: object) => Promise<any>;
  sendShareMessage: (msg: { id: string; [k: string]: unknown }) => Promise<any>;
  onSensorData: (callback: (snapshot: any) => void) => () => void;
  /** In-game preview stream — emitted when the host builds initial HTML for the active overlay. */
  onPreviewHtmlIngame: (callback: (data: { html: string; css: string }) => void) => () => void;
  /** In-game preview stream — incremental sensor/diff updates for the active overlay. */
  onPreviewUpdateIngame: (
    callback: (data: { diff?: PreviewDiff; values?: PreviewValues }) => void,
  ) => () => void;
  /** Editor preview stream — emitted when the host builds initial HTML for the editor selection. */
  onPreviewHtmlEditor: (
    callback: (data: { html: string; css: string; overlay_name: string }) => void,
  ) => () => void;
  /** Editor preview stream — incremental sensor/diff updates for the editor selection. */
  onPreviewUpdateEditor: (
    callback: (data: { diff?: PreviewDiff; values?: PreviewValues }) => void,
  ) => () => void;
  onHwInfoSensors: (callback: (data: any) => void) => () => void;
  onUpdateReady: (callback: (version: string, releaseDate: string) => void) => () => void;
  installUpdate: () => void;
  restartHost: () => Promise<{ success: boolean }>;
  /**
   * Save an identity backup via a native save dialog. Returns the absolute
   * path the user chose, or undefined if they cancelled. Rejects on write
   * errors. Wired to main-process `identity:save-backup`.
   */
  saveIdentityBackup: (bytes: Uint8Array) => Promise<string | undefined>;
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
