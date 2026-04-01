import React, { createContext, useContext, useReducer, useEffect, useCallback } from 'react';
import type { AppState, AppAction, Overlay, GameAssignment, EditorTab } from '@/types/omni';
import { DEFAULT_METRICS, SAMPLE_DEFAULT_OVERLAY } from '@/types/omni';
import { getStorageAdapter } from '@/lib/storage-adapter';

const initialState: AppState = {
  overlays: [],
  activeOverlayId: null,
  gameAssignments: [],
  selectedOverlayId: 'default',
  selectedWidgetId: null,
  openTabs: [],
  activeTabId: null,
  themeFiles: {
    // Sample theme files for demonstration
    'themes/neon.css': `.panel {
  background: linear-gradient(135deg, rgba(0, 217, 255, 0.2), rgba(168, 85, 247, 0.2));
  border: 1px solid rgba(0, 217, 255, 0.5);
  box-shadow: 0 0 20px rgba(0, 217, 255, 0.3);
}
.val {
  color: #00D9FF;
  text-shadow: 0 0 10px rgba(0, 217, 255, 0.5);
}
.val.critical {
  color: #EF4444;
  text-shadow: 0 0 10px rgba(239, 68, 68, 0.5);
}`,
    'themes/minimal.css': `.panel {
  background: rgba(0, 0, 0, 0.5);
  border: none;
  padding: 4px;
}
.val {
  color: #FAFAFA;
  font-size: 12px;
}`,
  },
  previewMetrics: DEFAULT_METRICS,
  isDirty: false,
};

function appReducer(state: AppState, action: AppAction): AppState {
  switch (action.type) {
    case 'SET_OVERLAYS':
      return { ...state, overlays: action.payload };

    case 'ADD_OVERLAY':
      return { ...state, overlays: [...state.overlays, action.payload] };

    case 'UPDATE_OVERLAY':
      return {
        ...state,
        overlays: state.overlays.map(o =>
          o.id === action.payload.id ? { ...o, ...action.payload.updates } : o
        ),
      };

    case 'UPDATE_OVERLAY_CONTENT':
      return {
        ...state,
        overlays: state.overlays.map(o =>
          o.id === action.payload.id
            ? { ...o, content: action.payload.content, updatedAt: new Date().toISOString() }
            : o
        ),
        isDirty: true,
      };

    case 'DELETE_OVERLAY':
      return {
        ...state,
        overlays: state.overlays.filter(o => o.id !== action.payload),
        selectedOverlayId:
          state.selectedOverlayId === action.payload
            ? state.overlays.find(o => o.id !== action.payload)?.id || 'default'
            : state.selectedOverlayId,
        activeOverlayId:
          state.activeOverlayId === action.payload ? null : state.activeOverlayId,
      };

    case 'SET_ACTIVE_OVERLAY':
      return { ...state, activeOverlayId: action.payload };

    case 'SELECT_OVERLAY':
      return { ...state, selectedOverlayId: action.payload, selectedWidgetId: null };

    case 'SELECT_WIDGET':
      return { ...state, selectedWidgetId: action.payload };

    case 'SET_GAME_ASSIGNMENTS':
      return { ...state, gameAssignments: action.payload };

    case 'ADD_GAME_ASSIGNMENT':
      // Remove any existing assignment for this executable first
      const filtered = state.gameAssignments.filter(
        a => a.executable !== action.payload.executable
      );
      return { ...state, gameAssignments: [...filtered, action.payload] };

    case 'REMOVE_GAME_ASSIGNMENT':
      return {
        ...state,
        gameAssignments: state.gameAssignments.filter(a => a.executable !== action.payload),
      };

    case 'UPDATE_PREVIEW_METRIC':
      return {
        ...state,
        previewMetrics: {
          ...state.previewMetrics,
          [action.payload.key]: action.payload.value,
        },
      };

    case 'SET_DIRTY':
      return { ...state, isDirty: action.payload };

    case 'OPEN_TAB': {
      // Check if tab already exists
      const existingTab = state.openTabs.find(t => t.id === action.payload.id);
      if (existingTab) {
        return { ...state, activeTabId: action.payload.id };
      }
      return {
        ...state,
        openTabs: [...state.openTabs, action.payload],
        activeTabId: action.payload.id,
      };
    }

    case 'CLOSE_TAB': {
      const newTabs = state.openTabs.filter(t => t.id !== action.payload);
      let newActiveTabId = state.activeTabId;
      
      // If closing active tab, select another
      if (state.activeTabId === action.payload) {
        const closedIndex = state.openTabs.findIndex(t => t.id === action.payload);
        if (newTabs.length > 0) {
          newActiveTabId = newTabs[Math.min(closedIndex, newTabs.length - 1)]?.id || null;
        } else {
          newActiveTabId = null;
        }
      }
      
      return { ...state, openTabs: newTabs, activeTabId: newActiveTabId };
    }

    case 'SET_ACTIVE_TAB':
      return { ...state, activeTabId: action.payload };

    case 'UPDATE_TAB_CONTENT':
      return {
        ...state,
        openTabs: state.openTabs.map(t =>
          t.id === action.payload.id
            ? { ...t, content: action.payload.content, isDirty: true }
            : t
        ),
      };

    case 'SET_THEME_FILE':
      return {
        ...state,
        themeFiles: {
          ...state.themeFiles,
          [action.payload.path]: action.payload.content,
        },
      };

    default:
      return state;
  }
}

interface OmniContextValue {
  state: AppState;
  dispatch: React.Dispatch<AppAction>;
  // Helper functions
  getCurrentOverlay: () => Overlay | undefined;
  createOverlay: (name: string) => Promise<Overlay>;
  duplicateOverlay: (id: string) => Promise<Overlay | null>;
  deleteOverlay: (id: string) => Promise<void>;
  saveCurrentOverlay: () => Promise<void>;
  setAsActive: (id: string | null) => Promise<void>;
  assignToGame: (overlayId: string, executable: string) => Promise<void>;
  removeGameAssignment: (executable: string) => Promise<void>;
  getOverlayForGame: (executable: string) => Overlay | undefined;
  // Tab functions
  openThemeTab: (themePath: string) => void;
  openOverlayTab: (overlayId: string) => void;
  closeTab: (tabId: string) => void;
  getActiveTab: () => EditorTab | undefined;
}

const OmniContext = createContext<OmniContextValue | null>(null);

export function OmniProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(appReducer, initialState);
  const storage = getStorageAdapter();

  // Load initial data
  useEffect(() => {
    async function loadData() {
      const [overlays, assignments, activeId] = await Promise.all([
        storage.loadOverlays(),
        storage.loadGameAssignments(),
        storage.getActiveOverlayId(),
      ]);

      dispatch({ type: 'SET_OVERLAYS', payload: overlays });
      dispatch({ type: 'SET_GAME_ASSIGNMENTS', payload: assignments });
      dispatch({ type: 'SET_ACTIVE_OVERLAY', payload: activeId });

      // Select first overlay if available
      if (overlays.length > 0 && !overlays.find(o => o.id === 'default')) {
        dispatch({ type: 'SELECT_OVERLAY', payload: overlays[0].id });
      }
    }

    loadData();
  }, []);

  const getCurrentOverlay = useCallback(() => {
    return state.overlays.find(o => o.id === state.selectedOverlayId);
  }, [state.overlays, state.selectedOverlayId]);

  const createOverlay = useCallback(async (name: string): Promise<Overlay> => {
    const overlay: Overlay = {
      id: `overlay-${Date.now()}`,
      name,
      isDefault: false,
      content: `<widget id="new-widget" name="New Widget" enabled="true">
  <template>
    <div class="panel" style="position: fixed; top: 20px; left: 20px;">
      <span class="val">FPS: {fps}</span>
    </div>
  </template>
  <style>
    .panel {
      background: rgba(20, 20, 20, 0.7);
      border-radius: 4px;
      padding: 6px;
    }
    .val {
      color: #ffffff;
      font-size: 16px;
    }
  </style>
</widget>`,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    await storage.saveOverlay(overlay);
    dispatch({ type: 'ADD_OVERLAY', payload: overlay });
    dispatch({ type: 'SELECT_OVERLAY', payload: overlay.id });

    return overlay;
  }, [storage]);

  const duplicateOverlay = useCallback(async (id: string): Promise<Overlay | null> => {
    const source = state.overlays.find(o => o.id === id);
    if (!source) return null;

    const overlay: Overlay = {
      id: `overlay-${Date.now()}`,
      name: `${source.name} (Copy)`,
      isDefault: false,
      content: source.content,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    await storage.saveOverlay(overlay);
    dispatch({ type: 'ADD_OVERLAY', payload: overlay });
    dispatch({ type: 'SELECT_OVERLAY', payload: overlay.id });

    return overlay;
  }, [state.overlays, storage]);

  const deleteOverlay = useCallback(async (id: string): Promise<void> => {
    const overlay = state.overlays.find(o => o.id === id);
    if (!overlay || overlay.isDefault) return; // Can't delete default

    await storage.deleteOverlay(id);
    dispatch({ type: 'DELETE_OVERLAY', payload: id });
  }, [state.overlays, storage]);

  const saveCurrentOverlay = useCallback(async (): Promise<void> => {
    const overlay = getCurrentOverlay();
    if (!overlay) return;

    await storage.saveOverlay(overlay);
    dispatch({ type: 'SET_DIRTY', payload: false });
  }, [getCurrentOverlay, storage]);

  const setAsActive = useCallback(async (id: string | null): Promise<void> => {
    await storage.setActiveOverlayId(id);
    dispatch({ type: 'SET_ACTIVE_OVERLAY', payload: id });
  }, [storage]);

  const assignToGame = useCallback(async (overlayId: string, executable: string): Promise<void> => {
    const assignment: GameAssignment = { overlayId, executable };
    dispatch({ type: 'ADD_GAME_ASSIGNMENT', payload: assignment });
    
    // Save all assignments
    const newAssignments = [
      ...state.gameAssignments.filter(a => a.executable !== executable),
      assignment,
    ];
    await storage.saveGameAssignments(newAssignments);
  }, [state.gameAssignments, storage]);

  const removeGameAssignment = useCallback(async (executable: string): Promise<void> => {
    dispatch({ type: 'REMOVE_GAME_ASSIGNMENT', payload: executable });
    
    const newAssignments = state.gameAssignments.filter(a => a.executable !== executable);
    await storage.saveGameAssignments(newAssignments);
  }, [state.gameAssignments, storage]);

  const getOverlayForGame = useCallback((executable: string): Overlay | undefined => {
    // Priority: Per-game -> Active -> Default
    const assignment = state.gameAssignments.find(a => a.executable === executable);
    if (assignment) {
      return state.overlays.find(o => o.id === assignment.overlayId);
    }

    if (state.activeOverlayId) {
      return state.overlays.find(o => o.id === state.activeOverlayId);
    }

    return state.overlays.find(o => o.isDefault);
  }, [state.gameAssignments, state.overlays, state.activeOverlayId]);

  const openThemeTab = useCallback((themePath: string): void => {
    const content = state.themeFiles[themePath] || `/* Theme: ${themePath} */\n/* File not found */`;
    const name = themePath.split('/').pop()?.replace(/\.css$/i, '') || themePath;
    
    const tab: EditorTab = {
      id: `theme:${themePath}`,
      name: `${name}.css`,
      type: 'theme',
      content,
      isDirty: false,
    };
    
    dispatch({ type: 'OPEN_TAB', payload: tab });
  }, [state.themeFiles]);

  const openOverlayTab = useCallback((overlayId: string): void => {
    const overlay = state.overlays.find(o => o.id === overlayId);
    if (!overlay) return;
    
    const tab: EditorTab = {
      id: `overlay:${overlayId}`,
      name: `${overlay.name}.omni`,
      type: 'overlay',
      content: overlay.content,
      isDirty: false,
    };
    
    dispatch({ type: 'OPEN_TAB', payload: tab });
  }, [state.overlays]);

  const closeTab = useCallback((tabId: string): void => {
    dispatch({ type: 'CLOSE_TAB', payload: tabId });
  }, []);

  const getActiveTab = useCallback((): EditorTab | undefined => {
    return state.openTabs.find(t => t.id === state.activeTabId);
  }, [state.openTabs, state.activeTabId]);

  const value: OmniContextValue = {
    state,
    dispatch,
    getCurrentOverlay,
    createOverlay,
    duplicateOverlay,
    deleteOverlay,
    saveCurrentOverlay,
    setAsActive,
    assignToGame,
    removeGameAssignment,
    getOverlayForGame,
    openThemeTab,
    openOverlayTab,
    closeTab,
    getActiveTab,
  };

  return <OmniContext.Provider value={value}>{children}</OmniContext.Provider>;
}

export function useOmniState(): OmniContextValue {
  const context = useContext(OmniContext);
  if (!context) {
    throw new Error('useOmniState must be used within an OmniProvider');
  }
  return context;
}
