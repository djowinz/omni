import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ExploreSidebar } from '../explore-sidebar';

// Mock the filters + vocab hooks (existing test pattern in this directory)
const mockSetTab = vi.fn();
const mockSetKind = vi.fn();
const mockSetTags = vi.fn();

vi.mock('../../../hooks/use-explore-filters', () => ({
  useExploreFilters: () => ({
    tab: 'discover',
    kind: 'all',
    sort: 'new',
    tags: [],
    q: '',
    selectedId: null,
    setTab: mockSetTab,
    setKind: mockSetKind,
    setSort: vi.fn(),
    setTags: mockSetTags,
    setQ: vi.fn(),
    setSelectedId: vi.fn(),
  }),
}));

vi.mock('../../../hooks/use-config-vocab', () => ({
  useConfigVocab: () => ({ tags: ['dark', 'gaming', 'minimal'], loading: false }),
}));

describe('ExploreSidebar', () => {
  it('renders three tabs (Discover, Installed, My Uploads)', () => {
    render(<ExploreSidebar />);
    expect(screen.getByText('Discover')).toBeInTheDocument();
    expect(screen.getByText('Installed')).toBeInTheDocument();
    expect(screen.getByText('My Uploads')).toBeInTheDocument();
  });

  it('clicking a tab calls setTab', async () => {
    render(<ExploreSidebar />);
    await userEvent.click(screen.getByTestId('explore-sidebar-tab-installed'));
    expect(mockSetTab).toHaveBeenCalledWith('installed');
  });

  it('renders three Type rows (All, Themes, Bundles)', () => {
    render(<ExploreSidebar />);
    expect(screen.getByText('All')).toBeInTheDocument();
    expect(screen.getByText('Themes')).toBeInTheDocument();
    expect(screen.getByText('Bundles')).toBeInTheDocument();
  });

  it('clicking a Type row calls setKind', async () => {
    render(<ExploreSidebar />);
    await userEvent.click(screen.getByTestId('explore-sidebar-kind-theme'));
    expect(mockSetKind).toHaveBeenCalledWith('theme');
  });

  it('renders tag pills from config.vocab', () => {
    render(<ExploreSidebar />);
    expect(screen.getByRole('button', { name: 'dark' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'gaming' })).toBeInTheDocument();
  });

  it('clicking a tag pill toggles it via setTags', async () => {
    render(<ExploreSidebar />);
    await userEvent.click(screen.getByRole('button', { name: 'dark' }));
    expect(mockSetTags).toHaveBeenCalledWith(['dark']);
  });

  it('does NOT render a sort radio (moved to grid toolbar)', () => {
    render(<ExploreSidebar />);
    expect(screen.queryByText(/sort/i)).not.toBeInTheDocument();
  });
});
