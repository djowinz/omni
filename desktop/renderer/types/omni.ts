// Core Types for Omni Overlay System

import type { Config } from '@/src/generated/Config';

/**
 * Widget parsed from .omni file content
 * A single .omni file can contain multiple widgets
 */
export interface ParsedWidget {
  id: string;
  name: string;
  enabled: boolean;
  startLine: number;
  endLine: number;
}

/**
 * Theme import parsed from .omni file
 * Themes are external CSS files imported via <theme src="..." />
 */
export interface ThemeImport {
  src: string; // File path, e.g., "themes/neon.css"
  name: string; // Derived from filename, e.g., "neon"
  line: number; // Line number in the .omni file
}

/**
 * Represents an open tab in the editor
 */
export interface EditorTab {
  id: string;
  name: string;
  type: 'overlay' | 'theme';
  content: string;
  isDirty: boolean;
}

/**
 * An overlay is a folder on disk at %APPDATA%\Omni\overlays/{name}/overlay.omni
 * Identity is the folder name, not a synthetic ID.
 */
export interface Overlay {
  name: string;           // Folder name — this IS the identity
  content: string | null; // Raw .omni file content, null = not yet loaded (lazy)
}

/**
 * Simulated metric values for preview
 */
export interface MetricValues {
  fps: number;
  frametime: number;
  'frame.1pct': number;
  'gpu.usage': number;
  'gpu.temp': number;
  'gpu.clock': number;
  'gpu.vram.used': number;
  'gpu.vram.total': number;
  'gpu.power': number;
  'gpu.volt': number;
  'gpu.fan': number;
  'cpu.usage': number;
  'cpu.temp': number;
  'cpu.core': number[];
  'ram.usage': number;
}

/**
 * Global application state
 */
export interface AppState {
  // Overlay management
  overlays: Overlay[];
  config: Config | null;        // From backend — replaces activeOverlayId + gameAssignments
  connected: boolean;           // Host connection status

  // UI state
  selectedOverlayName: string;  // Currently being edited
  selectedWidgetId: string | null; // Widget selected in panel

  // Editor tabs
  openTabs: EditorTab[];
  activeTabId: string | null;

  // Theme files
  themeFiles: Record<string, string>; // path -> content

  // Preview simulation
  previewMetrics: MetricValues;

  // Editor state
  isDirty: boolean; // Unsaved changes
}

/**
 * Actions for state reducer
 */
export type AppAction =
  | { type: 'SET_OVERLAYS'; payload: Overlay[] }
  | { type: 'ADD_OVERLAY'; payload: Overlay }
  | { type: 'UPDATE_OVERLAY_CONTENT'; payload: { name: string; content: string } }
  | { type: 'DELETE_OVERLAY'; payload: string }
  | { type: 'SELECT_OVERLAY'; payload: string }
  | { type: 'SELECT_WIDGET'; payload: string | null }
  | { type: 'SET_CONFIG'; payload: Config | null }
  | { type: 'SET_CONNECTED'; payload: boolean }
  | { type: 'UPDATE_PREVIEW_METRIC'; payload: { key: string; value: number | number[] } }
  | { type: 'SET_DIRTY'; payload: boolean }
  | { type: 'OPEN_TAB'; payload: EditorTab }
  | { type: 'CLOSE_TAB'; payload: string }
  | { type: 'SET_ACTIVE_TAB'; payload: string | null }
  | { type: 'UPDATE_TAB_CONTENT'; payload: { id: string; content: string } }
  | { type: 'SET_THEME_FILE'; payload: { path: string; content: string } };

/**
 * Default metric values for preview simulation
 */
export const DEFAULT_METRICS: MetricValues = {
  fps: 144,
  frametime: 6.9,
  'frame.1pct': 120,
  'gpu.usage': 85,
  'gpu.temp': 72,
  'gpu.clock': 1950,
  'gpu.vram.used': 8192,
  'gpu.vram.total': 12288,
  'gpu.power': 280,
  'gpu.volt': 1050,
  'gpu.fan': 65,
  'cpu.usage': 45,
  'cpu.temp': 68,
  'cpu.core': [42, 55, 38, 51, 44, 48, 39, 52],
  'ram.usage': 62,
};
