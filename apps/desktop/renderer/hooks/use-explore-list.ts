/**
 * useExploreList — fetch + paginate explorer.list responses.
 *
 * Runs a fresh query whenever the filter args change. Debounces the `q`
 * (search) field at 250ms (per design §2.5). `loadMore()` appends the next
 * page using the stashed next_cursor; concurrent calls are de-duplicated.
 *
 * Installed + My-Uploads tabs return empty synchronously in Wave 3b —
 * their data sources (install registry, upload.list) aren't owned by #014
 * and are deferred to #015/#016.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useDebounce } from 'use-debounce';
import { useShareWs } from './use-share-ws';
import type { CachedArtifactDetail, ExplorerListParams, ShareWsError } from '../lib/share-types';

export interface ExploreListFilters {
  tab: 'discover' | 'installed' | 'my-uploads';
  kind: 'theme' | 'bundle' | 'all';
  sort: 'new' | 'installs' | 'name';
  tags: string[];
  q: string;
}

export interface ExploreListState {
  items: CachedArtifactDetail[];
  nextCursor: string | null;
  loading: boolean;
  error: ShareWsError | null;
  loadMore: () => Promise<void>;
  refetch: () => Promise<void>;
}

const PAGE_SIZE = 48;

function toListParams(filters: ExploreListFilters, cursor: string | null): ExplorerListParams {
  return {
    kind: filters.kind,
    sort: filters.sort,
    tags: filters.tags,
    cursor,
    limit: PAGE_SIZE,
  };
}

export function useExploreList(filters: ExploreListFilters): ExploreListState {
  const { send } = useShareWs();
  const [items, setItems] = useState<CachedArtifactDetail[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<ShareWsError | null>(null);
  const inFlight = useRef(false);

  const [debouncedQ] = useDebounce(filters.q, 250);
  const effectiveFilters: ExploreListFilters = { ...filters, q: debouncedQ };
  const filtersKey = JSON.stringify([
    effectiveFilters.tab,
    effectiveFilters.kind,
    effectiveFilters.sort,
    effectiveFilters.tags,
    effectiveFilters.q,
  ]);

  const doFetch = useCallback(
    async (cursor: string | null, append: boolean) => {
      if (inFlight.current) return;
      inFlight.current = true;
      setLoading(true);
      setError(null);
      try {
        if (effectiveFilters.tab !== 'discover') {
          // Installed + my-uploads deferred to sibling sub-specs — return empty.
          // Use functional setter so we don't need `items` in the dep array.
          setItems((prev) => (append ? prev : []));
          setNextCursor(null);
          return;
        }
        const params = toListParams(effectiveFilters, cursor);
        const resp = await send('explorer.list', params);
        setItems((prev) => (append ? [...prev, ...resp.items] : [...resp.items]));
        setNextCursor(resp.next_cursor);
      } catch (err) {
        setError(err as ShareWsError);
      } finally {
        setLoading(false);
        inFlight.current = false;
      }
    },
    // filtersKey is a stringified snapshot of the relevant filter fields —
    // captures all content we depend on without re-binding on every render.
    [send, filtersKey],
  );

  useEffect(() => {
    void doFetch(null, false);
  }, [doFetch]);

  const loadMore = useCallback(async () => {
    if (!nextCursor) return;
    await doFetch(nextCursor, true);
  }, [nextCursor, doFetch]);

  const refetch = useCallback(async () => {
    await doFetch(null, false);
  }, [doFetch]);

  return { items, nextCursor, loading, error, loadMore, refetch };
}
