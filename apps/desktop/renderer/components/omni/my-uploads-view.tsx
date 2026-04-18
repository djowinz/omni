/**
 * MyUploadsView — the My Uploads sub-tab content.
 *
 * Identity card pinned at top + grid of own artifacts via useMyUploads
 * (identity.show pubkey → author-filtered explorer.list). Reuses the
 * shipped ExploreGrid for the grid and adds per-tab action slots via
 * artifact-actions helpers.
 */

import { useMyUploads } from '../../hooks/use-my-uploads';
import { useExploreFilters } from '../../hooks/use-explore-filters';
import { IdentitySummaryCard } from './identity-summary-card';
import { ExploreGrid } from './explore-grid';
import { ExploreEmptyState } from './explore-empty-state';

export function MyUploadsView() {
  const state = useMyUploads();
  const filters = useExploreFilters();

  if (state.identityPubkey === null && !state.loading) {
    return (
      <ExploreEmptyState
        label="No identity yet."
        hint="Publish something to create your author identity."
      />
    );
  }

  return (
    <div className="flex h-full w-full flex-col">
      {state.identityPubkey !== null && (
        <div className="p-4">
          <IdentitySummaryCard
            pubkeyHex={state.identityPubkey}
            fingerprintHex=""
            backedUp={false}
          />
        </div>
      )}
      <div className="flex-1 overflow-hidden">
        <ExploreGrid
          items={state.items}
          loading={state.loading}
          hasMore={state.nextCursor !== null}
          selectedId={filters.selectedId}
          onSelect={filters.setSelectedId}
          onLoadMore={state.loadMore}
          emptyLabel="You haven't published anything yet."
          emptyHint="Click + Upload to share your first theme or bundle."
        />
      </div>
    </div>
  );
}
