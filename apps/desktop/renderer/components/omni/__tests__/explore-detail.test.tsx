/// <reference types="@testing-library/jest-dom/vitest" />

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ExploreDetail } from '../explore-detail';

const mockSetSelectedId = vi.fn();

vi.mock('../../../hooks/use-explore-filters', () => ({
  useExploreFilters: () => ({
    tab: 'discover',
    kind: 'all',
    sort: 'new',
    tags: [],
    q: '',
    selectedId: 'a1',
    setTab: vi.fn(),
    setKind: vi.fn(),
    setSort: vi.fn(),
    setTags: vi.fn(),
    setQ: vi.fn(),
    setSelectedId: mockSetSelectedId,
  }),
}));

vi.mock('../../../hooks/use-explore-detail', () => ({
  useExploreDetail: () => ({
    artifact: {
      artifact_id: 'a1',
      kind: 'bundle',
      manifest: { name: 'Full Telemetry', description: 'Complete telemetry suite.', version: '4.2.0', license: 'Apache-2.0', tags: ['gaming', 'fps', 'racing'] },
      content_hash: 'sha256:abc',
      r2_url: '',
      thumbnail_url: '',
      author_pubkey: 'aa'.repeat(32),
      author_fingerprint_hex: 'aabbccdd',
      installs: 3241,
      reports: 0,
      created_at: 0,
      updated_at: 1712601600,
      status: 'active',
      author_display_name: 'djowinz',
    },
    loading: false,
    error: null,
  }),
}));

// Stub out share-ws + omni-state + identity + preview hooks the same way
// the existing detail test does (read existing file before authoring).
vi.mock('../../../hooks/use-share-ws', () => ({ useShareWs: () => ({ send: vi.fn() }) }));
vi.mock('../../../hooks/use-omni-state', () => ({
  useOmniState: () => ({ state: { overlays: [] }, dispatch: vi.fn() }),
}));
vi.mock('../../../lib/identity-context', () => ({ useIdentity: () => ({ identity: null }) }));
vi.mock('../../../lib/preview-context', () => ({ usePreview: () => ({ setPreview: vi.fn() }) }));

describe('ExploreDetail', () => {
  it('renders the kind icon, name, and kind label in the header', () => {
    render(<ExploreDetail selectedId="a1" tab="discover" />);
    expect(screen.getByText('Full Telemetry')).toBeInTheDocument();
    expect(screen.getByText('Bundle')).toBeInTheDocument();
  });

  it('renders the description in the body', () => {
    render(<ExploreDetail selectedId="a1" tab="discover" />);
    expect(screen.getByText('Complete telemetry suite.')).toBeInTheDocument();
  });

  it('renders all four stats in a 2x2 grid', () => {
    render(<ExploreDetail selectedId="a1" tab="discover" />);
    expect(screen.getByText('Installs')).toBeInTheDocument();
    expect(screen.getByText('3,241')).toBeInTheDocument();
    expect(screen.getByText('Version')).toBeInTheDocument();
    expect(screen.getByText('4.2.0')).toBeInTheDocument();
    expect(screen.getByText('License')).toBeInTheDocument();
    expect(screen.getByText('Apache-2.0')).toBeInTheDocument();
  });

  it('renders tag pills', () => {
    render(<ExploreDetail selectedId="a1" tab="discover" />);
    expect(screen.getByText('gaming')).toBeInTheDocument();
    expect(screen.getByText('fps')).toBeInTheDocument();
    expect(screen.getByText('racing')).toBeInTheDocument();
  });

  it('clicking the close button calls setSelectedId(null)', async () => {
    render(<ExploreDetail selectedId="a1" tab="discover" />);
    await userEvent.click(screen.getByTestId('explore-detail-close'));
    expect(mockSetSelectedId).toHaveBeenCalledWith(null);
  });

  it('renders Preview, Install, and Fork actions in the footer', () => {
    render(<ExploreDetail selectedId="a1" tab="discover" />);
    expect(screen.getByRole('button', { name: /preview/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^install$/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /fork/i })).toBeInTheDocument();
  });
});
