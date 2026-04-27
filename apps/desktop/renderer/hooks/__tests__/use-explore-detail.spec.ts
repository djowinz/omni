/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import type { ArtifactDetail } from '../../lib/share-types';

const FIXTURE: ArtifactDetail = {
  artifact_id: 'art-1',
  kind: 'theme',
  manifest: { name: 'Demo Theme' },
  content_hash: 'h',
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  author_pubkey: 'pk',
  author_fingerprint_hex: 'aabbcc',
  installs: 5,
  reports: 0,
  created_at: 0,
  updated_at: 0,
  status: 'published',
  author_display_name: null,
};

describe('useExploreDetail', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('returns artifact:null when artifactId is null, without calling send', async () => {
    const send = vi.fn();
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreDetail } = await import('../use-explore-detail');

    const { result } = renderHook(() => useExploreDetail(null));

    expect(result.current.artifact).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(send).not.toHaveBeenCalled();
  });

  it('fetches on mount when artifactId is provided', async () => {
    const send = vi.fn(async () => ({
      id: 'r1',
      type: 'explorer.getResult',
      artifact: FIXTURE,
    }));
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreDetail } = await import('../use-explore-detail');

    const { result } = renderHook(() => useExploreDetail('art-1'));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.artifact).toEqual(FIXTURE);
    expect(send).toHaveBeenCalledWith('explorer.get', { artifact_id: 'art-1' });
  });

  it('captures error from send', async () => {
    const send = vi.fn(async () => {
      throw { code: 'NOT_FOUND', kind: 'Io', message: 'gone' };
    });
    vi.doMock('../use-share-ws', () => ({ useShareWs: () => ({ send, subscribe: vi.fn() }) }));
    const { useExploreDetail } = await import('../use-explore-detail');

    const { result } = renderHook(() => useExploreDetail('art-1'));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toMatchObject({ code: 'NOT_FOUND' });
    expect(result.current.artifact).toBeNull();
  });
});
