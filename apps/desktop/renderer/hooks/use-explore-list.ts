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
import { debugLog } from '../lib/debug-log';
import type { CachedArtifactDetail, ExplorerListParams, ShareWsError } from '../lib/share-types';

export interface ExploreListFilters {
  tab: 'discover' | 'installed' | 'my-uploads';
  kind: 'theme' | 'bundle' | 'all';
  sort: 'new' | 'installs' | 'name';
  tags: string[];
  q: string;
  /** Optional 64-hex pubkey filter — used by My Uploads to scope the list. */
  authorPubkey?: string;
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
    author_pubkey: filters.authorPubkey,
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
    effectiveFilters.authorPubkey,
  ]);

  const doFetch = useCallback(
    async (cursor: string | null, append: boolean) => {
      debugLog('[useExploreList] doFetch enter', { cursor, append, tab: effectiveFilters.tab });
      if (inFlight.current) {
        debugLog('[useExploreList] skipped: already in flight');
        return;
      }
      inFlight.current = true;
      setLoading(true);
      setError(null);
      try {
        if (effectiveFilters.tab === 'installed') {
          // Installed tab deferred to #016 — return empty.
          debugLog('[useExploreList] installed tab → returning empty (deferred to #016)');
          setItems((prev) => (append ? prev : []));
          setNextCursor(null);
          return;
        }
        if (effectiveFilters.tab === 'my-uploads' && !effectiveFilters.authorPubkey) {
          // My Uploads without a pubkey can't produce results — return empty,
          // let the caller set authorPubkey after identity.show resolves.
          debugLog('[useExploreList] my-uploads tab with no authorPubkey → empty until identity.show lands');
          setItems((prev) => (append ? prev : []));
          setNextCursor(null);
          return;
        }
        const params = toListParams(effectiveFilters, cursor);
        debugLog('[useExploreList] sending explorer.list', params);
        const resp = await send('explorer.list', params);
        debugLog('[useExploreList] raw response', resp);
        debugLog(
          '[useExploreList] items=',
          resp.items?.length,
          'next_cursor=',
          resp.next_cursor,
        );
        setItems((prev) => (append ? [...prev, ...resp.items] : [...resp.items]));
        setNextCursor(resp.next_cursor);
        debugLog('[useExploreList] state updated with', resp.items.length, 'items');
      } catch (err) {
        console.error('[useExploreList] FAILED', err);
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
