/**
 * useInstalledArtifactIds — fetch the set of artifact_ids currently installed
 * locally (bundles + themes registries) so explorer cards can badge
 * "Installed" without scanning the filesystem from the renderer.
 *
 * Wire shape: `workspace.listInstalled` (`crates/host/src/share/ws_messages.rs
 * handle_list_installed`) returns a flat `string[]`. We Set-ify it for O(1)
 * `has()` lookups in the grid render hot path.
 *
 * Lifecycle:
 *   - Fetches once on mount.
 *   - Exposes `refetch()` so callers can re-pull after side-channel mutations
 *     (`explorer.install` success, future `explorer.uninstall`). The explore
 *     panel calls this from its install success handlers.
 *
 * Errors are coalesced to an empty Set + console.warn — a transient WS hiccup
 * shouldn't break the grid render. Worst case the user briefly sees a card
 * un-badged until the next refetch.
 */

import { useCallback, useEffect, useState } from 'react';
import { useShareWs } from './use-share-ws';

export interface InstalledArtifactIdsState {
  ids: Set<string>;
  loading: boolean;
  refetch: () => Promise<void>;
}

export function useInstalledArtifactIds(): InstalledArtifactIdsState {
  const { send } = useShareWs();
  const [ids, setIds] = useState<Set<string>>(() => new Set());
  const [loading, setLoading] = useState(true);

  const fetchOnce = useCallback(async () => {
    setLoading(true);
    try {
      const resp = await send('workspace.listInstalled', {});
      setIds(new Set(resp.params.artifact_ids));
    } catch (err) {
      console.warn('[useInstalledArtifactIds] fetch failed', err);
      setIds(new Set());
    } finally {
      setLoading(false);
    }
  }, [send]);

  useEffect(() => {
    void fetchOnce();
  }, [fetchOnce]);

  return { ids, loading, refetch: fetchOnce };
}
