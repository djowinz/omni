/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import type { CachedArtifactDetail } from '../../lib/share-types';

const FIXTURE_ITEM: CachedArtifactDetail = {
  artifact_id: 'mine-1',
  content_hash: 'h',
  author_pubkey: 'aa'.repeat(32),
  name: 'My Theme',
  kind: 'theme',
  tags: [],
  installs: 0,
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  updated_at: 1700000000,
};

describe('useMyUploads', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('fetches own pubkey via identity.show then calls explorer.list with author_pubkey', async () => {
    const send = vi.fn(async (type: string, _params: unknown) => {
      if (type === 'identity.show') {
        return {
          id: 'r1',
          type: 'identity.showResult',
          params: {
            pubkey_hex: 'aa'.repeat(32),
            fingerprint_hex: '',
            fingerprint_emoji: [],
            fingerprint_words: [],
            created_at: 0,
            backed_up: false,
          },
        };
      }
      // explorer.list
      return {
        id: 'r2',
        type: 'explorer.listResult',
        items: [FIXTURE_ITEM],
        next_cursor: null,
      };
    });
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useMyUploads } = await import('../use-my-uploads');

    const { result } = renderHook(() => useMyUploads());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([FIXTURE_ITEM]);

    const listCall = send.mock.calls.find(([t]) => t === 'explorer.list');
    expect(listCall).toBeDefined();
    expect((listCall![1] as { author_pubkey?: string }).author_pubkey).toBe('aa'.repeat(32));
  });

  it('returns empty with no error when identity.show fails (fresh install)', async () => {
    const send = vi.fn(async (type: string, _params: unknown) => {
      if (type === 'identity.show') {
        throw { code: 'IDENTITY_MISSING', kind: 'Io', message: 'no identity yet' };
      }
      throw new Error('explorer.list should not be called');
    });
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useMyUploads } = await import('../use-my-uploads');

    const { result } = renderHook(() => useMyUploads());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([]);
    expect(result.current.identityPubkey).toBeNull();
  });
});
