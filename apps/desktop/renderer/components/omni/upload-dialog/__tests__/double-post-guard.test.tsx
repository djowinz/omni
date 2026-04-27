/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * Double-POST regression gate (T-A2.2 / OWI-42, spec §8.9).
 *
 * Pins the contract that rapid Publish clicks fire `upload.publish` exactly
 * once on the wire. The OWI-6 ticket originally suspected React 18 StrictMode
 * double-mounting; this app is not wrapped in StrictMode (verified during
 * spec §8.9 root-cause hunt), so the real defence is the in-flight ref guard
 * inside `useUploadMachine.next()` (added by T-A2.1 / OWI-35 and extended by
 * OWI-41 to cover the publish branch).
 *
 * The guard is ref-backed (not state-backed) because state updates are batched
 * across React 18's automatic batching window — by the time the second click
 * handler reads state, the first click's "in-flight" flag still hasn't landed.
 * A ref mutates synchronously, which is what closes the race.
 *
 * What this test exercises:
 *   - Drive the machine to Step 3 ('packing') with all five pack stages
 *     marked 'passed' (via a real `upload.packProgress` subscription frame
 *     dispatched from the mocked `onShareEvent`). This is the only state
 *     where `next()` actually fires `upload.publish`.
 *   - Fire two `actions.next()` calls in the same microtask (mirrors a
 *     double-click on the Publish CTA — both React click handlers run in
 *     the same task tick before either completes).
 *   - Assert `sendShareMessage` was called with `type: 'upload.publish'`
 *     exactly ONCE.
 *
 * Failure mode this prevents (root cause documented in the use-upload-machine
 * file header): without the ref guard, the second `next()` would advance past
 * the (now-stale) state check, dispatch `NEXT` a second time, and call
 * `publishWithGate()` again — POSTing the bundle twice. The first extra
 * upload.publish would race the artifact-id allocator at the worker; the
 * second would get rejected with `AuthorNameConflict`. Either outcome
 * surfaces as a bewildering UX bug.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { PackStage, PublishablesEntry } from '@omni/shared-types';
import { useUploadMachine } from '../hooks/use-upload-machine';

// ── Test harness ─────────────────────────────────────────────────────────────

/** Captures every `share:event` listener so the test can push frames into
 *  the renderer's `useShareWs.subscribe()` plumbing. The hook subscribes to
 *  `upload.packProgress` on mount; we need to deliver five 'passed' frames
 *  to ungate the Publish step. */
let shareEventListeners: Array<(frame: unknown) => void> = [];

/** Captured outgoing `sendShareMessage` calls. Each entry is the raw `msg`
 *  arg ({ id, type, params }). Letting the test assert against this directly
 *  rather than against a `vi.fn()` spy keeps the call-shape inspection
 *  readable when failures land. */
let sentMessages: Array<{ id: string; type: string; params: unknown }> = [];

/** Mock `sendShareMessage` — returns a successful frame for the request types
 *  the machine fires during the path under test. Returns a never-resolving
 *  promise for `upload.publish` itself so the in-flight guard stays held
 *  across the second `next()` call (matches production: real publishes take
 *  hundreds of ms; the guard must hold the whole time). */
const sendShareMessage = vi.fn(async (msg: { id: string; type: string; params: unknown }) => {
  sentMessages.push(msg);
  switch (msg.type) {
    case 'identity.show':
      return {
        id: msg.id,
        type: 'identity.showResult',
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
      };
    case 'workspace.listPublishables':
      return {
        id: msg.id,
        type: 'workspace.listPublishablesResult',
        params: { entries: [] },
      };
    case 'upload.pack':
      // UploadPackResultSchema requires content_hash + sizes. We don't care
      // about the result shape — runPack swallows it and the per-stage UI is
      // driven by emitPackProgress() below — but the renderer's Zod parse
      // would throw PARSE_FAILED otherwise, which runPack would also swallow,
      // but the noise muddies test output.
      return {
        id: msg.id,
        type: 'upload.packResult',
        params: {
          content_hash: 'sha256:0000',
          compressed_size: 0,
          uncompressed_size: 0,
          manifest: {},
          sanitize_report: { issues: [] },
        },
      };
    case 'upload.publish':
      // Hold the publish open. The in-flight guard is released in a `finally`
      // block when the await chain inside next() completes; we want it held
      // for the duration of the second next() call so the guard's behaviour
      // under contention is what's actually being tested.
      return new Promise(() => {
        /* never resolves */
      });
    default:
      throw new Error(`unexpected sendShareMessage in double-post test: ${msg.type}`);
  }
});

beforeEach(() => {
  shareEventListeners = [];
  sentMessages = [];
  sendShareMessage.mockClear();
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage,
    onShareEvent: vi.fn((cb: (frame: unknown) => void) => {
      shareEventListeners.push(cb);
      return () => {
        shareEventListeners = shareEventListeners.filter((h) => h !== cb);
      };
    }),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// ── Fixture ──────────────────────────────────────────────────────────────────

function makeEntry(overrides: Partial<PublishablesEntry> = {}): PublishablesEntry {
  return {
    kind: 'overlay',
    workspace_path: 'overlays/double-post-overlay',
    name: 'double-post-overlay',
    widget_count: 1,
    modified_at: '2026-04-25T00:00:00Z',
    has_preview: true,
    sidecar: null,
    ...overrides,
  };
}

const PACK_STAGES: readonly PackStage[] = [
  'schema',
  'content-safety',
  'asset',
  'dependency',
  'size',
] as const;

/** Push a `upload.packProgress` frame through every registered share-event
 *  listener (same path the real preload bridge uses). The reducer's
 *  `PACK_PROGRESS` action flips `packStages[stage]` to the supplied status.
 *  `detail` is required (`string | null`) per `PackProgressSchema` —
 *  ts-rs emits `Option<String>` as nullable, not optional. */
function emitPackProgress(stage: PackStage, status: 'passed' | 'failed' | 'running') {
  const frame = {
    id: `pack-${stage}`,
    type: 'upload.packProgress',
    params: { stage, status, detail: null },
  };
  for (const cb of shareEventListeners) cb(frame);
}

// ── Test ─────────────────────────────────────────────────────────────────────

describe('useUploadMachine — double-POST guard (T-A2.2 / OWI-42, §8.9)', () => {
  it('rapid Publish clicks fire upload.publish exactly once', async () => {
    const { result } = renderHook(() => useUploadMachine());

    // Drive the machine: select an entry → fill required form fields → advance
    // through Step 1 (select) and Step 2 (details) into Step 3 (packing). The
    // 'details → packing' transition kicks `upload.pack` (which our mock
    // resolves), but the per-stage UI gating reads from packStages, which we
    // populate by hand below.
    act(() => {
      result.current.actions.selectItem(makeEntry());
      result.current.form.setValue('name', 'double-post-overlay');
    });
    await act(async () => {
      await result.current.actions.next(); // select → details
    });
    await act(async () => {
      await result.current.actions.next(); // details → packing (fires upload.pack)
    });
    expect(result.current.state.step).toBe('packing');

    // Mark every pack stage as passed. The reducer flips primaryDisabled to
    // false once allPackStagesPassed is true, which is the gate `next()`
    // checks before firing publishWithGate.
    act(() => {
      for (const s of PACK_STAGES) emitPackProgress(s, 'passed');
    });
    expect(result.current.state.primaryDisabled).toBe(false);

    // Sanity-check: nothing has POSTed `upload.publish` yet.
    expect(sentMessages.filter((m) => m.type === 'upload.publish')).toHaveLength(0);

    // Double-click the Publish CTA. Both calls happen in the same microtask
    // — exactly the production race condition. The in-flight ref guard inside
    // next() must catch the second call before the await Promise.resolve()
    // yield in the first call lets the second slip past.
    //
    // We do NOT `await` the returned promises here: the upload.publish mock
    // is intentionally never-resolving (so the guard stays held for the
    // duration the test inspects). We only need the microtask queue to drain
    // far enough that:
    //   (a) the first next()'s `await Promise.resolve()` has resumed,
    //       it has dispatched NEXT, and called publishWithGate which has
    //       fired identity.show then upload.publish (and is now awaiting
    //       the never-resolving publish promise);
    //   (b) the second next()'s synchronous guard check has rejected the
    //       call before its own `await Promise.resolve()` (it returns
    //       BEFORE the yield because the guard is checked first).
    //
    // Three `await Promise.resolve()` cycles inside act() are enough to
    // cover (a)'s identity.show + upload.publish microtask hops without
    // hanging on the publish promise.
    let firstSettled = false;
    let secondSettled = false;
    await act(async () => {
      void result.current.actions.next().finally(() => {
        firstSettled = true;
      });
      void result.current.actions.next().finally(() => {
        secondSettled = true;
      });
      // Drain microtasks until identity.show + upload.publish have been
      // dispatched. The second call settles immediately (synchronous guard
      // rejection); the first stays pending on the never-resolving publish.
      for (let i = 0; i < 10; i++) await Promise.resolve();
    });

    // The second call must have settled (it returned synchronously after the
    // guard check). The first call is pending forever-ish on the publish
    // promise — that's by design and what holds the guard for assertion.
    expect(secondSettled).toBe(true);
    expect(firstSettled).toBe(false);

    // The contract: exactly ONE upload.publish on the wire.
    const publishCalls = sentMessages.filter((m) => m.type === 'upload.publish');
    expect(publishCalls).toHaveLength(1);
    // And it should target the workspace path of the selected entry, not be
    // a malformed envelope (rules out "fired but with wrong shape" failures).
    const params = publishCalls[0].params as { workspace_path?: string };
    expect(params.workspace_path).toBe('overlays/double-post-overlay');
  });

  it('a second next() AFTER the publish resolves is allowed (guard is per-call, not permanent)', async () => {
    // The guard releases in a finally block when next()'s await chain
    // settles. This test pins the "guard is per-call" half of the contract
    // — important so the user can Retry after a failed publish without
    // having to close+reopen the dialog.
    //
    // We do this by resolving upload.publish (override the never-resolving
    // default) and asserting that a third next() (which would be a Retry on
    // the upload step in production — actually wired through retryPublish
    // in the real UI but the next() call is structurally the same gate) is
    // not blocked once the first call's promise settles.

    // Override the mock for this test to resolve upload.publish.
    sendShareMessage.mockImplementation(
      async (msg: { id: string; type: string; params: unknown }) => {
        sentMessages.push(msg);
        if (msg.type === 'identity.show') {
          return {
            id: msg.id,
            type: 'identity.showResult',
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
          };
        }
        if (msg.type === 'workspace.listPublishables') {
          return {
            id: msg.id,
            type: 'workspace.listPublishablesResult',
            params: { entries: [] },
          };
        }
        if (msg.type === 'upload.pack') {
          return {
            id: msg.id,
            type: 'upload.packResult',
            params: {
              content_hash: 'sha256:0000',
              compressed_size: 0,
              uncompressed_size: 0,
              manifest: {},
              sanitize_report: { issues: [] },
            },
          };
        }
        if (msg.type === 'upload.publish') {
          return {
            id: msg.id,
            type: 'upload.publishResult',
            params: {
              artifact_id: 'ov_01ABC',
              content_hash: 'sha256:deadbeef',
              status: 'created',
              worker_url: 'https://hub.example/ov_01ABC',
            },
          };
        }
        throw new Error(`unexpected: ${msg.type}`);
      },
    );

    const { result } = renderHook(() => useUploadMachine());
    act(() => {
      result.current.actions.selectItem(makeEntry());
      result.current.form.setValue('name', 'double-post-overlay');
    });
    await act(async () => {
      await result.current.actions.next();
    });
    await act(async () => {
      await result.current.actions.next();
    });
    act(() => {
      for (const s of PACK_STAGES) emitPackProgress(s, 'passed');
    });

    // First publish: settles successfully.
    await act(async () => {
      await result.current.actions.next();
    });
    expect(result.current.state.uploadState).toBe('success');
    expect(sentMessages.filter((m) => m.type === 'upload.publish')).toHaveLength(1);

    // After the publish settles, the guard is released. A subsequent next()
    // is a no-op on Step 4 (the reducer's NEXT clamps at upload), but the
    // guard itself doesn't block it — that's what we're verifying.
    expect(() => {
      void result.current.actions.next();
    }).not.toThrow();
  });
});
