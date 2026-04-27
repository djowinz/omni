// useAuthorResolver — single resolver path for individual-author lookups.
//
// Module-level Map keyed by pubkey_hex with a 5-minute TTL. Cache miss →
// single-flight `fetch('/v1/author/<pubkey_hex>')` → populate. 404 caches
// `null` for the same TTL (avoids hammering on missing authors). Cache
// eviction on local rename/rotate via `invalidateAuthor()`. List/gallery/
// artifact responses can pre-warm the cache via `primeAuthor()` so
// subsequent resolver hits are warm — no N+1 on grids.
//
// Authoritative spec: 2026-04-26-identity-completion-and-display-name §6.

import { useEffect, useState } from 'react';
import type { AuthorDetail } from '@omni/shared-types';

export const TTL_MS = 5 * 60 * 1000;

type Entry = {
  detail: AuthorDetail | null;
  fetched_at: number;
  inflight: Promise<AuthorDetail | null> | null;
  error?: Error; // populated on non-404 fetch failure; cleared on next successful fetch
};

const cache = new Map<string, Entry>();
const subscribers = new Set<() => void>();

function notify(): void {
  for (const s of subscribers) s();
}

/** Test-only: clear the module-level cache between tests. */
export function __resetAuthorCache(): void {
  cache.clear();
}

/** Evict a single author's cache entry (call after local rename/rotate). */
export function invalidateAuthor(pubkey_hex: string): void {
  cache.delete(pubkey_hex);
  notify();
}

/**
 * Pre-warm the cache from list/gallery/artifact responses that already
 * carry an embedded `author_display_name`. Avoids N+1 fetches on grids.
 *
 * The fingerprint slot is filled with a placeholder; consumers who need
 * the real fingerprint will trigger a refetch via `useAuthorResolver` once
 * the TTL elapses, or can call `invalidateAuthor` to force one.
 */
export function primeAuthor(
  pubkey_hex: string,
  display_name: string | null,
): void {
  if (display_name === null) {
    // Don't poison the cache with a "confirmed null" entry — the author MAY
    // exist but just haven't set a name yet. Early-return so the next
    // useAuthorResolver call still triggers a real /v1/author fetch for the
    // full detail (fingerprint_hex, joined_at, total_uploads).
    return;
  }
  cache.set(pubkey_hex, {
    detail: {
      pubkey_hex,
      fingerprint_hex: '',
      display_name,
      joined_at: 0,
      total_uploads: 0,
    },
    fetched_at: Date.now(),
    inflight: null,
  });
  notify();
}

async function fetchAuthor(pubkey_hex: string): Promise<AuthorDetail | null> {
  const res = await fetch(`/v1/author/${pubkey_hex}`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`author fetch failed: ${res.status}`);
  return (await res.json()) as AuthorDetail;
}

export function useAuthorResolver(pubkey_hex: string): {
  data: AuthorDetail | null;
  loading: boolean;
  error: Error | null;
} {
  const [, force] = useState(0);
  useEffect(() => {
    const cb = (): void => force((n) => n + 1);
    subscribers.add(cb);
    return () => {
      subscribers.delete(cb);
    };
  }, []);

  const now = Date.now();
  let entry = cache.get(pubkey_hex);
  const isFresh = !!entry && now - entry.fetched_at < TTL_MS;

  if (!isFresh && (!entry || !entry.inflight)) {
    const promise = fetchAuthor(pubkey_hex)
      .then((detail) => {
        cache.set(pubkey_hex, {
          detail,
          fetched_at: Date.now(),
          inflight: null,
        }); // no error field = cleared on successful refetch
        notify();
        return detail;
      })
      .catch((err): AuthorDetail | null => {
        const errorObj = err instanceof Error ? err : new Error(String(err));
        cache.set(pubkey_hex, {
          detail: null,
          fetched_at: Date.now(),
          inflight: null,
          error: errorObj,
        });
        notify();
        // Do NOT re-throw — no consumer awaits this Promise; re-throwing creates
        // an unhandled rejection. Error is now surfaced via the entry.
        return null;
      });
    entry = {
      detail: entry?.detail ?? null,
      fetched_at: entry?.fetched_at ?? 0,
      inflight: promise,
    };
    cache.set(pubkey_hex, entry);
  }

  return {
    data: isFresh ? (entry?.detail ?? null) : null,
    loading: !!entry?.inflight && !isFresh,
    error: entry?.error ?? null,
  };
}
