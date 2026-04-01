// Core Types for Omni Overlay System

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
 * An overlay is a .omni file that can be assigned to games
 */
export interface Overlay {
  id: string;
  name: string;
  isDefault: boolean; // True = system default (installed with app, read-only)
  content: string; // Full .omni file content
  createdAt: string;
  updatedAt: string;
}

/**
 * Maps an overlay to a game executable
 * One overlay can map to many games
 * One game can only have one overlay
 */
export interface GameAssignment {
  executable: string; // e.g., "cyberpunk2077.exe"
  overlayId: string;
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
  activeOverlayId: string | null; // User's preferred overlay (second priority)

  // Game assignments (highest priority when matched)
  gameAssignments: GameAssignment[];

  // UI state
  selectedOverlayId: string; // Currently being edited
  selectedWidgetId: string | null; // Widget selected in panel

  // Editor tabs
  openTabs: EditorTab[];
  activeTabId: string | null;

  // Theme files (simulated for UI preview, actual file system in Electron)
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
  | { type: 'UPDATE_OVERLAY'; payload: { id: string; updates: Partial<Overlay> } }
  | { type: 'DELETE_OVERLAY'; payload: string }
  | { type: 'SET_ACTIVE_OVERLAY'; payload: string | null }
  | { type: 'SELECT_OVERLAY'; payload: string }
  | { type: 'SELECT_WIDGET'; payload: string | null }
  | { type: 'SET_GAME_ASSIGNMENTS'; payload: GameAssignment[] }
  | { type: 'ADD_GAME_ASSIGNMENT'; payload: GameAssignment }
  | { type: 'REMOVE_GAME_ASSIGNMENT'; payload: string }
  | { type: 'UPDATE_PREVIEW_METRIC'; payload: { key: string; value: number | number[] } }
  | { type: 'SET_DIRTY'; payload: boolean }
  | { type: 'UPDATE_OVERLAY_CONTENT'; payload: { id: string; content: string } }
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

/**
 * Sample overlay for development (simulates the installed default)
 */
export const SAMPLE_DEFAULT_OVERLAY: Overlay = {
  id: 'default',
  name: 'Default',
  isDefault: true,
  content: `<!-- Theme imports -->
<theme src="themes/neon.css" />

<widget id="system-stats" name="System Stats" enabled="true">
  <template>
    <div class="panel" style="position: absolute; top: 20px; left: 20px;">
      <span class="val">CPU: {cpu.usage}%</span>
      <span class="val">CPU Temp: {cpu.temp}°C</span>
      <span class="val">GPU: {gpu.usage}%</span>
      <span class="val">GPU Temp: {gpu.temp}°C</span>
      <span class="val">GPU Clock: {gpu.clock} MHz</span>
      <span class="val">VRAM: {gpu.vram.used}/{gpu.vram.total} MB</span>
      <span class="val">GPU Power: {gpu.power}W</span>
      <span class="val">GPU Fan: {gpu.fan}%</span>
      <span class="val">RAM: {ram.usage}%</span>
      <span class="val" class:critical="{fps} < 30">FPS: {fps}</span>
    </div>
  </template>
  <style>
    .panel {
      background: rgba(20, 20, 20, 0.7);
      border-radius: 4px;
      padding: 6px;
      display: flex;
      flex-direction: column;
      gap: 2px;
    }
    .val {
      color: #ffffff;
      font-size: 16px;
      font-weight: 400;
    }
    .val.critical {
      color: #EF4444;
    }
  </style>
</widget>`,
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

/**
 * Storage adapter interface for persistence abstraction
 * Defaults to localStorage, can be swapped for IPC in Electron
 */
export interface StorageAdapter {
  loadOverlays: () => Promise<Overlay[]>;
  saveOverlay: (overlay: Overlay) => Promise<void>;
  deleteOverlay: (id: string) => Promise<void>;
  loadGameAssignments: () => Promise<GameAssignment[]>;
  saveGameAssignments: (assignments: GameAssignment[]) => Promise<void>;
  getActiveOverlayId: () => Promise<string | null>;
  setActiveOverlayId: (id: string | null) => Promise<void>;
}
