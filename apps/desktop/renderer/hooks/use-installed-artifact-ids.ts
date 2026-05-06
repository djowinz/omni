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
  /** Last fetch error, if any. Cleared on the next successful refetch. */
  error: string | null;
  refetch: () => Promise<void>;
}

const FETCH_TIMEOUT_MS = 10_000;

export function useInstalledArtifacts(): InstalledArtifactsState {
  const { send } = useShareWs();
  const [entries, setEntries] = useState<InstalledEntryRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchOnce = useCallback(async () => {
    setLoading(true);
    setError(null);
    // Race the WS round-trip against a hard timeout. If the host binary
    // doesn't have the `workspace.listInstalled` handler (stale
    // build / wrong process / dispatch fall-through) the request never
    // completes — without this timeout the grid spins forever on a
    // skeleton with no clue why. The timeout surfaces the failure as a
    // visible error in the empty-state UI.
    const sendPromise = send('workspace.listInstalled', {});
    const timeoutPromise = new Promise<never>((_, reject) => {
      setTimeout(
        () =>
          reject(
            new Error(
              `workspace.listInstalled timed out after ${FETCH_TIMEOUT_MS}ms — likely a stale host binary missing the handler`,
            ),
          ),
        FETCH_TIMEOUT_MS,
      );
    });
    console.log('[useInstalledArtifacts] sending workspace.listInstalled');
    try {
      const resp = (await Promise.race([sendPromise, timeoutPromise])) as Awaited<
        typeof sendPromise
      >;
      console.log(
        '[useInstalledArtifacts] received',
        resp.params.entries.length,
        'entries',
      );
      setEntries(resp.params.entries);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      console.warn('[useInstalledArtifacts] fetch failed:', message, err);
      setEntries([]);
      setError(message);
    } finally {
      setLoading(false);
    }
  }, [send]);

  useEffect(() => {
    void fetchOnce();
  }, [fetchOnce]);

  // Cross-surface sync: any component that triggers an install/uninstall
  // (Explore panel detail, hover button, header submenu, editor fork CTA)
  // dispatches one of these window events on success. Every consumer of
  // this hook (and there are several — header.tsx, editor-panel.tsx,
  // explore-panel.tsx) needs to refetch so the registry-derived state
  // (badges, read-only overlay flag, hover affordance) stays in sync
  // without one component having to know about all the others.
  useEffect(() => {
    const onChanged = () => {
      void fetchOnce();
    };
    window.addEventListener('omni:artifact-installed', onChanged);
    window.addEventListener('omni:artifact-uninstalled', onChanged);
    return () => {
      window.removeEventListener('omni:artifact-installed', onChanged);
      window.removeEventListener('omni:artifact-uninstalled', onChanged);
    };
  }, [fetchOnce]);

  const ids = useMemo(() => new Set(entries.map((e) => e.artifact_id)), [entries]);

  return { ids, entries, loading, error, refetch: fetchOnce };
}

/** @deprecated use `useInstalledArtifacts` for the full row shape. */
export const useInstalledArtifactIds = useInstalledArtifacts;
