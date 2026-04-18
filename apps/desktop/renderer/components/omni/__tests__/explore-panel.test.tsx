/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import type { CachedArtifactDetail } from '../../../lib/share-types';

const SAMPLE: CachedArtifactDetail = {
  artifact_id: 'art-1',
  content_hash: 'h',
  author_pubkey: 'pk',
  name: 'Demo',
  kind: 'theme',
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  updated_at: 1700000000,
};

// Dynamically import the Provider + nuqs testing adapter AFTER vi.resetModules()
// so the Provider binds to the SAME module instance as the usePreview() call
// inside ExplorePanel's subtree (resetModules invalidates the module cache, so
// a statically-imported Provider would reference a stale Context object — the
// T8 retro flagged exactly this pitfall).
async function loadWrap() {
  const { PreviewContextProvider } = await import('../../../lib/preview-context');
  const { NuqsTestingAdapter } = await import('nuqs/adapters/testing');
  return function Wrap({ children, sp = '' }: { children: ReactNode; sp?: string }) {
    return (
      <NuqsTestingAdapter searchParams={sp}>
        <PreviewContextProvider>{children}</PreviewContextProvider>
      </NuqsTestingAdapter>
    );
  };
}

describe('ExplorePanel', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.doMock('../../../hooks/use-explore-list', () => ({
      useExploreList: () => ({
        items: [SAMPLE],
        nextCursor: null,
        loading: false,
        error: null,
        loadMore: vi.fn(),
        refetch: vi.fn(),
      }),
    }));
    vi.doMock('../../../hooks/use-explore-detail', () => ({
      useExploreDetail: () => ({ artifact: null, loading: false, error: null }),
    }));
    vi.doMock('../../../hooks/use-config-vocab', () => ({
      useConfigVocab: () => ({ tags: [], version: 0, loading: false, error: null }),
    }));
    // UploadDialog's SourceStep loads the workspace via useWorkspaceList,
    // which calls window.omni.sendMessage({ type: 'file.list' }). Stub it so
    // the dialog can mount when the + Upload CTA is clicked.
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(async () => ({ type: 'file.list', overlays: [], themes: [] })),
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders three sub-tab buttons', async () => {
    const Wrap = await loadWrap();
    const { ExplorePanel } = await import('../explore-panel');
    render(
      <Wrap>
        <ExplorePanel />
      </Wrap>,
    );
    expect(screen.getByTestId('explore-subtab-discover')).toBeInTheDocument();
    expect(screen.getByTestId('explore-subtab-installed')).toBeInTheDocument();
    expect(screen.getByTestId('explore-subtab-my-uploads')).toBeInTheDocument();
  });

  it('renders sidebar + grid + detail placeholder on Discover tab', async () => {
    const Wrap = await loadWrap();
    const { ExplorePanel } = await import('../explore-panel');
    render(
      <Wrap>
        <ExplorePanel />
      </Wrap>,
    );
    expect(screen.getByTestId('explore-sidebar')).toBeInTheDocument();
    expect(screen.getByTestId('explore-grid')).toBeInTheDocument();
    expect(screen.getByTestId('explore-detail-placeholder')).toBeInTheDocument();
  });

  it('clicking Installed sub-tab shows empty state', async () => {
    const Wrap = await loadWrap();
    const { ExplorePanel } = await import('../explore-panel');
    const user = userEvent.setup();
    render(
      <Wrap>
        <ExplorePanel />
      </Wrap>,
    );
    await user.click(screen.getByTestId('explore-subtab-installed'));
    expect(screen.getByTestId('explore-grid-empty')).toBeInTheDocument();
    expect(screen.getByText(/Nothing installed yet/i)).toBeInTheDocument();
  });

  it('renders + Upload CTA', async () => {
    const Wrap = await loadWrap();
    const { ExplorePanel } = await import('../explore-panel');
    render(
      <Wrap>
        <ExplorePanel />
      </Wrap>,
    );
    expect(screen.getByTestId('explore-upload-cta')).toBeInTheDocument();
  });

  it('clicking + Upload opens the UploadDialog', async () => {
    const Wrap = await loadWrap();
    const { ExplorePanel } = await import('../explore-panel');
    const user = userEvent.setup();
    render(
      <Wrap>
        <ExplorePanel />
      </Wrap>,
    );
    await user.click(screen.getByTestId('explore-upload-cta'));
    // Dialog renders step-source (no source prefilled)
    await waitFor(() => expect(screen.getByTestId('upload-step-source')).toBeInTheDocument());
  });
});
