import type { AppState, AppAction } from '@/types/omni';

export function appReducer(state: AppState, action: AppAction): AppState {
  switch (action.type) {
    case 'SET_OVERLAYS':
      return { ...state, overlays: action.payload };

    case 'ADD_OVERLAY':
      return { ...state, overlays: [...state.overlays, action.payload] };

    case 'UPDATE_OVERLAY_CONTENT':
      return {
        ...state,
        overlays: state.overlays.map((o) =>
          o.name === action.payload.name ? { ...o, content: action.payload.content } : o,
        ),
      };

    case 'DELETE_OVERLAY':
      return {
        ...state,
        overlays: state.overlays.filter((o) => o.name !== action.payload),
        selectedOverlayName:
          state.selectedOverlayName === action.payload
            ? state.overlays.find((o) => o.name !== action.payload)?.name || 'Default'
            : state.selectedOverlayName,
      };

    case 'SELECT_OVERLAY':
      return { ...state, selectedOverlayName: action.payload, selectedWidgetId: null };

    case 'SELECT_WIDGET':
      return {
        ...state,
        selectedWidgetId: action.payload,
        widgetScrollRequest: action.payload
          ? state.widgetScrollRequest + 1
          : state.widgetScrollRequest,
      };

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
      const existingTab = state.openTabs.find((t) => t.id === action.payload.id);
      if (existingTab) {
        return {
          ...state,
          openTabs: state.openTabs.map((t) => (t.id === action.payload.id ? action.payload : t)),
          activeTabId: action.payload.id,
        };
      }
      return {
        ...state,
        openTabs: [...state.openTabs, action.payload],
        activeTabId: action.payload.id,
      };
    }

    case 'CLOSE_TAB': {
      const newTabs = state.openTabs.filter((t) => t.id !== action.payload);
      let newActiveTabId = state.activeTabId;

      if (state.activeTabId === action.payload) {
        const closedIndex = state.openTabs.findIndex((t) => t.id === action.payload);
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
        openTabs: state.openTabs.map((t) =>
          t.id === action.payload.id ? { ...t, content: action.payload.content, isDirty: true } : t,
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

    case 'SET_ACTIVE_PANEL':
      return { ...state, activePanel: action.payload };

    case 'SET_UPDATE_READY':
      return {
        ...state,
        updateReady: true,
        updateVersion: action.payload.version,
        updateReleaseDate: action.payload.releaseDate,
      };

    case 'SET_HWINFO_SENSORS':
      return {
        ...state,
        hwinfoConnected: action.payload.connected,
        hwinfoSensors: action.payload.sensors,
        hwinfoSensorCount: action.payload.sensors.length,
      };

    default:
      return state;
  }
}
