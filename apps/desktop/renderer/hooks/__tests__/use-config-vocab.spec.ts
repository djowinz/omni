/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';

describe('useConfigVocab', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('fetches on first mount and returns tags', async () => {
    const send = vi.fn(async () => ({
      id: 'r1',
      type: 'config.vocabResult',
      params: { tags: ['dark', 'minimal', 'gaming'], version: 3 },
    }));
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useConfigVocab } = await import('../use-config-vocab');

    const { result } = renderHook(() => useConfigVocab());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.tags).toEqual(['dark', 'minimal', 'gaming']);
    expect(result.current.version).toBe(3);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it('reuses cache across multiple hook consumers in the same session', async () => {
    const send = vi.fn(async () => ({
      id: 'r1',
      type: 'config.vocabResult',
      params: { tags: ['a'], version: 1 },
    }));
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useConfigVocab } = await import('../use-config-vocab');

    const first = renderHook(() => useConfigVocab());
    await waitFor(() => expect(first.result.current.loading).toBe(false));
    const second = renderHook(() => useConfigVocab());
    await waitFor(() => expect(second.result.current.tags).toEqual(['a']));

    expect(send).toHaveBeenCalledTimes(1);
  });

  it('retry() re-fires the fetch after an error and recovers on success', async () => {
    let calls = 0;
    const send = vi.fn(async () => {
      calls += 1;
      if (calls === 1) {
        throw { code: 'NETWORK', kind: 'Io', message: 'offline' };
      }
      return {
        id: 'r2',
        type: 'config.vocabResult',
        params: { tags: ['recovered'], version: 7 },
      };
    });
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useConfigVocab } = await import('../use-config-vocab');

    const { result } = renderHook(() => useConfigVocab());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toMatchObject({ code: 'NETWORK' });
    expect(result.current.tags).toEqual([]);

    act(() => {
      result.current.retry();
    });

    await waitFor(() => expect(result.current.tags).toEqual(['recovered']));
    expect(result.current.error).toBeNull();
    expect(result.current.version).toBe(7);
    expect(send).toHaveBeenCalledTimes(2);
  });
});
