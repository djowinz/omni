/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { CachedArtifactDetail } from '../../../lib/share-types';
import { ExploreGrid } from '../explore-grid';

const fixture = (n: number): CachedArtifactDetail[] =>
  Array.from({ length: n }, (_, i) => ({
    artifact_id: `art-${i}`,
    content_hash: `h-${i}`,
    author_pubkey: 'pk',
    name: `Artifact ${i}`,
    kind: 'theme',
    tags: [],
    installs: 0,
    r2_url: `https://x/${i}`,
    thumbnail_url: `https://x/${i}/t`,
    updated_at: 1700000000 + i,
  }));

describe('ExploreGrid', () => {
  it('renders cards for each item', () => {
    render(
      <ExploreGrid
        items={fixture(3)}
        loading={false}
        hasMore={false}
        onSelect={() => {}}
        onLoadMore={() => {}}
        selectedId={null}
        emptyLabel="empty"
      />,
    );
    expect(screen.getByText('Artifact 0')).toBeInTheDocument();
    expect(screen.getByText('Artifact 1')).toBeInTheDocument();
    expect(screen.getByText('Artifact 2')).toBeInTheDocument();
  });

  it('calls onSelect with artifact id when a card is clicked', async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <ExploreGrid
        items={fixture(2)}
        loading={false}
        hasMore={false}
        onSelect={onSelect}
        onLoadMore={() => {}}
        selectedId={null}
        emptyLabel="empty"
      />,
    );
    await user.click(screen.getByText('Artifact 1').closest('[data-testid="artifact-card-grid"]')!);
    expect(onSelect).toHaveBeenCalledWith('art-1');
  });

  it('renders empty state when items is empty and not loading', () => {
    render(
      <ExploreGrid
        items={[]}
        loading={false}
        hasMore={false}
        onSelect={() => {}}
        onLoadMore={() => {}}
        selectedId={null}
        emptyLabel="No artifacts match your filters."
      />,
    );
    expect(screen.getByTestId('explore-grid-empty')).toBeInTheDocument();
    expect(screen.getByText(/No artifacts match your filters/)).toBeInTheDocument();
  });

  it('renders skeleton grid while loading and items empty', () => {
    render(
      <ExploreGrid
        items={[]}
        loading={true}
        hasMore={false}
        onSelect={() => {}}
        onLoadMore={() => {}}
        selectedId={null}
        emptyLabel="empty"
      />,
    );
    const skeletons = screen.getAllByTestId('explore-grid-skeleton');
    expect(skeletons.length).toBeGreaterThanOrEqual(6);
  });

  it('marks selected card with data-selected="true"', () => {
    render(
      <ExploreGrid
        items={fixture(2)}
        loading={false}
        hasMore={false}
        onSelect={() => {}}
        onLoadMore={() => {}}
        selectedId="art-1"
        emptyLabel="empty"
      />,
    );
    const cards = screen.getAllByTestId('artifact-card-grid');
    expect(cards[0]!.getAttribute('data-selected')).toBe('false');
    expect(cards[1]!.getAttribute('data-selected')).toBe('true');
  });
});
