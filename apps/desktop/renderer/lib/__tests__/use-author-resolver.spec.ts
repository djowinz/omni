import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import {
  useAuthorResolver,
  __resetAuthorCache,
  invalidateAuthor,
  TTL_MS,
} from '../use-author-resolver';

const PK = 'a'.repeat(64);

describe('useAuthorResolver', () => {
  beforeEach(() => {
    __resetAuthorCache();
    vi.restoreAllMocks();
  });

  it('fetches and caches', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(
        JSON.stringify({
          pubkey_hex: PK,
          fingerprint_hex: 'abcdef012345',
          display_name: 'starfire',
          joined_at: 0,
          total_uploads: 0,
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const { result } = renderHook(() => useAuthorResolver(PK));
    await waitFor(() => expect(result.current.data?.display_name).toBe('starfire'));
    expect(fetchSpy).toHaveBeenCalledTimes(1);

    // Re-render with same pubkey — no second fetch.
    renderHook(() => useAuthorResolver(PK));
    await new Promise((r) => setTimeout(r, 10));
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it('caches null on 404', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(null, { status: 404 }),
    );
    const { result } = renderHook(() => useAuthorResolver(PK));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.data).toBeNull();
    renderHook(() => useAuthorResolver(PK));
    await new Promise((r) => setTimeout(r, 10));
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it('single-flights concurrent requests for same pubkey', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockImplementation(
      () =>
        new Promise((resolve) =>
          setTimeout(
            () =>
              resolve(
                new Response(
                  JSON.stringify({
                    pubkey_hex: PK,
                    fingerprint_hex: 'abcdef012345',
                    display_name: 'a',
                    joined_at: 0,
                    total_uploads: 0,
                  }),
                  { status: 200 },
                ),
              ),
            30,
          ),
        ),
    );
    const r1 = renderHook(() => useAuthorResolver(PK));
    const r2 = renderHook(() => useAuthorResolver(PK));
    const r3 = renderHook(() => useAuthorResolver(PK));
    await waitFor(() => {
      expect(r1.result.current.data?.display_name).toBe('a');
      expect(r2.result.current.data?.display_name).toBe('a');
      expect(r3.result.current.data?.display_name).toBe('a');
    });
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it('invalidateAuthor evicts and forces refetch', async () => {
    // mockImplementation (not mockResolvedValue) so each call returns a
    // fresh Response — Response.json() consumes the body, so reusing one
    // Response across multiple fetch calls throws "Body is unusable".
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockImplementation(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            pubkey_hex: PK,
            fingerprint_hex: 'abcdef012345',
            display_name: 'a',
            joined_at: 0,
            total_uploads: 0,
          }),
          { status: 200 },
        ),
      ),
    );
    const { result, rerender } = renderHook(() => useAuthorResolver(PK));
    await waitFor(() => expect(result.current.data?.display_name).toBe('a'));
    act(() => invalidateAuthor(PK));
    rerender();
    await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(2));
  });

  it('expires after TTL', async () => {
    vi.useFakeTimers();
    // mockImplementation (not mockResolvedValue) so each call returns a
    // fresh Response — see invalidateAuthor test above.
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockImplementation(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            pubkey_hex: PK,
            fingerprint_hex: 'abcdef012345',
            display_name: 'a',
            joined_at: 0,
            total_uploads: 0,
          }),
          { status: 200 },
        ),
      ),
    );
    const { rerender } = renderHook(() => useAuthorResolver(PK));
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(TTL_MS + 100);
    rerender();
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchSpy).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });
});
