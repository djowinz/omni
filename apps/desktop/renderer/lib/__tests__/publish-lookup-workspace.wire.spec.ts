/**
 * publish-lookup-workspace.wire.spec.ts — wire-shape regression for
 * `publish.lookupWorkspace` (OWI-132 T10).
 *
 * Per `feedback_wire_shape_tests.md`: tests that mock `useShareWs.send()`'s
 * return value alone hide request-side wire bugs (wrong field name, params
 * nesting drift, accidental top-level spread, etc.). The canonical pattern
 * — already used by `identity-ws-shape.spec.ts` in this directory — is:
 *
 *   1. Stub `window.omni.sendShareMessage` with a spy via `installShareIpcSpy()`
 *      (the actual IPC entry point the hook calls into).
 *   2. Invoke the hook's `send('publish.lookupWorkspace', params)`.
 *   3. Inspect `sendSpy.mock.calls[0][0]` to assert the literal outgoing
 *      `{ id, type, params }` envelope handed to the IPC layer.
 *
 * This exercises the REAL outgoing-message boundary; we are NOT mocking
 * `send`'s return value — we capture the call arguments, which is a
 * fundamentally different mechanism from a return-value mock.
 *
 * Oracle for the request shape:
 *   `crates/host/src/share/ws_messages.rs::handle_lookup_workspace` reads
 *   `msg.get("params")` and deserialises into a struct with a single
 *   `artifact_id: String` field. Top-level fields outside `params` are
 *   ignored by the host dispatcher.
 *
 * Oracle for the response shape:
 *   `PublishLookupWorkspaceResultSchema` in `share-types.ts` (Task 6); the
 *   three terminal statuses are `ok`, `missing_index`, `missing_folder`.
 *
 * File is named `*.wire.spec.ts` per the plan; the project's vitest config
 * picks up `**‍/*.spec.ts` (renderer convention pinned by T8).
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook } from '@testing-library/react';

import { installShareIpcSpy } from '../../test-utils/mock-share-ws';
import { PublishLookupWorkspaceResultSchema } from '../share-types';

describe('publish.lookupWorkspace outgoing wire shape', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
  });

  it('outgoing request envelope is { id, type, params: { artifact_id } } and nothing else', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    // The default spy response is a benign D-004-J error envelope — the hook
    // will reject. We only care about the captured outgoing call, so swallow
    // the rejection.
    await expect(
      result.current.send('publish.lookupWorkspace', { artifact_id: 'some-artifact-id' }),
    ).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(1);
    const envelope = sendSpy.mock.calls[0][0] as {
      id: unknown;
      type: unknown;
      params: { artifact_id: unknown };
    };

    // id is hook-generated (uuid). Assert presence + non-empty string, not a
    // hard-coded value — the real wire contract is type + params shape.
    expect(typeof envelope.id).toBe('string');
    expect((envelope.id as string).length).toBeGreaterThan(0);

    expect(envelope.type).toBe('publish.lookupWorkspace');

    // Params shape — must mirror host struct exactly.
    expect(envelope.params).toEqual({ artifact_id: 'some-artifact-id' });
    expect(typeof envelope.params.artifact_id).toBe('string');

    // Negative assertion: no extra fields leak into params, and no
    // additional top-level keys appear beyond { id, type, params }. This
    // guards against accidental top-level spreads of param fields — the
    // exact failure mode `feedback_wire_shape_tests.md` was written to
    // catch.
    expect(Object.keys(envelope.params as object)).toEqual(['artifact_id']);
    expect(Object.keys(envelope).sort()).toEqual(['id', 'params', 'type']);
  });

  it('every call carries a fresh request id (no reuse across calls)', async () => {
    const { sendSpy } = installShareIpcSpy();

    const { useShareWs } = await import('../../hooks/use-share-ws');
    const { result } = renderHook(() => useShareWs());

    await expect(
      result.current.send('publish.lookupWorkspace', { artifact_id: 'A' }),
    ).rejects.toBeDefined();
    await expect(
      result.current.send('publish.lookupWorkspace', { artifact_id: 'B' }),
    ).rejects.toBeDefined();

    expect(sendSpy).toHaveBeenCalledTimes(2);
    const id1 = (sendSpy.mock.calls[0][0] as { id: string }).id;
    const id2 = (sendSpy.mock.calls[1][0] as { id: string }).id;
    expect(id1).not.toBe(id2);
    expect(id1.length).toBeGreaterThan(0);
    expect(id2.length).toBeGreaterThan(0);
  });
});

describe('publish.lookupWorkspace response schema (zod parse)', () => {
  it('parses status=ok (workspace resolved + folder present)', () => {
    const parsed = PublishLookupWorkspaceResultSchema.parse({
      id: 'r1',
      type: 'publish.lookupWorkspaceResult',
      artifact_id: 'A',
      status: 'ok',
      workspace_path: 'overlays/X',
      kind: 'overlay',
      name: 'X',
    });
    expect(parsed.status).toBe('ok');
    expect(parsed.workspace_path).toBe('overlays/X');
    expect(parsed.kind).toBe('overlay');
    expect(parsed.name).toBe('X');
  });

  it('parses status=missing_index (no publish-index entry at all)', () => {
    const parsed = PublishLookupWorkspaceResultSchema.parse({
      id: 'r2',
      type: 'publish.lookupWorkspaceResult',
      artifact_id: 'A',
      status: 'missing_index',
      workspace_path: null,
      kind: null,
      name: null,
    });
    expect(parsed.status).toBe('missing_index');
    expect(parsed.workspace_path).toBeNull();
    expect(parsed.kind).toBeNull();
    expect(parsed.name).toBeNull();
  });

  it('parses status=missing_folder (index entry exists but workspace folder is gone)', () => {
    const parsed = PublishLookupWorkspaceResultSchema.parse({
      id: 'r3',
      type: 'publish.lookupWorkspaceResult',
      artifact_id: 'A',
      status: 'missing_folder',
      workspace_path: null,
      kind: 'theme',
      name: 'Y',
    });
    expect(parsed.status).toBe('missing_folder');
    expect(parsed.workspace_path).toBeNull();
    // kind + name are retained from the index entry even when the folder
    // has been deleted — the renderer uses them to render a "your X named
    // 'Y' is gone" message.
    expect(parsed.kind).toBe('theme');
    expect(parsed.name).toBe('Y');
  });

  it('rejects payloads with an unknown status value', () => {
    expect(() =>
      PublishLookupWorkspaceResultSchema.parse({
        id: 'r4',
        type: 'publish.lookupWorkspaceResult',
        artifact_id: 'A',
        status: 'totally-bogus',
        workspace_path: null,
        kind: null,
        name: null,
      }),
    ).toThrow();
  });

  it('rejects payloads with the wrong response type literal', () => {
    expect(() =>
      PublishLookupWorkspaceResultSchema.parse({
        id: 'r5',
        type: 'publish.somethingElseResult',
        artifact_id: 'A',
        status: 'ok',
        workspace_path: null,
        kind: null,
        name: null,
      }),
    ).toThrow();
  });
});
