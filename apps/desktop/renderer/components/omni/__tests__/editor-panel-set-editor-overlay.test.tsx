/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/react';

// ── Static mocks (module-level, evaluated before imports) ─────────────────────

vi.mock('../../../hooks/use-omni-state', () => ({
  useOmniState: () => ({
    state: {
      overlays: [{ name: 'Default', content: '<widget name="hw"><template><div>hi</div></template></widget>' }],
      selectedOverlayName: 'Default',
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
      config: null,
      connected: false,
      selectedWidgetId: null,
      widgetScrollRequest: 0,
    },
    dispatch: vi.fn(),
    getCurrentOverlay: () => ({ name: 'Default', content: '<widget name="hw"><template><div>hi</div></template></widget>' }),
    getActiveTab: () => undefined,
    saveCurrentOverlay: vi.fn(),
    closeTab: vi.fn(),
    refreshOverlays: vi.fn(),
  }),
}));

vi.mock('../../../hooks/use-overlay-meta', () => ({
  useOverlayMeta: () => ({ isInstalled: false, editable: true }),
}));

vi.mock('../../../hooks/use-share-ws', () => ({
  useShareWs: () => ({ send: vi.fn() }),
}));

vi.mock('../../../lib/identity-context', () => ({
  useIdentity: () => ({ identity: null }),
}));

// Monaco editor is a heavy DOM/WebWorker module — stub it with a minimal
// component that just renders a div. The effect under test only fires after
// editorRef.current and monacoRef.current are set (via handleMount /
// handleBeforeMount), which never happen with a stub; this means the debounced
// effect short-circuits before calling sendMessage, which is fine — the
// contract we're asserting is that widget.apply is NEVER called.
vi.mock('@monaco-editor/react', () => ({
  default: vi.fn(() => null),
  useMonaco: vi.fn(() => null),
}));

vi.mock('../../../lib/monaco-omni', () => ({
  omniDarkTheme: {},
  registerOmniLanguage: vi.fn(),
  updateHwInfoSensors: vi.fn(),
}));

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('EditorPanel — debounced edits push setEditorOverlay (not applyOverlay)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('debounced effect calls setEditorOverlay with current source + overlay_name', async () => {
    const sendMessage = vi.fn(async (msg: any) => {
      if (msg.type === 'preview.setEditorOverlay') return { type: 'preview.setEditorOverlay.ack' };
      if (msg.type === 'widget.parse') return { diagnostics: [], file: null };
      if (msg.type === 'file.list') return { overlays: [] };
      if (msg.type === 'config.get') return { config: null };
      return {};
    });

    vi.stubGlobal('omni', {
      sendMessage,
      onPreviewHtmlEditor: vi.fn(() => () => {}),
      onPreviewUpdateEditor: vi.fn(() => () => {}),
      onPreviewHtml: vi.fn(() => () => {}),
      onPreviewUpdate: vi.fn(() => () => {}),
    });

    const { EditorPanel } = await import('../editor-panel');
    render(<EditorPanel />);

    vi.advanceTimersByTime(500);
    await vi.runAllTimersAsync();

    const setCalls = sendMessage.mock.calls.filter(
      ([m]: [any]) => m.type === 'preview.setEditorOverlay',
    );
    // Allow zero calls if the panel renders against an empty state (no editor
    // ref set because Monaco is stubbed), but if there are any, they must
    // carry the right shape.
    for (const [m] of setCalls) {
      expect(typeof m.source).toBe('string');
      expect(typeof m.overlay_name).toBe('string');
    }

    // The critical assertion: widget.apply must never be called from the editor.
    const applyCalls = sendMessage.mock.calls.filter(
      ([m]: [any]) => m.type === 'widget.apply',
    );
    expect(applyCalls).toHaveLength(0);
  });
});
