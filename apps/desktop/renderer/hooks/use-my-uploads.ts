/**
 * useMyUploads — gallery hook for the My Uploads sub-tab.
 *
 * Fetches the editor's own pubkey via identity.show once, then drives
 * useExploreList with tab='my-uploads' + authorPubkey=<own>. Returns the
 * same shape as useExploreList so <MyUploadsView> can render via the
 * existing <ExploreGrid>.
 */

import { useEffect, useState } from 'react';
import { useShareWs } from './use-share-ws';
import { useExploreList, type ExploreListState } from './use-explore-list';

export interface MyUploadsState extends ExploreListState {
  /** The editor's own pubkey if fetched, else null (e.g., fresh install without identity). */
  identityPubkey: string | null;
}

export function useMyUploads(): MyUploadsState {
  const { send } = useShareWs();
  const [identityPubkey, setIdentityPubkey] = useState<string | null>(null);
  const [identityLoading, setIdentityLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const resp = await send('identity.show', {});
        if (!alive) return;
        setIdentityPubkey(resp.params.pubkey_hex);
      } catch {
        if (!alive) return;
        setIdentityPubkey(null);
      } finally {
        if (alive) setIdentityLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [send]);

  const list = useExploreList({
    tab: 'my-uploads',
    kind: 'all',
    sort: 'new',
    tags: [],
    q: '',
    authorPubkey: identityPubkey ?? undefined,
  });

  return {
    ...list,
    loading: identityLoading || list.loading,
    identityPubkey,
  };
}
