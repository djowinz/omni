// Core Types for Omni Overlay System

import type { Config } from '@/generated/Config';
import type { EditorViewState } from '@/lib/persistence';

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
  name: string; // Folder name — this IS the identity
  content: string | null; // Raw .omni file content, null = not yet loaded (lazy)
}

/**
 * Simulated metric values for preview.
 * Keys match sensor paths in host/src/omni/sensor_map.rs.
 */
export interface MetricValues {
  fps: number;
  'frame-time': number;
  'frame-time.avg': number;
  'frame-time.1pct': number;
  'frame-time.01pct': number;
  'cpu.usage': number;
  'cpu.temp': number;
  'gpu.usage': number;
  'gpu.temp': number;
  'gpu.clock': number;
  'gpu.mem-clock': number;
  'gpu.vram.used': number;
  'gpu.vram.total': number;
  'gpu.power': number;
  'gpu.fan': number;
  'ram.usage': number;
  'ram.used': number;
  'ram.total': number;
}

/**
 * Global application state
 */
export interface AppState {
  // Overlay management
  overlays: Overlay[];
  config: Config | null; // From backend — replaces activeOverlayId + gameAssignments
  connected: boolean; // Host connection status

  // UI state
  selectedOverlayName: string; // Currently being edited
  selectedWidgetId: string | null; // Widget selected in panel
  widgetScrollRequest: number; // Increments on every widget click to trigger scroll

  // Editor tabs
  openTabs: EditorTab[];
  activeTabId: string | null;

  // Editor view states (cursor, scroll) per tab — persisted to IndexedDB
  editorViewStates: Record<string, EditorViewState>;

  // Theme files
  themeFiles: Record<string, string>; // path -> content

  // Editor state
  isDirty: boolean; // Unsaved changes

  // Sidebar panel selection
  activePanel: 'components' | 'settings' | 'explore';

  // Auto-update
  updateReady: boolean;
  updateVersion: string | null;
  updateReleaseDate: string | null;

  // HWiNFO integration
  hwinfoConnected: boolean;
  hwinfoSensorCount: number;
  hwinfoSensors: Array<{ path: string; label: string; unit: string }>;
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
  | { type: 'SET_DIRTY'; payload: boolean }
  | { type: 'OPEN_TAB'; payload: EditorTab }
  | { type: 'CLOSE_TAB'; payload: string }
  | { type: 'SET_ACTIVE_TAB'; payload: string | null }
  | { type: 'UPDATE_TAB_CONTENT'; payload: { id: string; content: string } }
  | { type: 'SET_THEME_FILE'; payload: { path: string; content: string } }
  | { type: 'SET_ACTIVE_PANEL'; payload: 'components' | 'settings' | 'explore' }
  | { type: 'SET_UPDATE_READY'; payload: { version: string; releaseDate: string } }
  | {
      type: 'SET_HWINFO_SENSORS';
      payload: {
        connected: boolean;
        sensors: Array<{ path: string; label: string; unit: string }>;
      };
    }
  | { type: 'SET_EDITOR_VIEW_STATE'; payload: { tabId: string; viewState: EditorViewState } };
