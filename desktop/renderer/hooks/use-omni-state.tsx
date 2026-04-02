import React, { createContext, useContext, useReducer, useEffect, useCallback } from 'react';
import type { AppState, AppAction, Overlay, EditorTab } from '@/types/omni';
import { DEFAULT_METRICS } from '@/types/omni';
import type { Config } from '@/src/generated/Config';
import { BackendApi } from '@/lib/backend-api';

const backend = new BackendApi();

const initialState: AppState = {
  overlays: [],
  config: null,
  connected: false,
  selectedOverlayName: 'Default',
  selectedWidgetId: null,
  openTabs: [],
  activeTabId: null,
  themeFiles: {},
  previewMetrics: DEFAULT_METRICS,
  isDirty: false,
};

function appReducer(state: AppState, action: AppAction): AppState {
  switch (action.type) {
    case 'SET_OVERLAYS':
      return { ...state, overlays: action.payload };

    case 'ADD_OVERLAY':
      return { ...state, overlays: [...state.overlays, action.payload] };

    case 'UPDATE_OVERLAY_CONTENT':
      return {
        ...state,
        overlays: state.overlays.map(o =>
          o.name === action.payload.name
            ? { ...o, content: action.payload.content }
            : o
        ),
        isDirty: true,
      };

    case 'DELETE_OVERLAY':
      return {
        ...state,
        overlays: state.overlays.filter(o => o.name !== action.payload),
        selectedOverlayName:
          state.selectedOverlayName === action.payload
            ? state.overlays.find(o => o.name !== action.payload)?.name || 'Default'
            : state.selectedOverlayName,
      };

    case 'SELECT_OVERLAY':
      return { ...state, selectedOverlayName: action.payload, selectedWidgetId: null };

    case 'SELECT_WIDGET':
      return { ...state, selectedWidgetId: action.payload };

    case 'SET_CONFIG':
      return { ...state, config: action.payload };

    case 'SET_CONNECTED':
      return { ...state, connected: action.payload };

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

/** Load the list of overlay names from the backend, creating Overlay stubs with null content. */
async function loadOverlayList(): Promise<Overlay[]> {
  const res = await backend.listFiles();
  const names: string[] = res.overlays ?? [];
  return names.map(name => ({ name, content: null }));
}

/** Load a single overlay's content from the backend. */
async function loadOverlayContent(name: string): Promise<string> {
  return backend.readFile(`overlays/${name}/overlay.omni`);
}

interface OmniContextValue {
  state: AppState;
  dispatch: React.Dispatch<AppAction>;
  // Helper functions
  getCurrentOverlay: () => Overlay | undefined;
  ensureOverlayLoaded: (name: string) => Promise<void>;
  createOverlay: (name: string) => Promise<void>;
  duplicateOverlay: (name: string) => Promise<void>;
  deleteOverlay: (name: string) => Promise<void>;
  saveCurrentOverlay: () => Promise<void>;
  setAsActive: (name: string) => Promise<void>;
  assignToGame: (overlayName: string, executable: string) => Promise<void>;
  removeGameAssignment: (executable: string) => Promise<void>;
  getOverlayForGame: (executable: string) => Overlay | undefined;
  // Tab functions
  openThemeTab: (themePath: string) => void;
  openOverlayTab: (overlayName: string) => void;
  closeTab: (tabId: string) => void;
  getActiveTab: () => EditorTab | undefined;
}

const OmniContext = createContext<OmniContextValue | null>(null);

export function OmniProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(appReducer, initialState);

  // Load initial data from backend
  useEffect(() => {
    async function loadData() {
      try {
        const [overlays, config] = await Promise.all([
          loadOverlayList(),
          backend.getConfig(),
        ]);

        dispatch({ type: 'SET_OVERLAYS', payload: overlays });
        dispatch({ type: 'SET_CONFIG', payload: config });
        dispatch({ type: 'SET_CONNECTED', payload: true });

        // Select the active overlay or first available
        if (config.active_overlay && overlays.find(o => o.name === config.active_overlay)) {
          dispatch({ type: 'SELECT_OVERLAY', payload: config.active_overlay });
        } else if (overlays.length > 0) {
          dispatch({ type: 'SELECT_OVERLAY', payload: overlays[0].name });
        }

        // Eagerly load the selected overlay's content
        const selectedName = config.active_overlay && overlays.find(o => o.name === config.active_overlay)
          ? config.active_overlay
          : overlays[0]?.name;
        if (selectedName) {
          try {
            const content = await loadOverlayContent(selectedName);
            dispatch({ type: 'UPDATE_OVERLAY_CONTENT', payload: { name: selectedName, content } });
            dispatch({ type: 'SET_DIRTY', payload: false }); // Loading isn't a user edit
          } catch { /* overlay file may not exist yet */ }
        }
      } catch {
        // Backend not available — graceful fallback
        dispatch({ type: 'SET_CONNECTED', payload: false });
      }
    }

    loadData();
  }, []);

  // Listen for host connection status changes
  useEffect(() => {
    const unsub = window.omni?.onHostStatus?.((status: any) => {
      dispatch({ type: 'SET_CONNECTED', payload: !!status?.connected });
    });
    return () => { unsub?.(); };
  }, []);

  /** Ensure an overlay's content is loaded (lazy loading). */
  const ensureOverlayLoaded = useCallback(async (name: string) => {
    const overlay = state.overlays.find(o => o.name === name);
    if (overlay && overlay.content !== null) return; // Already loaded

    try {
      const content = await loadOverlayContent(name);
      dispatch({ type: 'UPDATE_OVERLAY_CONTENT', payload: { name, content } });
      dispatch({ type: 'SET_DIRTY', payload: false });
    } catch {
      // File may not exist; set empty content
      dispatch({ type: 'UPDATE_OVERLAY_CONTENT', payload: { name, content: '' } });
      dispatch({ type: 'SET_DIRTY', payload: false });
    }
  }, [state.overlays]);

  const getCurrentOverlay = useCallback(() => {
    return state.overlays.find(o => o.name === state.selectedOverlayName);
  }, [state.overlays, state.selectedOverlayName]);

  const createOverlay = useCallback(async (name: string): Promise<void> => {
    try {
      await backend.createOverlay(name);
      // Reload the overlay list from backend
      const overlays = await loadOverlayList();
      dispatch({ type: 'SET_OVERLAYS', payload: overlays });
      dispatch({ type: 'SELECT_OVERLAY', payload: name });
      // Load the newly created overlay's content
      try {
        const content = await loadOverlayContent(name);
        dispatch({ type: 'UPDATE_OVERLAY_CONTENT', payload: { name, content } });
        dispatch({ type: 'SET_DIRTY', payload: false });
      } catch { /* new overlay may have no content yet */ }
    } catch (err) {
      console.error('Failed to create overlay:', err);
    }
  }, []);

  const duplicateOverlay = useCallback(async (name: string): Promise<void> => {
    const source = state.overlays.find(o => o.name === name);
    if (!source) return;

    // Ensure source content is loaded
    let content = source.content;
    if (content === null) {
      try {
        content = await loadOverlayContent(name);
      } catch {
        return;
      }
    }

    const newName = `${name} (Copy)`;
    try {
      await backend.createOverlay(newName);
      await backend.writeFile(`overlays/${newName}/overlay.omni`, content);
      // Reload overlay list
      const overlays = await loadOverlayList();
      dispatch({ type: 'SET_OVERLAYS', payload: overlays });
      dispatch({ type: 'SELECT_OVERLAY', payload: newName });
      dispatch({ type: 'UPDATE_OVERLAY_CONTENT', payload: { name: newName, content } });
      dispatch({ type: 'SET_DIRTY', payload: false });
    } catch (err) {
      console.error('Failed to duplicate overlay:', err);
    }
  }, [state.overlays]);

  const deleteOverlay = useCallback(async (name: string): Promise<void> => {
    if (name === 'Default') return; // Can't delete default

    try {
      await backend.deleteFile(`overlays/${name}`);
      dispatch({ type: 'DELETE_OVERLAY', payload: name });
    } catch (err) {
      console.error('Failed to delete overlay:', err);
    }
  }, []);

  const saveCurrentOverlay = useCallback(async (): Promise<void> => {
    const overlay = state.overlays.find(o => o.name === state.selectedOverlayName);
    if (!overlay || overlay.content === null) return;

    try {
      await backend.writeFile(`overlays/${overlay.name}/overlay.omni`, overlay.content);
      dispatch({ type: 'SET_DIRTY', payload: false });

      // If this is the active overlay, also apply it to the host
      if (state.config?.active_overlay === overlay.name) {
        await backend.applyOverlay(overlay.content);
      }
    } catch (err) {
      console.error('Failed to save overlay:', err);
    }
  }, [state.overlays, state.selectedOverlayName, state.config]);

  const setAsActive = useCallback(async (name: string): Promise<void> => {
    try {
      const config = await backend.getConfig();
      config.active_overlay = name;
      await backend.updateConfig(config);
      dispatch({ type: 'SET_CONFIG', payload: config });
    } catch (err) {
      console.error('Failed to set active overlay:', err);
    }
  }, []);

  const assignToGame = useCallback(async (overlayName: string, executable: string): Promise<void> => {
    try {
      const config = await backend.getConfig();
      config.overlay_by_game[executable] = overlayName;
      await backend.updateConfig(config);
      dispatch({ type: 'SET_CONFIG', payload: config });
    } catch (err) {
      console.error('Failed to assign overlay to game:', err);
    }
  }, []);

  const removeGameAssignment = useCallback(async (executable: string): Promise<void> => {
    try {
      const config = await backend.getConfig();
      delete config.overlay_by_game[executable];
      await backend.updateConfig(config);
      dispatch({ type: 'SET_CONFIG', payload: config });
    } catch (err) {
      console.error('Failed to remove game assignment:', err);
    }
  }, []);

  const getOverlayForGame = useCallback((executable: string): Overlay | undefined => {
    // Priority: Per-game -> Active -> Default
    const gameOverlay = state.config?.overlay_by_game[executable];
    if (gameOverlay) {
      return state.overlays.find(o => o.name === gameOverlay);
    }

    const activeOverlay = state.config?.active_overlay;
    if (activeOverlay) {
      return state.overlays.find(o => o.name === activeOverlay);
    }

    return state.overlays.find(o => o.name === 'Default');
  }, [state.config, state.overlays]);

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

  const openOverlayTab = useCallback((overlayName: string): void => {
    const overlay = state.overlays.find(o => o.name === overlayName);
    if (!overlay) return;

    const tab: EditorTab = {
      id: `overlay:${overlayName}`,
      name: `${overlay.name}.omni`,
      type: 'overlay',
      content: overlay.content ?? '',
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
    ensureOverlayLoaded,
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
