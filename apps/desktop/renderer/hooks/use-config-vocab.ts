/**
 * useConfigVocab — fetch config.vocab once per session; cache at module scope.
 *
 * Vocab rarely changes (worker-side tag list). Refetching on every sidebar
 * render would be wasteful. Module-level cache is a deliberate single-flight
 * pattern: the first caller triggers the fetch, all concurrent callers
 * subscribe to the same Promise, and subsequent calls read the cached value.
 *
 * The cache is session-lifetime only — a full app reload (F5 during dev, or
 * Electron restart) re-fetches. That matches the real use case: tag lists
 * update on the order of weeks, and any change ships via a worker deploy
 * that also forces a fresh editor session.
 */

import { useEffect, useState } from 'react';
import { useShareWs } from './use-share-ws';
import type { ConfigVocabResult, ShareWsError } from '../lib/share-types';

interface Cached {
  tags: string[];
  version: number;
}

let cached: Cached | null = null;
let inFlight: Promise<Cached> | null = null;

export interface ConfigVocabState {
  tags: string[];
  version: number | null;
  loading: boolean;
  error: ShareWsError | null;
  /** Re-kick the fetch after an error. No-op while a request is already in flight or cache is warm. */
  retry: () => void;
}

export function useConfigVocab(): ConfigVocabState {
  const { send } = useShareWs();
  const [state, setState] = useState<Omit<ConfigVocabState, 'retry'>>(() =>
    cached
      ? { tags: cached.tags, version: cached.version, loading: false, error: null }
      : { tags: [], version: null, loading: true, error: null },
  );
  // Tick bumps on retry() to force the fetch effect to re-run.
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (cached) return;
    if (!inFlight) {
      inFlight = (async () => {
        const resp: ConfigVocabResult = await send('config.vocab', {});
        const next: Cached = { tags: resp.params.tags, version: resp.params.version };
        cached = next;
        return next;
      })();
    }
    let alive = true;
    setState((prev) => ({ ...prev, loading: true, error: null }));
    inFlight
      .then((next) => {
        if (!alive) return;
        setState({ tags: next.tags, version: next.version, loading: false, error: null });
      })
      .catch((err) => {
        if (!alive) return;
        inFlight = null;
        setState({ tags: [], version: null, loading: false, error: err as ShareWsError });
      });
    return () => {
      alive = false;
    };
  }, [send, tick]);

  const retry = () => {
    if (cached || inFlight) return;
    setTick((n) => n + 1);
  };

  return { ...state, retry };
}
