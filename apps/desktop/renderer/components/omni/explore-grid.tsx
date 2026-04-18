/**
 * ExploreGrid — 3-column responsive grid of ArtifactCard items.
 *
 * Below 48 items, renders all cards as a simple flex wrap. At 48+, switches
 * to @tanstack/react-virtual row-virtualization (3 cards per row × 96px
 * estimated row height). The threshold matches design §2.3.
 *
 * Infinite scroll: a sentinel div at the bottom triggers onLoadMore when it
 * intersects the viewport. react-intersection-observer handles the
 * IntersectionObserver lifecycle.
 */

import { useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useInView } from 'react-intersection-observer';
import type { CachedArtifactDetail } from '../../lib/share-types';
import { ArtifactCard } from './artifact-card';
import { ExploreEmptyState } from './explore-empty-state';

const VIRTUALIZE_THRESHOLD = 48;
const ROW_HEIGHT_PX = 220;
const COLS_PER_ROW = 3;

export interface ExploreGridProps {
  items: CachedArtifactDetail[];
  loading: boolean;
  hasMore: boolean;
  selectedId: string | null;
  onSelect: (artifactId: string) => void;
  onLoadMore: () => void;
  emptyLabel: string;
  emptyHint?: string;
}

export function ExploreGrid({
  items,
  loading,
  hasMore,
  selectedId,
  onSelect,
  onLoadMore,
  emptyLabel,
  emptyHint,
}: ExploreGridProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const { ref: sentinelRef, inView } = useInView({
    root: scrollRef.current,
    threshold: 0.1,
  });

  if (inView && hasMore && !loading) {
    // Fire in-render is usually bad, but useInView keeps a stable ref so we
    // only call once per transition into view; onLoadMore itself is guarded
    // by an in-flight flag in the hook.
    onLoadMore();
  }

  if (!loading && items.length === 0) {
    return <ExploreEmptyState label={emptyLabel} hint={emptyHint} />;
  }

  if (loading && items.length === 0) {
    return (
      <div className="grid h-full grid-cols-3 gap-3 p-4 content-start">
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

  // Below threshold: plain flex wrap — no virtualization overhead.
  if (items.length < VIRTUALIZE_THRESHOLD) {
    return (
      <div ref={scrollRef} data-testid="explore-grid" className="h-full overflow-y-auto p-4">
        <div className="grid grid-cols-3 gap-3 content-start">
          {items.map((item) => (
            <ArtifactCard
              key={item.artifact_id}
              artifact={item}
              onClick={() => onSelect(item.artifact_id)}
              data-selected={selectedId === item.artifact_id ? 'true' : 'false'}
            />
          ))}
        </div>
        {hasMore ? (
          <div ref={sentinelRef} data-testid="explore-grid-sentinel" className="h-8" />
        ) : null}
      </div>
    );
  }

  // Virtualized path (hit at 48+ items): group into rows of 3.
  const rows: CachedArtifactDetail[][] = [];
  for (let i = 0; i < items.length; i += COLS_PER_ROW) {
    rows.push(items.slice(i, i + COLS_PER_ROW));
  }

  return (
    <VirtualGrid
      rows={rows}
      selectedId={selectedId}
      onSelect={onSelect}
      hasMore={hasMore}
      sentinelRef={sentinelRef}
    />
  );
}

interface VirtualGridProps {
  rows: CachedArtifactDetail[][];
  selectedId: string | null;
  onSelect: (id: string) => void;
  hasMore: boolean;
  sentinelRef: (node: Element | null) => void;
}

function VirtualGrid({ rows, selectedId, onSelect, hasMore, sentinelRef }: VirtualGridProps) {
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
              className="grid grid-cols-3 gap-3 pb-3"
            >
              {row.map((item) => (
                <ArtifactCard
                  key={item.artifact_id}
                  artifact={item}
                  onClick={() => onSelect(item.artifact_id)}
                  data-selected={selectedId === item.artifact_id ? 'true' : 'false'}
                />
              ))}
            </div>
          );
        })}
      </div>
      {hasMore ? (
        <div ref={sentinelRef} data-testid="explore-grid-sentinel" className="h-8" />
      ) : null}
    </div>
  );
}
