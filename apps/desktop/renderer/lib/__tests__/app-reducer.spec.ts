import { describe, it, expect } from 'vitest';
import { appReducer } from '../app-reducer';
import type { AppState, EditorTab } from '@/types/omni';

function makeState(overrides?: Partial<AppState>): AppState {
  return {
    overlays: [],
    config: null,
    connected: false,
    selectedOverlayName: 'Default',
    selectedWidgetId: null,
    widgetScrollRequest: 0,
    openTabs: [],
    activeTabId: null,
    editorViewStates: {},
    themeFiles: {},
    isDirty: false,
    activePanel: 'components',
    updateReady: false,
    updateVersion: null,
    updateReleaseDate: null,
    hwinfoConnected: false,
    hwinfoSensorCount: 0,
    hwinfoSensors: [],
    ...overrides,
  };
}

function makeTab(id: string, content = ''): EditorTab {
  return { id, name: id, type: 'overlay', content, isDirty: false };
}

describe('appReducer', () => {
  describe('overlay management', () => {
    describe('given DELETE_OVERLAY on the currently selected overlay', () => {
      it('should auto-select another overlay', () => {
        const state = makeState({
          overlays: [
            { name: 'Default', content: null },
            { name: 'Custom', content: null },
          ],
          selectedOverlayName: 'Custom',
        });

        const next = appReducer(state, { type: 'DELETE_OVERLAY', payload: 'Custom' });

        expect(next.overlays).toHaveLength(1);
        expect(next.selectedOverlayName).toBe('Default');
      });
    });

    describe('given DELETE_OVERLAY on a non-selected overlay', () => {
      it('should keep the current selection', () => {
        const state = makeState({
          overlays: [
            { name: 'Default', content: null },
            { name: 'Custom', content: null },
          ],
          selectedOverlayName: 'Default',
        });

        const next = appReducer(state, { type: 'DELETE_OVERLAY', payload: 'Custom' });

        expect(next.overlays).toHaveLength(1);
        expect(next.selectedOverlayName).toBe('Default');
      });
    });

    describe('given DELETE_OVERLAY when no other overlays exist', () => {
      it('should fall back to Default', () => {
        const state = makeState({
          overlays: [{ name: 'OnlyOne', content: null }],
          selectedOverlayName: 'OnlyOne',
        });

        const next = appReducer(state, { type: 'DELETE_OVERLAY', payload: 'OnlyOne' });

        expect(next.overlays).toHaveLength(0);
        expect(next.selectedOverlayName).toBe('Default');
      });
    });

    describe('given SELECT_OVERLAY', () => {
      it('should clear the selected widget', () => {
        const state = makeState({ selectedWidgetId: 'fps-widget' });

        const next = appReducer(state, { type: 'SELECT_OVERLAY', payload: 'Custom' });

        expect(next.selectedOverlayName).toBe('Custom');
        expect(next.selectedWidgetId).toBeNull();
      });
    });

    describe('given UPDATE_OVERLAY_CONTENT', () => {
      it('should update content for the matching overlay', () => {
        const state = makeState({
          overlays: [{ name: 'Test', content: 'old content' }],
        });

        const next = appReducer(state, {
          type: 'UPDATE_OVERLAY_CONTENT',
          payload: { name: 'Test', content: 'new content' },
        });

        expect(next.overlays[0].content).toBe('new content');
      });

      it('should not modify other overlays', () => {
        const state = makeState({
          overlays: [
            { name: 'A', content: 'a' },
            { name: 'B', content: 'b' },
          ],
        });

        const next = appReducer(state, {
          type: 'UPDATE_OVERLAY_CONTENT',
          payload: { name: 'A', content: 'updated' },
        });

        expect(next.overlays[1].content).toBe('b');
      });
    });
  });

  describe('tab management', () => {
    describe('given OPEN_TAB for a new tab', () => {
      it('should add the tab and set it active', () => {
        const tab = makeTab('overlay:FPS');
        const state = makeState();

        const next = appReducer(state, { type: 'OPEN_TAB', payload: tab });

        expect(next.openTabs).toHaveLength(1);
        expect(next.activeTabId).toBe('overlay:FPS');
      });
    });

    describe('given OPEN_TAB for an existing tab', () => {
      it('should switch to it without duplicating', () => {
        const tab = makeTab('overlay:FPS');
        const state = makeState({
          openTabs: [tab, makeTab('overlay:GPU')],
          activeTabId: 'overlay:GPU',
        });

        const next = appReducer(state, { type: 'OPEN_TAB', payload: tab });

        expect(next.openTabs).toHaveLength(2);
        expect(next.activeTabId).toBe('overlay:FPS');
      });
    });

    describe('given CLOSE_TAB on the active tab', () => {
      it('should switch to the adjacent tab', () => {
        const state = makeState({
          openTabs: [makeTab('A'), makeTab('B'), makeTab('C')],
          activeTabId: 'B',
        });

        const next = appReducer(state, { type: 'CLOSE_TAB', payload: 'B' });

        expect(next.openTabs).toHaveLength(2);
        expect(next.activeTabId).toBe('C');
      });
    });

    describe('given CLOSE_TAB on the last tab', () => {
      it('should switch to the previous tab', () => {
        const state = makeState({
          openTabs: [makeTab('A'), makeTab('B')],
          activeTabId: 'B',
        });

        const next = appReducer(state, { type: 'CLOSE_TAB', payload: 'B' });

        expect(next.openTabs).toHaveLength(1);
        expect(next.activeTabId).toBe('A');
      });
    });

    describe('given CLOSE_TAB on the only tab', () => {
      it('should set activeTabId to null', () => {
        const state = makeState({
          openTabs: [makeTab('A')],
          activeTabId: 'A',
        });

        const next = appReducer(state, { type: 'CLOSE_TAB', payload: 'A' });

        expect(next.openTabs).toHaveLength(0);
        expect(next.activeTabId).toBeNull();
      });
    });

    describe('given CLOSE_TAB on a non-active tab', () => {
      it('should keep the active tab unchanged', () => {
        const state = makeState({
          openTabs: [makeTab('A'), makeTab('B'), makeTab('C')],
          activeTabId: 'A',
        });

        const next = appReducer(state, { type: 'CLOSE_TAB', payload: 'B' });

        expect(next.openTabs).toHaveLength(2);
        expect(next.activeTabId).toBe('A');
      });
    });

    describe('given UPDATE_TAB_CONTENT', () => {
      it('should update content and mark the tab dirty', () => {
        const state = makeState({
          openTabs: [makeTab('A', 'original')],
        });

        const next = appReducer(state, {
          type: 'UPDATE_TAB_CONTENT',
          payload: { id: 'A', content: 'modified' },
        });

        expect(next.openTabs[0].content).toBe('modified');
        expect(next.openTabs[0].isDirty).toBe(true);
      });
    });
  });

  describe('auto-update state', () => {
    describe('given SET_UPDATE_READY', () => {
      it('should store version and release date', () => {
        const state = makeState();

        const next = appReducer(state, {
          type: 'SET_UPDATE_READY',
          payload: { version: '1.2.0', releaseDate: '2026-04-08T05:00:00Z' },
        });

        expect(next.updateReady).toBe(true);
        expect(next.updateVersion).toBe('1.2.0');
        expect(next.updateReleaseDate).toBe('2026-04-08T05:00:00Z');
      });
    });
  });
});
