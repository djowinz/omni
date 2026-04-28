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
  author_fingerprint_hex: '',
  name: 'Demo',
  kind: 'theme',
  tags: [],
  installs: 0,
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  created_at: 0,
  updated_at: 1700000000,
  author_display_name: null,
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
    // ExploreDetail now calls useOmniState (existingNames for ForkDialog) +
    // useIdentity (selfHandle for ForkDialog). Stub both so tests that render
    // ExploreDetail don't require OmniProvider / IdentityContextProvider.
    vi.doMock('../../../hooks/use-omni-state', () => ({
      useOmniState: () => ({
        state: { overlays: [] },
        dispatch: vi.fn(),
      }),
    }));
    vi.doMock('../../../lib/identity-context', () => ({
      useIdentity: () => ({
        identity: null,
        loading: false,
        is_fresh_install: false,
        first_run_handled: false,
        refresh: vi.fn(),
        markFirstRunHandled: vi.fn(),
      }),
    }));
    // UploadDialog's source picker loads the workspace via useWorkspaceList,
    // which now calls the `workspace.listPublishables` Share-WS RPC (replaced
    // file.list in upload-flow-redesign Wave A0). Stub the `window.omni`
    // bridge that production `useShareWs` reads so the dialog can mount.
    //
    // We do NOT vi.doMock('use-share-ws') because that pattern returns a
    // fresh hook-result object every render, which makes effects with
    // `[ws]` deps re-fire infinitely → OOM.
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(async (msg: { id: string; type: string }) => {
        if (msg.type === 'workspace.listPublishables') {
          return {
            id: msg.id,
            type: 'workspace.listPublishablesResult',
            params: { entries: [] },
          };
        }
        if (msg.type === 'identity.show') {
          return {
            id: msg.id,
            type: 'identity.showResult',
            params: {
              pubkey_hex: 'cc'.repeat(32),
              fingerprint_hex: '',
              fingerprint_emoji: [],
              fingerprint_words: [],
              created_at: 0,
              backed_up: true,
              display_name: null,
              last_backed_up_at: null,
              last_rotated_at: null,
              last_backup_path: null,
            },
          };
        }
        if (msg.type === 'config.vocab') {
          return { id: msg.id, type: 'config.vocabResult', params: { tags: [], version: 0 } };
        }
        throw new Error('unexpected sendShareMessage: ' + msg.type);
      }),
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
    // Dialog renders the new source-picker (no prefilledPath given, so the
    // source picker is the first thing visible inside the dialog content).
    await waitFor(() => expect(screen.getByTestId('upload-dialog-content')).toBeInTheDocument());
    expect(screen.getByTestId('source-picker')).toBeInTheDocument();
  });
});
