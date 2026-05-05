/**
 * useInstalledArtifacts — fetch the local installed-artifacts registry
 * (bundles + themes) for two consumers in the explorer:
 *
 *   1. Discover / My-Uploads grid badge: cards whose `artifact_id` is in
 *      `ids` render the green 'Installed' kind label instead of
 *      'Bundle'/'Theme'. The Set is built once per refetch for O(1)
 *      `has()` lookups in the render hot path.
 *
 *   2. Installed sub-tab grid: ExplorePanel uses `entries` directly as
 *      grid items (mapped into the CachedArtifactDetail shape the cards
 *      expect). This means the Installed tab works offline and keeps
 *      showing artifacts even when the worker has tombstoned the row.
 *
 * Wire shape: `workspace.listInstalled` (host
 * `handle_list_installed`) returns `{ entries: InstalledEntryRow[] }`.
 *
 * Lifecycle:
 *   - Fetches once on mount.
 *   - `refetch()` re-pulls after side-channel mutations (install / future
 *     uninstall). ExplorePanel calls it after install success.
 *
 * Errors coalesce to empty + console.warn so a transient WS hiccup
 * doesn't break grid render.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useShareWs } from './use-share-ws';
import type { InstalledEntryRow } from '../lib/share-types';

export interface InstalledArtifactsState {
  /** O(1) artifact_id lookup for the Discover / My-Uploads installed badge. */
  ids: Set<string>;
  /** Full registry rows for the Installed sub-tab grid. */
  entries: InstalledEntryRow[];
  loading: boolean;
  refetch: () => Promise<void>;
}

export function useInstalledArtifacts(): InstalledArtifactsState {
  const { send } = useShareWs();
  const [entries, setEntries] = useState<InstalledEntryRow[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchOnce = useCallback(async () => {
    setLoading(true);
    try {
      const resp = await send('workspace.listInstalled', {});
      setEntries(resp.params.entries);
    } catch (err) {
      console.warn('[useInstalledArtifacts] fetch failed', err);
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [send]);

  useEffect(() => {
    void fetchOnce();
  }, [fetchOnce]);

  const ids = useMemo(() => new Set(entries.map((e) => e.artifact_id)), [entries]);

  return { ids, entries, loading, refetch: fetchOnce };
}

/** @deprecated use `useInstalledArtifacts` for the full row shape. */
export const useInstalledArtifactIds = useInstalledArtifacts;
