/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import type { CachedArtifactDetail } from '../../lib/share-types';

const FIXTURE_ITEM: CachedArtifactDetail = {
  artifact_id: 'art-1',
  content_hash: 'h',
  author_pubkey: 'pk',
  name: 'Demo',
  kind: 'theme',
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  updated_at: 1700000000,
};

const SECOND_PAGE: CachedArtifactDetail = { ...FIXTURE_ITEM, artifact_id: 'art-2', name: 'Demo 2' };

function stubSend(
  mockImpl: (type: string, params: unknown) => unknown,
): ReturnType<typeof vi.fn> {
  const fn = vi.fn(async (type: string, params: unknown) => mockImpl(type, params));
  return fn;
}

describe('useExploreList', () => {
  beforeEach(() => {
    vi.resetModules();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('fetches explorer.list on mount with the provided filters', async () => {
    const send = stubSend(() => ({
      id: 'r1',
      type: 'explorer.listResult',
      items: [FIXTURE_ITEM],
      next_cursor: null,
    }));
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreList } = await import('../use-explore-list');

    const { result } = renderHook(() =>
      useExploreList({ tab: 'discover', kind: 'theme', sort: 'new', tags: [], q: '' }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([FIXTURE_ITEM]);
    expect(result.current.nextCursor).toBeNull();
    expect(send).toHaveBeenCalledWith('explorer.list', {
      kind: 'theme',
      sort: 'new',
      tags: [],
      cursor: null,
      limit: 48,
    });
  });

  it('appends items on loadMore using the latest cursor', async () => {
    let callCount = 0;
    const send = stubSend(() => {
      callCount += 1;
      if (callCount === 1) {
        return {
          id: 'r1',
          type: 'explorer.listResult',
          items: [FIXTURE_ITEM],
          next_cursor: 'CURSOR-2',
        };
      }
      return {
        id: 'r2',
        type: 'explorer.listResult',
        items: [SECOND_PAGE],
        next_cursor: null,
      };
    });
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreList } = await import('../use-explore-list');

    const { result } = renderHook(() =>
      useExploreList({ tab: 'discover', kind: 'all', sort: 'new', tags: [], q: '' }),
    );

    await waitFor(() => expect(result.current.items).toHaveLength(1));
    expect(result.current.nextCursor).toBe('CURSOR-2');

    await act(async () => {
      await result.current.loadMore();
    });

    expect(result.current.items).toHaveLength(2);
    expect(result.current.items[1]!.artifact_id).toBe('art-2');
    expect(result.current.nextCursor).toBeNull();
  });

  it('captures errors from the send call', async () => {
    const send = stubSend(() => {
      throw { code: 'NETWORK', kind: 'Io', message: 'offline' };
    });
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreList } = await import('../use-explore-list');

    const { result } = renderHook(() =>
      useExploreList({ tab: 'discover', kind: 'all', sort: 'new', tags: [], q: '' }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toMatchObject({ code: 'NETWORK' });
    expect(result.current.items).toEqual([]);
  });

  it('installed tab returns empty without calling send (Wave 3b placeholder)', async () => {
    const send = stubSend(() => ({ items: [], next_cursor: null }));
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreList } = await import('../use-explore-list');

    const { result } = renderHook(() =>
      useExploreList({ tab: 'installed', kind: 'all', sort: 'new', tags: [], q: '' }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([]);
    expect(send).not.toHaveBeenCalled();
  });

  it('my-uploads tab returns empty without calling send (Wave 3b placeholder)', async () => {
    const send = stubSend(() => ({ items: [], next_cursor: null }));
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreList } = await import('../use-explore-list');

    const { result } = renderHook(() =>
      useExploreList({ tab: 'my-uploads', kind: 'all', sort: 'new', tags: [], q: '' }),
    );

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([]);
    expect(send).not.toHaveBeenCalled();
  });
});
