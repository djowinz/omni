import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ExplorePanel } from '../explore-panel';

vi.mock('../../../hooks/use-explore-filters', () => ({
  useExploreFilters: () => ({
    tab: 'discover',
    kind: 'all',
    sort: 'new',
    tags: [],
    q: '',
    selectedId: null,
    setTab: vi.fn(),
    setKind: vi.fn(),
    setSort: vi.fn(),
    setTags: vi.fn(),
    setQ: vi.fn(),
    setSelectedId: vi.fn(),
  }),
}));

vi.mock('../../../hooks/use-explore-list', () => ({
  useExploreList: () => ({ items: [], loading: false, nextCursor: null, error: null, loadMore: vi.fn(), refetch: vi.fn() }),
}));
vi.mock('../../../hooks/use-my-uploads', () => ({
  useMyUploads: () => ({ items: [], loading: false, nextCursor: null, identityPubkey: null, loadMore: vi.fn() }),
}));
// useShareWs exposes both send (request-response) and subscribe (push events).
// UploadDialog's use-upload-machine calls subscribe('upload.packProgress', ...)
// with [ws] in the dependency array. We return a STABLE object from the mock —
// a new object literal each render would cause the [ws] effect to re-fire on
// every render, producing an infinite loop → OOM. The stable object is captured
// inside the factory closure (vi.mock factories run once at module-load time).
vi.mock('../../../hooks/use-share-ws', () => {
  const stableWs = { send: vi.fn(), subscribe: vi.fn(() => () => {}) };
  return { useShareWs: () => stableWs };
});
vi.mock('../../../hooks/use-config-vocab', () => ({ useConfigVocab: () => ({ tags: [], loading: false }) }));
vi.mock('../../../hooks/use-omni-state', () => ({ useOmniState: () => ({ state: { overlays: [] }, dispatch: vi.fn() }) }));
vi.mock('../../../lib/identity-context', () => ({ useIdentity: () => ({ identity: null }) }));
vi.mock('../../../lib/preview-context', () => ({ usePreview: () => ({ setPreview: vi.fn() }) }));

describe('ExplorePanel', () => {
  it('renders the panel header (Compass + Explore + Upload CTA)', () => {
    render(<ExplorePanel />);
    expect(screen.getByText('Explore')).toBeInTheDocument();
    expect(screen.getByTestId('explore-upload-cta')).toBeInTheDocument();
  });

  it('renders the sidebar', () => {
    render(<ExplorePanel />);
    expect(screen.getByTestId('explore-sidebar')).toBeInTheDocument();
  });

  it('renders the grid (toolbar + body)', () => {
    render(<ExplorePanel />);
    expect(screen.getByRole('searchbox')).toBeInTheDocument();
    expect(screen.getByRole('combobox')).toBeInTheDocument();
  });

  it('does NOT mount the detail pane when selectedId is null', () => {
    render(<ExplorePanel />);
    expect(screen.queryByTestId('explore-detail')).not.toBeInTheDocument();
  });
});
