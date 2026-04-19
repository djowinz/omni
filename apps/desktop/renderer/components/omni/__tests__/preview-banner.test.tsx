/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useEffect } from 'react';
import { PreviewContextProvider, usePreview } from '../../../lib/preview-context';
import type { CachedArtifactDetail } from '../../../lib/share-types';
import { PreviewBanner } from '../preview-banner';

// ── Fixture ───────────────────────────────────────────────────────────────────

const FIXTURE_TOKEN = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee';

const FIXTURE_ARTIFACT: CachedArtifactDetail = {
  artifact_id: 'art-001',
  content_hash: 'deadbeef',
  author_pubkey: 'pubkey-hex',
  name: 'Neon Dusk',
  kind: 'theme',
  r2_url: 'https://r2.example/art-001.tar.zst',
  thumbnail_url: 'https://r2.example/art-001.png',
  updated_at: 1700000000,
};

// ── Window.omni stub ──────────────────────────────────────────────────────────

function stubOmni() {
  vi.stubGlobal('omni', {
    sendShareMessage: vi.fn().mockImplementation(async (msg: { id: string }) => ({
      id: msg.id,
      type: 'explorer.cancelPreviewResult',
      restored: true,
    })),
    onShareEvent: vi.fn(() => () => {}),
  });
}

// ── TestHarness ───────────────────────────────────────────────────────────────

/**
 * Wraps children inside <PreviewContextProvider> and, when `initial` is
 * provided, calls setPreview() on mount so tests can boot with an active
 * token + artifact without going through the WS flow.
 */
function TestHarness({
  children,
  initial,
}: {
  children: React.ReactNode;
  initial?: { token: string; artifact: CachedArtifactDetail };
}) {
  return (
    <PreviewContextProvider>
      <Initializer initial={initial} />
      {children}
    </PreviewContextProvider>
  );
}

function Initializer({ initial }: { initial?: { token: string; artifact: CachedArtifactDetail } }) {
  const { setPreview } = usePreview();
  useEffect(() => {
    if (initial) {
      setPreview(initial.token, initial.artifact);
    }
  }, []);
  return null;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('PreviewBanner', () => {
  beforeEach(() => {
    stubOmni();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('renders null when no active token', () => {
    render(
      <TestHarness>
        <PreviewBanner />
      </TestHarness>,
    );

    expect(screen.queryByTestId('preview-banner')).toBeNull();
  });

  it('renders banner when active token exists', async () => {
    render(
      <TestHarness initial={{ token: FIXTURE_TOKEN, artifact: FIXTURE_ARTIFACT }}>
        <PreviewBanner />
      </TestHarness>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('preview-banner')).toBeInTheDocument();
    });

    expect(screen.getByText(/Neon Dusk/)).toBeInTheDocument();
  });

  it('Revert button calls sendShareMessage("explorer.cancelPreview") and clears preview', async () => {
    const user = userEvent.setup();

    render(
      <TestHarness initial={{ token: FIXTURE_TOKEN, artifact: FIXTURE_ARTIFACT }}>
        <PreviewBanner />
      </TestHarness>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('preview-banner-revert')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('preview-banner-revert'));

    await waitFor(() => {
      expect(window.omni!.sendShareMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'explorer.cancelPreview',
          id: expect.any(String),
          params: expect.objectContaining({ preview_token: FIXTURE_TOKEN }),
        }),
      );
    });

    // Banner should null-render after clearPreview()
    await waitFor(() => {
      expect(screen.queryByTestId('preview-banner')).toBeNull();
    });
  });

  it('Install Now button clears preview', async () => {
    const user = userEvent.setup();

    render(
      <TestHarness initial={{ token: FIXTURE_TOKEN, artifact: FIXTURE_ARTIFACT }}>
        <PreviewBanner />
      </TestHarness>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('preview-banner-install-now')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('preview-banner-install-now'));

    await waitFor(() => {
      expect(screen.queryByTestId('preview-banner')).toBeNull();
    });
  });
});
