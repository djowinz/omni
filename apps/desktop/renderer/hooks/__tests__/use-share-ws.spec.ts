/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';

// We re-import the hook fresh per describe block via dynamic import to avoid
// module-level singleton state leaking across test groups. Each `describe` block
// calls `vi.resetModules()` in `beforeEach` and uses a local `importHook` helper.

describe('useShareWs — send() happy path', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('resolves with the Zod-validated response when sendMessage returns a valid listResult frame', async () => {
    const validFrame = {
      id: 'req-1',
      type: 'explorer.listResult',
      items: [
        {
          artifact_id: 'art-abc',
          content_hash: 'sha256-abc',
          author_pubkey: 'pk-abc',
          name: 'Neon Theme',
          kind: 'theme',
          tags: [],
          author_fingerprint_hex: '',
          created_at: 0,
          installs: 0,
          r2_url: 'https://r2.example.com/art-abc',
          thumbnail_url: 'https://r2.example.com/art-abc/thumb.png',
          updated_at: 1700000000,
        },
      ],
      next_cursor: null,
    };

    vi.stubGlobal('omni', {
      sendShareMessage: vi.fn().mockResolvedValue(validFrame),
      onShareEvent: vi.fn().mockReturnValue(() => {}),
    });

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    const response = await result.current.send('explorer.list', { kind: 'theme' });
    expect(response).toEqual(validFrame);
    expect(response.items[0].name).toBe('Neon Theme');
  });
});

describe('useShareWs — send() injects request id', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('generates a fresh id on every call and forwards it to sendShareMessage', async () => {
    const validFrame = {
      id: 'ignored-host-echoes-back',
      type: 'explorer.cancelPreviewResult',
      restored: true,
    };

    const sendShareMessage = vi.fn().mockResolvedValue(validFrame);
    vi.stubGlobal('omni', {
      sendShareMessage,
      onShareEvent: vi.fn().mockReturnValue(() => {}),
    });

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    await result.current.send('explorer.cancelPreview', {
      preview_token: '11111111-1111-1111-1111-111111111111',
    });
    await result.current.send('explorer.cancelPreview', {
      preview_token: '22222222-2222-2222-2222-222222222222',
    });

    expect(sendShareMessage).toHaveBeenCalledTimes(2);
    const firstCall = sendShareMessage.mock.calls[0][0];
    const secondCall = sendShareMessage.mock.calls[1][0];
    expect(typeof firstCall.id).toBe('string');
    expect(firstCall.id.length).toBeGreaterThan(0);
    expect(firstCall.id).not.toBe(secondCall.id);
    expect(firstCall.type).toBe('explorer.cancelPreview');
  });
});

describe('useShareWs — send() parse failure', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('rejects with PARSE_FAILED when sendMessage returns a frame with an unexpected shape', async () => {
    const badFrame = { id: 'req-2', type: 'explorer.listResult', items: 'not-an-array' };

    vi.stubGlobal('omni', {
      sendShareMessage: vi.fn().mockResolvedValue(badFrame),
      onShareEvent: vi.fn().mockReturnValue(() => {}),
    });

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    await expect(result.current.send('explorer.list', { kind: 'theme' })).rejects.toMatchObject({
      code: 'PARSE_FAILED',
      kind: 'Malformed',
    });
  });
});

describe('useShareWs — send() error envelope', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('rejects with the host error when sendMessage returns a D-004-J error envelope', async () => {
    const errorFrame = {
      id: 'req-3',
      type: 'error',
      error: {
        code: 'E_AUTH_EXPIRED',
        kind: 'Auth',
        detail: 'token expired at 2026-01-01',
        message: 'Your session has expired. Please reconnect.',
      },
    };

    vi.stubGlobal('omni', {
      sendShareMessage: vi.fn().mockResolvedValue(errorFrame),
      onShareEvent: vi.fn().mockReturnValue(() => {}),
    });

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    await expect(result.current.send('explorer.list', {})).rejects.toMatchObject({
      code: 'E_AUTH_EXPIRED',
      kind: 'Auth',
      message: 'Your session has expired. Please reconnect.',
    });
  });
});

describe('useShareWs — subscribe() receives frame', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('calls handler with the parsed installProgress frame when a valid frame is fired', async () => {
    let capturedCallback: ((frame: unknown) => void) | null = null;
    const unsubSpy = vi.fn();

    vi.stubGlobal('omni', {
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn().mockImplementation((cb: (frame: unknown) => void) => {
        capturedCallback = cb;
        return unsubSpy;
      }),
    });

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    const handler = vi.fn();
    result.current.subscribe('explorer.installProgress', handler);

    expect(capturedCallback).not.toBeNull();

    const validProgressFrame = {
      id: 'op-1',
      type: 'explorer.installProgress',
      phase: 'download',
      done: 512,
      total: 1024,
    };

    capturedCallback!(validProgressFrame);

    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith(validProgressFrame);
  });
});

describe('useShareWs — subscribe() unsubscribe stops delivery', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('does not call handler after the unsubscribe callback is invoked', async () => {
    let capturedCallback: ((frame: unknown) => void) | null = null;

    vi.stubGlobal('omni', {
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn().mockImplementation((cb: (frame: unknown) => void) => {
        capturedCallback = cb;
        return vi.fn();
      }),
    });

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    const handler = vi.fn();
    const unsub = result.current.subscribe('explorer.installProgress', handler);

    const frame = {
      id: 'op-2',
      type: 'explorer.installProgress',
      phase: 'verify',
      done: 100,
      total: 200,
    };

    // Fire once before unsubscribing — should arrive.
    capturedCallback!(frame);
    expect(handler).toHaveBeenCalledTimes(1);

    // Unsubscribe, then fire again — should NOT arrive.
    unsub();
    capturedCallback!(frame);
    expect(handler).toHaveBeenCalledTimes(1);
  });
});

describe('useShareWs — subscribe() invalid frame', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('does not call handler and emits console.warn for a frame failing schema validation', async () => {
    let capturedCallback: ((frame: unknown) => void) | null = null;

    vi.stubGlobal('omni', {
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn().mockImplementation((cb: (frame: unknown) => void) => {
        capturedCallback = cb;
        return vi.fn();
      }),
    });

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const { useShareWs } = await import('../use-share-ws');
    const { result } = renderHook(() => useShareWs());

    const handler = vi.fn();
    result.current.subscribe('explorer.installProgress', handler);

    // Frame has correct type but wrong shape (phase is not a valid enum value).
    const badFrame = {
      id: 'op-3',
      type: 'explorer.installProgress',
      phase: 'bogus-phase',
      done: 'not-a-number',
      total: 'also-not-a-number',
    };

    capturedCallback!(badFrame);

    expect(handler).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('[useShareWs]'),
      expect.anything(),
      expect.anything(),
    );

    warnSpy.mockRestore();
  });
});
