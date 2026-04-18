/**
 * useExploreDetail — fetch explorer.get for the currently-selected artifact.
 *
 * Re-fetches whenever artifactId changes; ignores stale responses if the
 * user selects a different artifact while the previous fetch is in-flight.
 */

import { useEffect, useRef, useState } from 'react';
import { useShareWs } from './use-share-ws';
import type { ArtifactDetail, ShareWsError } from '../lib/share-types';

export interface ExploreDetailState {
  artifact: ArtifactDetail | null;
  loading: boolean;
  error: ShareWsError | null;
}

export function useExploreDetail(artifactId: string | null): ExploreDetailState {
  const { send } = useShareWs();
  const [artifact, setArtifact] = useState<ArtifactDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<ShareWsError | null>(null);
  const latestId = useRef<string | null>(null);

  useEffect(() => {
    if (artifactId === null) {
      latestId.current = null;
      setArtifact(null);
      setError(null);
      setLoading(false);
      return;
    }
    latestId.current = artifactId;
    setLoading(true);
    setError(null);
    void send('explorer.get', { artifact_id: artifactId })
      .then((resp) => {
        if (latestId.current !== artifactId) return; // stale
        setArtifact(resp.artifact);
      })
      .catch((err) => {
        if (latestId.current !== artifactId) return;
        setError(err as ShareWsError);
        setArtifact(null);
      })
      .finally(() => {
        if (latestId.current === artifactId) {
          setLoading(false);
        }
      });
  }, [artifactId, send]);

  return { artifact, loading, error };
}
