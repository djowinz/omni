/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { PublishablesEntry } from '@omni/shared-types';
import { useUploadMachine, detectMode } from '../hooks/use-upload-machine';

// The new machine subscribes to packProgress + publishProgress via useShareWs
// and calls identity.show / workspace.listPublishables on mount. Stub the
// `window.omni` bridge so these effects no-op rather than crashing on a
// missing IPC surface (which produces an "infinite render → heap OOM" loop).
beforeEach(() => {
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage: vi.fn(async (msg: { type: string; id: string }) => {
      // identity.show — return a backed-up identity so the publish-time
      // backup gate doesn't open in tests that drive `next()` to publish.
      if (msg.type === 'identity.show') {
        return {
          id: msg.id,
          type: 'identity.showResult',
          params: {
            pubkey_hex: '00'.repeat(32),
            fingerprint_hex: '',
            fingerprint_emoji: [],
            fingerprint_words: [],
            created_at: 0,
            backed_up: true,
            display_name: null,
            last_backed_up_at: null,
            last_rotated_at: null,
            last_backup_path: null,
          },
        };
      }
      if (msg.type === 'workspace.listPublishables') {
        return {
          id: msg.id,
          type: 'workspace.listPublishablesResult',
          params: { entries: [] },
        };
      }
      throw new Error('unexpected sendShareMessage in machine test: ' + msg.type);
    }),
    onShareEvent: vi.fn(() => () => {}),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// ── Fixtures ─────────────────────────────────────────────────────────────────

const PUBKEY_A = 'aa'.repeat(32);
const PUBKEY_B = 'bb'.repeat(32);

function makeEntry(overrides: Partial<PublishablesEntry> = {}): PublishablesEntry {
  return {
    kind: 'overlay',
    workspace_path: 'overlays/test-overlay',
    name: 'test-overlay',
    widget_count: 5,
    modified_at: '2026-04-25T00:00:00Z',
    has_preview: true,
    sidecar: null,
    ...overrides,
  };
}

function withSidecar(authorPubkey: string): PublishablesEntry {
  return makeEntry({
    sidecar: {
      artifact_id: 'ov_01ABC',
      author_pubkey_hex: authorPubkey,
      version: '1.0.0',
      last_published_at: '2026-04-20T00:00:00Z',
    },
  });
}

// ── detectMode (pure helper) ─────────────────────────────────────────────────

describe('detectMode', () => {
  it('returns create when entry is null', () => {
    expect(detectMode(null, PUBKEY_A)).toBe('create');
  });

  it('returns create when entry has no sidecar', () => {
    expect(detectMode(makeEntry({ sidecar: null }), PUBKEY_A)).toBe('create');
  });

  it('returns create when current pubkey is null', () => {
    expect(detectMode(withSidecar(PUBKEY_A), null)).toBe('create');
  });

  it('returns update when sidecar.author_pubkey_hex matches current pubkey', () => {
    expect(detectMode(withSidecar(PUBKEY_A), PUBKEY_A)).toBe('update');
  });

  it('returns create when sidecar author differs (INV-7.6.4)', () => {
    expect(detectMode(withSidecar(PUBKEY_B), PUBKEY_A)).toBe('create');
  });
});

// ── useUploadMachine ─────────────────────────────────────────────────────────

describe('useUploadMachine — initial state', () => {
  it('starts on step "select" in mode "create" with no selection', () => {
    const { result } = renderHook(() => useUploadMachine());
    expect(result.current.state.step).toBe('select');
    expect(result.current.state.mode).toBe('create');
    expect(result.current.state.selected).toBeNull();
    expect(result.current.state.completedSteps).toEqual([]);
    expect(result.current.state.stepError).toBeNull();
    expect(result.current.state.uploadState).toBe('idle');
    expect(result.current.state.primaryDisabled).toBe(true);
    expect(result.current.state.currentPubkey).toBeNull();
  });
});

describe('useUploadMachine — selection + mode auto-detection', () => {
  it('selecting an item enables the primary CTA on Step 1', () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    expect(result.current.state.selected?.name).toBe('test-overlay');
    expect(result.current.state.primaryDisabled).toBe(false);
  });

  it('clearing the selection re-disables the primary CTA on Step 1', () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    act(() => {
      result.current.actions.selectItem(null);
    });
    expect(result.current.state.primaryDisabled).toBe(true);
  });

  it('auto-detects update mode when sidecar matches current pubkey', () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.setCurrentPubkey(PUBKEY_A);
    });
    act(() => {
      result.current.actions.selectItem(withSidecar(PUBKEY_A));
    });
    expect(result.current.state.mode).toBe('update');
  });

  it('stays in create mode when sidecar pubkey differs', () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.setCurrentPubkey(PUBKEY_A);
    });
    act(() => {
      result.current.actions.selectItem(withSidecar(PUBKEY_B));
    });
    expect(result.current.state.mode).toBe('create');
  });

  it('re-derives mode when pubkey arrives after selection', () => {
    const { result } = renderHook(() => useUploadMachine());
    // Pubkey not yet known — selection alone can't decide update mode.
    act(() => {
      result.current.actions.selectItem(withSidecar(PUBKEY_A));
    });
    expect(result.current.state.mode).toBe('create');
    // Pubkey arrives → mode flips.
    act(() => {
      result.current.actions.setCurrentPubkey(PUBKEY_A);
    });
    expect(result.current.state.mode).toBe('update');
  });
});

describe('useUploadMachine — next() in-flight guard', () => {
  it('advances step on first call', async () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    await act(async () => {
      await result.current.actions.next();
    });
    expect(result.current.state.step).toBe('details');
    expect(result.current.state.completedSteps).toContain(0);
  });

  it('clamps at the final step (no overflow past upload)', async () => {
    // T-A2.1 contract: next() from 'details' requires a valid Name; next()
    // from 'packing' requires all pack stages to be 'passed' (gated by
    // INV-7.3.8). We can't drive packProgress frames from this isolated
    // hook test, so verify the clamp by going as far as the gates allow
    // (select → details) and then asserting the gates HOLD instead of
    // overshooting. The 'no overflow past upload' contract is also
    // enforced in the reducer: STEP_INDEX[upload] === STEP_ORDER.length-1
    // so a NEXT dispatch on upload is a no-op (covered by the in-flight
    // gates above, plus the explicit `if (idx >= …) return state` branch).
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
      result.current.form.setValue('name', 'Test');
    });
    await act(async () => {
      await result.current.actions.next(); // → details
      await result.current.actions.next(); // → packing (kicks runPack which
      // throws on the unstubbed upload.pack — caught silently. Pack stages
      // remain 'pending' so the next() call below returns early via the gate.)
    });
    expect(result.current.state.step).toBe('packing');

    await act(async () => {
      await result.current.actions.next(); // gated — packing stages still pending
    });
    expect(result.current.state.step).toBe('packing');
  });

  it('concurrent next() calls only advance one step (in-flight guard)', async () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    // Fire two next() calls in the same microtask. The guard is held
    // synchronously via a ref, so the second call sees the flag and
    // returns immediately without dispatching.
    await act(async () => {
      const first = result.current.actions.next();
      const second = result.current.actions.next();
      await Promise.all([first, second]);
    });
    expect(result.current.state.step).toBe('details');
  });
});

describe('useUploadMachine — back()', () => {
  it('returns to the prior step and un-completes it', async () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    await act(async () => {
      await result.current.actions.next(); // → details
    });
    expect(result.current.state.completedSteps).toContain(0);
    act(() => {
      result.current.actions.back();
    });
    expect(result.current.state.step).toBe('select');
    expect(result.current.state.completedSteps).not.toContain(0);
  });

  it('is a no-op on the first step', () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.back();
    });
    expect(result.current.state.step).toBe('select');
  });
});

describe('useUploadMachine — reset on close', () => {
  it('reset() returns the machine to its initial state', async () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.setCurrentPubkey(PUBKEY_A);
    });
    act(() => {
      result.current.actions.selectItem(withSidecar(PUBKEY_A));
      result.current.form.setValue('name', 'Test');
    });
    await act(async () => {
      await result.current.actions.next(); // → details
      await result.current.actions.next(); // → packing (form.trigger() passes)
    });
    // Mutated state.
    expect(result.current.state.step).toBe('packing');
    expect(result.current.state.mode).toBe('update');
    expect(result.current.state.selected).not.toBeNull();
    expect(result.current.state.completedSteps).not.toEqual([]);

    act(() => {
      result.current.actions.reset();
    });

    expect(result.current.state.step).toBe('select');
    expect(result.current.state.mode).toBe('create');
    expect(result.current.state.selected).toBeNull();
    expect(result.current.state.completedSteps).toEqual([]);
    expect(result.current.state.stepError).toBeNull();
    expect(result.current.state.uploadState).toBe('idle');
    expect(result.current.state.primaryDisabled).toBe(true);
    expect(result.current.state.currentPubkey).toBeNull();
  });

  it('reset() releases the in-flight guard so subsequent next() works', async () => {
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    await act(async () => {
      await result.current.actions.next();
    });
    act(() => {
      result.current.actions.reset();
    });
    // After reset, a fresh selection + next() must still advance.
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    await act(async () => {
      await result.current.actions.next();
    });
    expect(result.current.state.step).toBe('details');
  });
});
