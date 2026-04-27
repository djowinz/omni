/**
 * mock-share-ws.ts — small test helper for capturing the OUTGOING wire shape
 * of `useShareWs.send(...)` calls.
 *
 * Per `feedback_wire_shape_tests.md`: tests that mock `sendMessage`'s return
 * value alone hide request-side wire bugs. The pattern that exercises the
 * REAL boundary is:
 *
 *   1. Stub `window.omni.sendShareMessage` with a `vi.fn()` (the actual IPC
 *      entry point the hook calls into).
 *   2. Invoke the hook's `send(type, params)`.
 *   3. Inspect `sendShareMessage.mock.calls[i][0]` to assert the captured
 *      `{ id, type, params }` envelope matches the expected wire shape.
 *
 * `installShareIpcSpy()` does (1) — it stubs `window.omni` with a spy on
 * `sendShareMessage` whose default response is a benign error envelope so the
 * hook's response-validation path doesn't reject in unexpected ways. The
 * caller can override the response by configuring the returned `sendSpy`.
 *
 * Shipped: `apps/desktop/renderer/hooks/use-share-ws.ts` calls
 * `window.omni.sendShareMessage({ id, type, params })` synchronously inside
 * the `send()` body — the returned `sendSpy.mock.calls[i][0]` therefore
 * captures the literal envelope the hook handed to the IPC layer.
 */

import { vi } from 'vitest';

export interface ShareIpcSpy {
  sendSpy: ReturnType<typeof vi.fn>;
  onShareEventSpy: ReturnType<typeof vi.fn>;
}

/**
 * Default response for `sendShareMessage` — a benign D-004-J error envelope.
 * The wire-shape tests don't care about the response shape; they only inspect
 * what was SENT. Returning an error envelope means the hook's response path
 * throws cleanly without running Zod schema validation against an unknown
 * `*Result` type (which would fail with PARSE_FAILED for the
 * markBackedUp/setDisplayName messages that lack a registered schema).
 */
const DEFAULT_NOOP_ERROR_RESPONSE = {
  id: 'spy-noop',
  type: 'error',
  error: {
    code: 'TEST_NOOP',
    kind: 'HostLocal' as const,
    detail: null,
    message: 'wire-shape spy: response intentionally unused',
  },
};

/**
 * Stub `window.omni` for a wire-shape test. Returns the underlying spies so
 * the test can:
 *
 *   - assert outgoing envelopes via `sendSpy.mock.calls[i][0]`
 *   - override the response by calling `sendSpy.mockResolvedValueOnce(...)`
 *     before the relevant `useShareWs().send(...)` invocation.
 *
 * Call this in `beforeEach` (or directly inside an `it`) BEFORE the dynamic
 * `import('../use-share-ws')` so the hook reads the stubbed globals on first
 * load. The hook caches `window.omni!` lookups inside its closures, so
 * stubbing late produces undefined-reference errors.
 *
 * @param options.defaultResponse — Override the default response returned by
 *   `sendShareMessage`. Defaults to `DEFAULT_NOOP_ERROR_RESPONSE` (a benign
 *   D-004-J error envelope used for wire-shape tests that only inspect the
 *   outgoing call). Pass a success frame object when the test also needs to
 *   assert the response path (e.g. context provider mount tests).
 */
export function installShareIpcSpy(
  options: { defaultResponse?: unknown } = {},
): ShareIpcSpy {
  const response =
    options.defaultResponse !== undefined
      ? options.defaultResponse
      : DEFAULT_NOOP_ERROR_RESPONSE;
  const sendSpy = vi.fn().mockResolvedValue(response);
  const onShareEventSpy = vi.fn().mockReturnValue(() => {});
  vi.stubGlobal('omni', {
    sendShareMessage: sendSpy,
    onShareEvent: onShareEventSpy,
  });
  return { sendSpy, onShareEventSpy };
}
