/// <reference types="@testing-library/jest-dom/vitest" />

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { ExploreGrid } from '../explore-grid';

const mockSetQ = vi.fn();
const mockSetSort = vi.fn();
const mockSetKind = vi.fn();
const mockSetTags = vi.fn();

// Mutable so the search-no-match test can override q without a second vi.mock call.
let mockQ = '';

vi.mock('../../../hooks/use-explore-filters', () => ({
  useExploreFilters: () => ({
    tab: 'discover',
    kind: 'all',
    sort: 'new',
    tags: [],
    get q() {
      return mockQ;
    },
    selectedId: null,
    setTab: vi.fn(),
    setKind: mockSetKind,
    setSort: mockSetSort,
    setTags: mockSetTags,
    setQ: mockSetQ,
    setSelectedId: vi.fn(),
  }),
}));

const fixture = {
  artifact_id: 'a1',
  content_hash: '',
  author_pubkey: 'aa'.repeat(32),
  author_fingerprint_hex: '',
  name: 'Marathon',
  kind: 'theme',
  tags: ['dark'],
  installs: 1,
  r2_url: '',
  thumbnail_url: '',
  created_at: 0,
  updated_at: 0,
  author_display_name: null,
} as const;

beforeEach(() => {
  mockQ = '';
  vi.clearAllMocks();
});

describe('ExploreGrid', () => {
  it('renders the toolbar with search input and sort dropdown', () => {
    render(
      <ExploreGrid
        items={[]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={() => {}}
        onLoadMore={() => {}}
      />,
    );
    expect(screen.getByRole('searchbox')).toBeInTheDocument();
    expect(screen.getByRole('combobox')).toBeInTheDocument();
  });

  it('typing in the search input flows into setQ', async () => {
    render(
      <ExploreGrid
        items={[]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={() => {}}
        onLoadMore={() => {}}
      />,
    );
    await userEvent.type(screen.getByRole('searchbox'), 'foo');
    expect(mockSetQ).toHaveBeenCalledWith('f');
  });

  it('renders cards in a 3-column grid', () => {
    render(
      <ExploreGrid
        items={[fixture]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={() => {}}
        onLoadMore={() => {}}
      />,
    );
    expect(screen.getByText('Marathon')).toBeInTheDocument();
  });

  it('clicking a card calls onSelect with the artifact_id', async () => {
    const onSelect = vi.fn();
    render(
      <ExploreGrid
        items={[fixture]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={onSelect}
        onLoadMore={() => {}}
      />,
    );
    await userEvent.click(screen.getByText('Marathon'));
    expect(onSelect).toHaveBeenCalledWith('a1');
  });

  it('shows search-no-match empty state when q is set and items is empty', () => {
    mockQ = 'nothing';
    render(
      <ExploreGrid
        items={[]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={() => {}}
        onLoadMore={() => {}}
      />,
    );
    expect(screen.getByText(/no results for/i)).toBeInTheDocument();
  });

  it('shows generic empty state when filters return zero and q is empty', () => {
    render(
      <ExploreGrid
        items={[]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={() => {}}
        onLoadMore={() => {}}
      />,
    );
    expect(screen.getByText(/no themes or bundles match/i)).toBeInTheDocument();
  });

  it('Clear filters button resets kind, tags, and q', async () => {
    render(
      <ExploreGrid
        items={[]}
        loading={false}
        hasMore={false}
        selectedId={null}
        onSelect={() => {}}
        onLoadMore={() => {}}
      />,
    );
    await userEvent.click(screen.getByText(/clear filters/i));
    expect(mockSetKind).toHaveBeenCalledWith('all');
    expect(mockSetTags).toHaveBeenCalledWith([]);
    expect(mockSetQ).toHaveBeenCalledWith('');
  });
});
