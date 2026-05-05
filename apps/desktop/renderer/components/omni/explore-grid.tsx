/**
 * ExploreGrid — grid panel with toolbar (search + sort) + virtualized 3-col card grid.
 *
 * Per share-explorer-redesign spec §3.3:
 *   - Toolbar h-10 above the existing virtualized grid.
 *   - Grid keeps 3 columns always (does not reflow when detail pane closes).
 *   - Cards get optional onPreview/onInstall handlers for hover overlay.
 *   - Empty state branches on whether `q` is set vs empty.
 */

import { useEffect, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useInView } from 'react-intersection-observer';
import type { CachedArtifactDetail } from '../../lib/share-types';
import { useExploreFilters } from '../../hooks/use-explore-filters';
import { ArtifactCard } from './artifact-card';
import { SearchInput } from './search-input';
import { SortDropdown } from './sort-dropdown';

const VIRTUALIZE_THRESHOLD = 48;
const ROW_HEIGHT_PX = 240;
const COLS_PER_ROW = 3;

export interface ExploreGridProps {
  items: CachedArtifactDetail[];
  loading: boolean;
  hasMore: boolean;
  selectedId: string | null;
  /**
   * Set of artifact_ids that are currently installed locally. Cards whose
   * artifact_id is in this set render with the green "Installed" kind
   * badge instead of "Theme"/"Bundle". Optional so test fixtures + future
   * consumers that don't have a host registry to query can omit it.
   */
  installedIds?: Set<string>;
  onSelect: (artifactId: string) => void;
  onPreview?: (artifact: CachedArtifactDetail) => void;
  onInstall?: (artifact: CachedArtifactDetail) => void;
  onLoadMore: () => void;
}

export function ExploreGrid({
  items,
  loading,
  hasMore,
  selectedId,
  installedIds,
  onSelect,
  onPreview,
  onInstall,
  onLoadMore,
}: ExploreGridProps) {
  const { q, sort, setQ, setSort, setKind, setTags } = useExploreFilters();

  const clearFilters = () => {
    setKind('all');
    setTags([]);
    setQ('');
  };

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Toolbar — h-16 to align horizontally with the detail-pane header. */}
      <div className="flex h-16 flex-shrink-0 items-center gap-3 border-b border-[#27272A] bg-[#18181B] px-4 py-3.5">
        <SearchInput value={q} onChange={setQ} />
        <div className="ml-auto">
          <SortDropdown value={sort} onChange={setSort} />
        </div>
      </div>

      {/* Body */}
      <div className="flex-1 overflow-hidden">
        <GridBody
          items={items}
          loading={loading}
          hasMore={hasMore}
          selectedId={selectedId}
          installedIds={installedIds}
          onSelect={onSelect}
          onPreview={onPreview}
          onInstall={onInstall}
          onLoadMore={onLoadMore}
          q={q}
          onClearFilters={clearFilters}
        />
      </div>
    </div>
  );
}

interface GridBodyProps {
  items: CachedArtifactDetail[];
  loading: boolean;
  hasMore: boolean;
  selectedId: string | null;
  installedIds?: Set<string>;
  onSelect: (artifactId: string) => void;
  onPreview?: (artifact: CachedArtifactDetail) => void;
  onInstall?: (artifact: CachedArtifactDetail) => void;
  onLoadMore: () => void;
  q: string;
  onClearFilters: () => void;
}

function GridBody({
  items,
  loading,
  hasMore,
  selectedId,
  installedIds,
  onSelect,
  onPreview,
  onInstall,
  onLoadMore,
  q,
  onClearFilters,
}: GridBodyProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const { ref: sentinelRef, inView } = useInView({ root: scrollRef.current, threshold: 0.1 });

  useEffect(() => {
    if (inView && hasMore && !loading) onLoadMore();
  }, [inView, hasMore, loading, onLoadMore]);

  if (!loading && items.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-center">
        <div className="flex flex-col gap-2 text-xs text-zinc-500">
          {q.length > 0 ? (
            <p>
              No results for <span className="font-medium text-[#A1A1AA]">"{q}"</span>.
            </p>
          ) : (
            <p>No themes or bundles match these filters.</p>
          )}
          <button
            onClick={onClearFilters}
            className="text-[11px] text-[#00D9FF] hover:underline"
          >
            Clear filters
          </button>
        </div>
      </div>
    );
  }

  if (loading && items.length === 0) {
    return (
      <div className="grid grid-cols-3 content-start gap-3 p-4">
        {Array.from({ length: 6 }).map((_, i) => (
          <div
            key={i}
            data-testid="explore-grid-skeleton"
            className="aspect-video animate-pulse rounded-md bg-[#27272A]"
          />
        ))}
      </div>
    );
  }

  if (items.length < VIRTUALIZE_THRESHOLD) {
    return (
      <div ref={scrollRef} data-testid="explore-grid" className="h-full overflow-y-auto p-4">
        <div className="grid grid-cols-3 content-start gap-3.5">
          {items.map((item) => (
            <ArtifactCard
              key={item.artifact_id}
              artifact={item}
              installed={installedIds?.has(item.artifact_id) ?? false}
              onClick={() => onSelect(item.artifact_id)}
              onPreview={onPreview ? () => onPreview(item) : undefined}
              onInstall={onInstall ? () => onInstall(item) : undefined}
              data-selected={selectedId === item.artifact_id ? 'true' : 'false'}
            />
          ))}
        </div>
        {hasMore && <div ref={sentinelRef} data-testid="explore-grid-sentinel" className="h-8" />}
      </div>
    );
  }

  // Virtualized path
  const rows: CachedArtifactDetail[][] = [];
  for (let i = 0; i < items.length; i += COLS_PER_ROW) {
    rows.push(items.slice(i, i + COLS_PER_ROW));
  }

  return (
    <VirtualGrid
      rows={rows}
      selectedId={selectedId}
      installedIds={installedIds}
      onSelect={onSelect}
      onPreview={onPreview}
      onInstall={onInstall}
      hasMore={hasMore}
      sentinelRef={sentinelRef}
    />
  );
}

interface VirtualGridProps {
  rows: CachedArtifactDetail[][];
  selectedId: string | null;
  installedIds?: Set<string>;
  onSelect: (id: string) => void;
  onPreview?: (artifact: CachedArtifactDetail) => void;
  onInstall?: (artifact: CachedArtifactDetail) => void;
  hasMore: boolean;
  sentinelRef: (node: Element | null) => void;
}

function VirtualGrid({
  rows,
  selectedId,
  installedIds,
  onSelect,
  onPreview,
  onInstall,
  hasMore,
  sentinelRef,
}: VirtualGridProps) {
  const parentRef = useRef<HTMLDivElement | null>(null);

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT_PX,
    overscan: 4,
  });

  return (
    <div ref={parentRef} data-testid="explore-grid" className="h-full overflow-y-auto p-4">
      <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
        {rowVirtualizer.getVirtualItems().map((virtualRow) => {
          const row = rows[virtualRow.index]!;
          return (
            <div
              key={virtualRow.index}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: ROW_HEIGHT_PX,
                transform: `translateY(${virtualRow.start}px)`,
              }}
              className="grid grid-cols-3 gap-3.5 pb-3.5"
            >
              {row.map((item) => (
                <ArtifactCard
                  key={item.artifact_id}
                  artifact={item}
                  installed={installedIds?.has(item.artifact_id) ?? false}
                  onClick={() => onSelect(item.artifact_id)}
                  onPreview={onPreview ? () => onPreview(item) : undefined}
                  onInstall={onInstall ? () => onInstall(item) : undefined}
                  data-selected={selectedId === item.artifact_id ? 'true' : 'false'}
                />
              ))}
            </div>
          );
        })}
      </div>
      {hasMore && <div ref={sentinelRef} data-testid="explore-grid-sentinel" className="h-8" />}
    </div>
  );
}
