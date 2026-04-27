/**
 * identity-ws-shape.spec.ts — wire-shape regression for the four identity.*
 * messages added by `2026-04-26-identity-completion-and-display-name`:
 *
 *   - identity.import         (T10 ws_messages.rs handle_identity_import)
 *   - identity.rotate         (T10 ws_messages.rs handle_identity_rotate)
 *   - identity.markBackedUp   (T10 ws_messages.rs handle_identity_mark_backed_up)
 *   - identity.setDisplayName (T10 ws_messages.rs handle_identity_set_display_name)
 *
 * Per `feedback_wire_shape_tests.md`: assert the OUTGOING envelope shape, not
 * the incoming response. Mocking `sendMessage`'s return value alone hides
 * request-side wire bugs; this file exercises the real `useShareWs.send()`
 * path through to the IPC entry point (`window.omni.sendShareMessage`) and
 * inspects the captured envelope.
 *
 * Oracle: `crates/host/src/share/ws_messages.rs` dispatcher arms — the host
 * reads `msg.get("params")` and deserialises into the per-handler param
 * struct. Top-level fields outside `params` are ignored. The hook MUST
 * therefore route every field through `params: { ... }`; spreading at the
 * top level produces "missing field" errors on every required field at the
 * Rust boundary.
 *
 * Per the project's `vitest.config.ts` `include` glob (`**‍/*.spec.ts`),
 * this file is named `.spec.ts` rather than `.test.ts` (renderer convention
 * pinned by T8 of this same plan).
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook } from '@testing-library/react';

import { installShareIpcSpy } from '../../test-utils/mock-share-ws';

// `useShareWs.send()`'s public type signature constrains `type` to
// `keyof ShareRequestMap`. `identity.markBackedUp` and `identity.setDisplayName`
// are NOT yet in that map (T10 added the host handlers but did not extend
// share-types.ts; that wiring lands in #016's renderer work). Tests need to
// invoke `send` for those types to lock the OUTGOING wire shape regardless,
// so we widen the `send` signature locally via `unknown`-cast. This is
// scoped to wire-shape tests; production renderer code paths remain typed.
type LooseSend = (type: string, params: Record<string, unknown>) => Promise<unknown>;

describe('identity WS outgoing wire shapes (per feedback_wire_shape_tests.md)', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
  });

  it('identity.import sends { id, type, params: { encrypted_bytes_b64, passphrase, overwrite_existing } }', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    // The hook rejects (default response is an error envelope); we only care
    // about the captured outgoing call, so swallow the rejection.
    await expect(
      result.current.send('identity.import', {
        encrypted_bytes_b64: 'AAAAAAAA',
        passphrase: 'very-long-passphrase',
        overwrite_existing: false,
      }),
    ).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(1);
    const envelope = sendSpy.mock.calls[0][0] as {
      id: unknown;
      type: unknown;
      params: { encrypted_bytes_b64: unknown; passphrase: unknown; overwrite_existing: unknown };
    };

    // id is hook-generated (uuid). Assert it's present and a non-empty string,
    // not a hard-coded value — the real wire test is type + params shape.
    expect(typeof envelope.id).toBe('string');
    expect((envelope.id as string).length).toBeGreaterThan(0);
    expect(envelope.type).toBe('identity.import');
    expect(envelope.params.encrypted_bytes_b64).toBe('AAAAAAAA');
    expect(envelope.params.passphrase).toBe('very-long-passphrase');
    expect(envelope.params.overwrite_existing).toBe(false);
    // Negative assertion: params field types match the host's struct P
    // (encrypted_bytes_b64: String, passphrase: String, overwrite_existing: bool).
    expect(typeof envelope.params.encrypted_bytes_b64).toBe('string');
    expect(typeof envelope.params.passphrase).toBe('string');
    expect(typeof envelope.params.overwrite_existing).toBe('boolean');
  });

  it('identity.rotate sends { id, type, params: {} }', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    await expect(result.current.send('identity.rotate', {})).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(1);
    const envelope = sendSpy.mock.calls[0][0] as {
      id: unknown;
      type: unknown;
      params: unknown;
    };

    expect(typeof envelope.id).toBe('string');
    expect(envelope.type).toBe('identity.rotate');
    // Host's handle_identity_rotate parses Value but ignores params; the
    // wire contract is "send an object" — empty {} satisfies. Assert the
    // params slot is an object (not undefined / null / array).
    expect(typeof envelope.params).toBe('object');
    expect(envelope.params).not.toBeNull();
    expect(Array.isArray(envelope.params)).toBe(false);
    expect(Object.keys(envelope.params as object)).toHaveLength(0);
  });

  it('identity.markBackedUp sends { id, type, params: { path, timestamp } }', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    // Cast through `unknown` — `identity.markBackedUp` is not yet in
    // ShareRequestMap (see top-of-file note). The wire-shape test still
    // needs to lock the OUTGOING envelope so #016's renderer tasks can
    // reuse the hook with confidence.
    const sendLoose = result.current.send as unknown as LooseSend;

    await expect(
      sendLoose('identity.markBackedUp', {
        path: 'C:\\Users\\foo\\identity.omniid',
        timestamp: 1714000000,
      }),
    ).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(1);
    const envelope = sendSpy.mock.calls[0][0] as {
      id: unknown;
      type: unknown;
      params: { path: unknown; timestamp: unknown };
    };

    expect(typeof envelope.id).toBe('string');
    expect(envelope.type).toBe('identity.markBackedUp');
    // Host's handle_identity_mark_backed_up struct P { path: String, timestamp: u64 }.
    expect(envelope.params.path).toBe('C:\\Users\\foo\\identity.omniid');
    expect(typeof envelope.params.path).toBe('string');
    expect(envelope.params.timestamp).toBe(1714000000);
    expect(typeof envelope.params.timestamp).toBe('number');
  });

  it('identity.setDisplayName sends { id, type, params: { display_name } }', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    const sendLoose = result.current.send as unknown as LooseSend;

    await expect(
      sendLoose('identity.setDisplayName', { display_name: 'starfire' }),
    ).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(1);
    const envelope = sendSpy.mock.calls[0][0] as {
      id: unknown;
      type: unknown;
      params: { display_name: unknown };
    };

    expect(typeof envelope.id).toBe('string');
    expect(envelope.type).toBe('identity.setDisplayName');
    // Host's handle_identity_set_display_name struct P { display_name: String }.
    expect(envelope.params.display_name).toBe('starfire');
    expect(typeof envelope.params.display_name).toBe('string');
  });

  it('every identity.* call carries a fresh request id (no reuse across calls)', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    const sendLoose = result.current.send as unknown as LooseSend;

    await expect(sendLoose('identity.rotate', {})).rejects.toBeDefined();
    await expect(sendLoose('identity.setDisplayName', { display_name: 'a' })).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(2);
    const id1 = (sendSpy.mock.calls[0][0] as { id: string }).id;
    const id2 = (sendSpy.mock.calls[1][0] as { id: string }).id;
    expect(id1).not.toBe(id2);
    expect(id1.length).toBeGreaterThan(0);
    expect(id2.length).toBeGreaterThan(0);
  });
});
