/**
 * useInstalledDetails — live-fetch display-only fields (thumbnail, install
 * count, tags, author display name) for locally-installed artifacts.
 *
 * The local install registry stores ONLY local install state — display
 * fields like `thumbnail_url` and `installs` come from the worker. Caching
 * them at install time would mean the Installed-tab card shows a stale
 * snapshot of the install count (an artifact that's gone from 12 → 1.2k
 * installs would still render "12"). So we fetch fresh on mount via
 * `explorer.batchGet` (max 50 ids per request, capped server-side).
 *
 * Lifecycle:
 *   - Fires when `entries` changes by id-set.
 *   - Network failure / oversize batch → empty `byId` map, error string set;
 *     cards stay with the gradient placeholder + "0" install badge so the
 *     tab still works offline.
 *
 * Pagination: not needed for v1 — most users have <50 installed items. If
 * we exceed the cap we'd need to chunk; for now we slice and warn.
 */

import { useEffect, useMemo, useState } from 'react';
import { useShareWs } from './use-share-ws';
import type { ArtifactDetail, InstalledEntryRow } from '../lib/share-types';

const BATCH_CAP = 50;

export interface InstalledDetailsState {
  /** Map keyed by artifact_id. Missing keys = no live data (placeholder). */
  byId: Map<string, ArtifactDetail>;
  loading: boolean;
  error: string | null;
}

export function useInstalledDetails(entries: InstalledEntryRow[]): InstalledDetailsState {
  const { send } = useShareWs();
  const [byId, setById] = useState<Map<string, ArtifactDetail>>(new Map());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Stable id-set key so the effect doesn't refire on registry-row reorders
  // or unrelated field churn.
  const idsKey = useMemo(
    () =>
      entries
        .map((e) => e.artifact_id)
        .sort()
        .join(','),
    [entries],
  );

  useEffect(() => {
    if (!idsKey) {
      setById(new Map());
      setError(null);
      setLoading(false);
      return;
    }
    const ids = idsKey.split(',');
    if (ids.length > BATCH_CAP) {
      console.warn(
        `[useInstalledDetails] ${ids.length} installed entries exceeds batch cap of ${BATCH_CAP}; truncating`,
      );
    }
    const requestIds = ids.slice(0, BATCH_CAP);
    let cancelled = false;
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const resp = await send('explorer.batchGet', { ids: requestIds });
        if (cancelled) return;
        const next = new Map<string, ArtifactDetail>();
        for (const a of resp.artifacts) {
          next.set(a.artifact_id, a);
        }
        setById(next);
      } catch (err) {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        console.warn('[useInstalledDetails] batch fetch failed:', message);
        setById(new Map());
        setError(message);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [idsKey, send]);

  return { byId, loading, error };
}
