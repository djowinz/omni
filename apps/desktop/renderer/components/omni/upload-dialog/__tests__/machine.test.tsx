/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { PublishablesEntry } from '@omni/shared-types';
import { useUploadMachine, detectMode } from '../hooks/use-upload-machine';

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
    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
    });
    await act(async () => {
      await result.current.actions.next(); // → details
      await result.current.actions.next(); // → packing
      await result.current.actions.next(); // → upload
      await result.current.actions.next(); // no-op
    });
    expect(result.current.state.step).toBe('upload');
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
    });
    await act(async () => {
      await result.current.actions.next(); // → details
      await result.current.actions.next(); // → packing
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
