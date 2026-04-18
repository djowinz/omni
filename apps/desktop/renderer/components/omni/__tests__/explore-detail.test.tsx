/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import type { ArtifactDetail } from '../../../lib/share-types';

const FIXTURE: ArtifactDetail = {
  artifact_id: 'art-1',
  kind: 'theme',
  manifest: { name: 'Neon Demo' },
  content_hash: 'h',
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  author_pubkey: 'pk',
  author_fingerprint_hex: 'aabbcc',
  installs: 42,
  reports: 0,
  created_at: 0,
  updated_at: 0,
  status: 'published',
};

// Dynamically import PreviewContextProvider after vi.resetModules() so the
// provider + the usePreview() call inside ExploreDetail bind to the SAME
// Context object (vi.resetModules invalidates the module cache, so a statically
// imported provider would wind up referencing a stale Context instance).
async function loadWrap() {
  const { PreviewContextProvider } = await import('../../../lib/preview-context');
  return function Wrap({ children }: { children: ReactNode }) {
    return <PreviewContextProvider>{children}</PreviewContextProvider>;
  };
}

describe('ExploreDetail', () => {
  beforeEach(() => {
    vi.resetModules();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders placeholder when selectedId is null', async () => {
    vi.doMock('../../../hooks/use-explore-detail', () => ({
      useExploreDetail: () => ({ artifact: null, loading: false, error: null }),
    }));
    const Wrap = await loadWrap();
    const { ExploreDetail } = await import('../explore-detail');
    render(
      <Wrap>
        <ExploreDetail selectedId={null} tab="discover" />
      </Wrap>,
    );
    expect(screen.getByTestId('explore-detail-placeholder')).toBeInTheDocument();
  });

  it('renders skeleton when loading', async () => {
    vi.doMock('../../../hooks/use-explore-detail', () => ({
      useExploreDetail: () => ({ artifact: null, loading: true, error: null }),
    }));
    const Wrap = await loadWrap();
    const { ExploreDetail } = await import('../explore-detail');
    render(
      <Wrap>
        <ExploreDetail selectedId="art-1" tab="discover" />
      </Wrap>,
    );
    expect(screen.getByTestId('explore-detail-skeleton')).toBeInTheDocument();
  });

  it('renders the card with Preview/Install/Fork buttons on Discover', async () => {
    vi.doMock('../../../hooks/use-explore-detail', () => ({
      useExploreDetail: () => ({ artifact: FIXTURE, loading: false, error: null }),
    }));
    const Wrap = await loadWrap();
    const { ExploreDetail } = await import('../explore-detail');
    render(
      <Wrap>
        <ExploreDetail selectedId="art-1" tab="discover" />
      </Wrap>,
    );
    expect(screen.getByText('Preview')).toBeInTheDocument();
    expect(screen.getByText('Install')).toBeInTheDocument();
    expect(screen.getByText('Fork')).toBeInTheDocument();
  });

  it('renders Installed-tab buttons', async () => {
    vi.doMock('../../../hooks/use-explore-detail', () => ({
      useExploreDetail: () => ({ artifact: FIXTURE, loading: false, error: null }),
    }));
    const Wrap = await loadWrap();
    const { ExploreDetail } = await import('../explore-detail');
    render(
      <Wrap>
        <ExploreDetail selectedId="art-1" tab="installed" />
      </Wrap>,
    );
    expect(screen.getByText('Open')).toBeInTheDocument();
    expect(screen.getByText('Uninstall')).toBeInTheDocument();
    expect(screen.getByText('Fork')).toBeInTheDocument();
  });

  it('clicking Install emits a "coming soon" toast (not yet wired)', async () => {
    const toastInfoSpy = vi.fn();
    vi.doMock('../../../lib/toast', () => ({
      toast: { info: toastInfoSpy, success: vi.fn(), error: vi.fn() },
    }));
    vi.doMock('../../../hooks/use-explore-detail', () => ({
      useExploreDetail: () => ({ artifact: FIXTURE, loading: false, error: null }),
    }));
    const Wrap = await loadWrap();
    const { ExploreDetail } = await import('../explore-detail');
    const user = userEvent.setup();
    render(
      <Wrap>
        <ExploreDetail selectedId="art-1" tab="discover" />
      </Wrap>,
    );
    await user.click(screen.getByText('Install'));
    expect(toastInfoSpy).toHaveBeenCalledWith(expect.stringMatching(/sub-spec #016/i));
  });
});
