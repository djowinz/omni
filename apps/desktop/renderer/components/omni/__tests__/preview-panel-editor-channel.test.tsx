/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';

// Mock context hooks that PreviewPanel depends on so we can render without a full provider tree.
vi.mock('../../../hooks/use-omni-state', () => ({
  useOmniState: () => ({ state: { connected: false }, dispatch: vi.fn() }),
}));
vi.mock('../../../hooks/use-backend', () => ({
  useBackend: () => ({ subscribePreview: vi.fn() }),
}));
vi.mock('../../../hooks/use-sensor-data', () => ({
  useSensorData: () => null,
}));

describe('PreviewPanel — subscribes to editor channel', () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('uses onPreviewHtmlEditor / onPreviewUpdateEditor, not the in-game variants', async () => {
    const onPreviewHtmlEditor = vi.fn(() => () => {});
    const onPreviewUpdateEditor = vi.fn(() => () => {});
    const onPreviewHtml = vi.fn(() => () => {});
    const onPreviewUpdate = vi.fn(() => () => {});

    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      onPreviewHtmlEditor,
      onPreviewUpdateEditor,
      onPreviewHtml,
      onPreviewUpdate,
      subscribePreview: vi.fn(),
    });

    const { PreviewPanel } = await import('../preview-panel');
    render(<PreviewPanel />);

    await waitFor(() => {
      expect(onPreviewHtmlEditor).toHaveBeenCalledTimes(1);
      expect(onPreviewUpdateEditor).toHaveBeenCalledTimes(1);
    });
    expect(onPreviewHtml).not.toHaveBeenCalled();
    expect(onPreviewUpdate).not.toHaveBeenCalled();
  });
});
