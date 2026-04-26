/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * recovery-card.test.tsx — Wave A2 / Task A2.7 / OWI-47.
 *
 * Spec invariant under test: INV-7.6.3 (sidecar-deleted recovery — worker
 * rejects with `AuthorNameConflict` after the local publish-index also
 * misses, and Step 4 renders an amber recovery card with two actions).
 *
 * Integration scope: this exercises the full UploadDialog → useUploadMachine
 * → useShareWs wire path for the AuthorNameConflict recovery flow.
 *
 *   1. Render `<UploadDialog open>` with `prefilledPath` to skip Step 1's
 *      source picker (the prefilled effect resolves
 *      `workspace.listPublishables`, dispatches SELECT_KIND + SELECT_ITEM,
 *      then NEXT to advance into Step 2).
 *   2. Drive the form (Name field), click Continue → walks into Step 3
 *      (Packing). The machine fires `upload.pack`, which the mocked WS
 *      bridge resolves to a valid `upload.packResult` envelope.
 *   3. Synthesise five `upload.packProgress` frames (`status: 'passed'`)
 *      via the captured `onShareEvent` callback so all five pack stages
 *      flip to passed, enabling the Publish CTA.
 *   4. Click Publish → the machine fires `upload.publish`, the mocked WS
 *      bridge returns the D-004-J error envelope with
 *      `code: 'AuthorNameConflict'` and a JSON-stringified
 *      `AuthorNameConflictDetail` blob — `useShareWs.send` throws the
 *      inner `error` object as a `ShareWsError`.
 *   5. Assert: `<PublishRecoveryCard>` renders with the title, truncated
 *      artifact id (first 12 chars + ellipsis), version `1.3.0`,
 *      "Link and update → v1.3.1" button label, and "Rename and publish
 *      new" button.
 *   6. Click "Link and update": assert the next `sendShareMessage` call is
 *      `type: 'upload.update'` with `params.artifact_id ===
 *      'ov_01J8XKZ9F'` (the existing_artifact_id from the error detail).
 *   7. Click "Rename and publish new" (in a separate render): assert the
 *      dialog navigates back to Step 2 (the header verb stays "Publish"
 *      since mode flips back to 'create' on JUMP_TO_DETAILS, and the Step 2
 *      Name input is mounted again).
 *
 * Mock pattern: `window.omni.{sendShareMessage, onShareEvent}` is stubbed
 * via `vi.stubGlobal` so the production `useShareWs` hook (and therefore
 * the production `useUploadMachine`) sees a working IPC bridge — same
 * pattern the parent `upload-dialog.test.tsx` uses. The `useShareWs` hook
 * is NEVER `vi.doMock`-ed because that pattern returns a fresh object on
 * every render and triggers infinite useEffect loops + heap OOM.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';

// ── Wire fixtures ────────────────────────────────────────────────────────────

/** Existing artifact metadata the worker surfaces inside the error.detail blob. */
const EXISTING_ARTIFACT_ID = 'ov_01J8XKZ9F';
const EXISTING_VERSION = '1.3.0';
const LAST_PUBLISHED_AT = '2026-04-18T18:12:44Z';

/** Identity bootstrap — already backed up so the publish gate doesn't open. */
const IDENTITY_RESULT = {
  type: 'identity.showResult',
  params: {
    pubkey_hex: 'cc'.repeat(32),
    fingerprint_hex: '',
    fingerprint_emoji: [],
    fingerprint_words: [],
    created_at: 0,
    backed_up: true,
  },
};

/** config.vocab — Step 2 Review's tag chips read this; keep deterministic. */
const VOCAB_RESULT = {
  type: 'config.vocabResult',
  params: { tags: ['dark', 'minimal'], version: 1 },
};

/** workspace.listPublishables — one overlay matching the test's prefilled path. */
const WORKSPACE_PATH = 'overlays/full-telemetry';
const ARTIFACT_NAME = 'Full Telemetry';
const LIST_RESULT = {
  type: 'workspace.listPublishablesResult',
  params: {
    entries: [
      {
        kind: 'overlay',
        workspace_path: WORKSPACE_PATH,
        name: ARTIFACT_NAME,
        widget_count: 4,
        modified_at: '2026-04-21T00:00:00Z',
        has_preview: false,
        sidecar: null,
      },
    ],
  },
};

/** upload.pack success envelope — runPack swallows the body but useShareWs
 *  Zod-validates the shape, so we must return a schema-valid frame. */
const PACK_RESULT = {
  type: 'upload.packResult',
  params: {
    content_hash: 'abc123',
    compressed_size: 1024,
    uncompressed_size: 2048,
    manifest: {},
    sanitize_report: {},
  },
};

/** D-004-J error envelope returned by `upload.publish` to trigger INV-7.6.3.
 *  `useShareWs.send` parses this against `ShareErrorFrameSchema` and throws
 *  the inner `error` object as a `ShareWsError`. */
const AUTHOR_NAME_CONFLICT_ENVELOPE = {
  type: 'error',
  error: {
    code: 'AuthorNameConflict',
    kind: 'Quota' as const,
    message: 'Name already taken under your identity',
    detail: JSON.stringify({
      existing_artifact_id: EXISTING_ARTIFACT_ID,
      existing_version: EXISTING_VERSION,
      last_published_at: LAST_PUBLISHED_AT,
    }),
  },
};

// ── Frame-callback capture for upload.packProgress synthesis ────────────────

type FrameCallback = (frame: unknown) => void;

interface MockBridge {
  sendShareMessage: ReturnType<typeof vi.fn>;
  fireShareEvent: FrameCallback;
}

/**
 * Stubs `window.omni` with `sendShareMessage` (request-response) and
 * `onShareEvent` (streaming). The `onShareEvent` callback is captured so
 * tests can fire `upload.packProgress` frames into the live useShareWs
 * subscription without spinning up a real WebSocket.
 *
 * `sendShareMessage` is the spy the test asserts against (e.g. to confirm
 * the Link-and-update click fires `upload.update` with the correct
 * artifact_id). The default handler dispatches by `msg.type`.
 */
function stubOmniBridge(): MockBridge {
  let captured: FrameCallback | null = null;
  const sendShareMessage = vi.fn(async (msg: { id: string; type: string; params?: unknown }) => {
    switch (msg.type) {
      case 'identity.show':
        return { id: msg.id, ...IDENTITY_RESULT };
      case 'config.vocab':
        return { id: msg.id, ...VOCAB_RESULT };
      case 'workspace.listPublishables':
        return { id: msg.id, ...LIST_RESULT };
      case 'upload.pack':
        return { id: msg.id, ...PACK_RESULT };
      case 'upload.publish':
        // D-004-J error envelope — useShareWs.send throws the inner `error`.
        return { id: msg.id, ...AUTHOR_NAME_CONFLICT_ENVELOPE };
      case 'upload.update':
        // The Link-and-update click re-fires through the machine's
        // doPublish → upload.update path. Return a successful update result
        // so the recovery flow completes cleanly (the test asserts the
        // outgoing CALL shape, not the post-update UI).
        return {
          id: msg.id,
          type: 'upload.updateResult',
          params: {
            artifact_id: EXISTING_ARTIFACT_ID,
            content_hash: 'def456',
            status: 'updated' as const,
            worker_url: 'https://example.com/' + EXISTING_ARTIFACT_ID,
          },
        };
      default:
        throw new Error('unexpected sendShareMessage in recovery-card test: ' + msg.type);
    }
  });
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage,
    onShareEvent: vi.fn().mockImplementation((cb: FrameCallback) => {
      captured = cb;
      // unsubscribe — no-op for the test's lifetime.
      return () => {};
    }),
  });
  return {
    sendShareMessage,
    fireShareEvent: (frame: unknown) => {
      if (captured === null) throw new Error('onShareEvent was never subscribed');
      captured(frame);
    },
  };
}

/** Synthesise an `upload.packProgress` frame matching `PackProgressSchema`. */
function packFrame(
  stage: 'schema' | 'content-safety' | 'asset' | 'dependency' | 'size',
  status: 'running' | 'passed' | 'failed' = 'passed',
) {
  return {
    id: 'pack-1',
    type: 'upload.packProgress',
    params: { stage, status, detail: null },
  };
}

/**
 * Walks the dialog from open → Step 4 with the AuthorNameConflict error
 * already surfaced. Returns the bridge spies so the caller can assert
 * follow-up sends (e.g. the upload.update call after Link-and-update).
 */
async function renderDialogAtRecoveryCard(): Promise<MockBridge> {
  const bridge = stubOmniBridge();
  // IdentityBackupDialog (mounted unconditionally by UploadDialog) calls
  // useBackend(); stub it to a no-op so it doesn't crash on a missing IPC
  // bridge. Same trick the parent upload-dialog.test.tsx uses.
  vi.doMock('../../../../hooks/use-backend', () => ({ useBackend: () => ({}) }));

  const { UploadDialog } = await import('../index');
  render(<UploadDialog open onOpenChange={() => {}} prefilledPath={WORKSPACE_PATH} />);

  // Wait for the prefilled effect to advance into Step 2 (Review).
  await waitFor(() => expect(screen.getByTestId('upload-name')).toBeInTheDocument());

  // Fill in the Name field (Step 2's only required field).
  fireEvent.change(screen.getByTestId('upload-name'), {
    target: { value: ARTIFACT_NAME },
  });

  // Click Continue → advances into Step 3 (Packing) and fires upload.pack.
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
    // Yield once for the form.trigger() + dispatch + runPack chain.
    await Promise.resolve();
  });
  await waitFor(() =>
    expect(screen.getByTestId('packing-stage-schema-validation')).toBeInTheDocument(),
  );

  // Synthesise pack-progress frames passing every stage.
  await act(async () => {
    bridge.fireShareEvent(packFrame('schema'));
    bridge.fireShareEvent(packFrame('content-safety'));
    bridge.fireShareEvent(packFrame('asset'));
    bridge.fireShareEvent(packFrame('dependency'));
    bridge.fireShareEvent(packFrame('size'));
  });

  // Click Publish → fires upload.publish, which the mock resolves into the
  // AuthorNameConflict error envelope. Wait for the recovery card to mount.
  const publishBtn = await screen.findByRole('button', { name: /Publish/ });
  await act(async () => {
    fireEvent.click(publishBtn);
  });
  await waitFor(() => expect(screen.getByTestId('publish-recovery-card')).toBeInTheDocument());

  return bridge;
}

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.doUnmock('../../../../hooks/use-backend');
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('UploadDialog — AuthorNameConflict recovery card (INV-7.6.3)', () => {
  it('renders the amber recovery card with title, truncated artifact id, version, and both action buttons', async () => {
    await renderDialogAtRecoveryCard();

    // Title.
    expect(screen.getByText('Name already taken')).toBeInTheDocument();

    const card = screen.getByTestId('publish-recovery-card');

    // The user-typed artifact name appears in the card body copy.
    expect(card).toHaveTextContent(ARTIFACT_NAME);
    // Truncated artifact id — the recovery-card slices the first 12 chars
    // and appends an ellipsis. EXISTING_ARTIFACT_ID is 11 chars so the
    // visible string is the full id + "…".
    expect(card).toHaveTextContent(`${EXISTING_ARTIFACT_ID}…`);
    // Existing version surfaces in the "v{X.Y.Z}" inline metadata.
    expect(card).toHaveTextContent(`v${EXISTING_VERSION}`);
    // Last-published date trims ISO-8601 to the YYYY-MM-DD prefix.
    expect(card).toHaveTextContent('published 2026-04-18');

    // Both action buttons are mounted.
    const linkAndUpdate = screen.getByTestId('publish-recovery-link-and-update');
    expect(linkAndUpdate).toBeInTheDocument();
    // Patch-bump label: 1.3.0 → 1.3.1 surfaces in the button text.
    expect(linkAndUpdate).toHaveTextContent('Link and update → v1.3.1');

    expect(screen.getByTestId('publish-recovery-rename-and-publish-new')).toHaveTextContent(
      'Rename and publish new',
    );
  });

  it('Link-and-update click fires upload.update with params.artifact_id from the error detail', async () => {
    const bridge = await renderDialogAtRecoveryCard();

    // Snapshot the call count BEFORE clicking so we can isolate the
    // upload.update call without coupling to the test's setup-phase calls.
    const callsBeforeClick = bridge.sendShareMessage.mock.calls.length;

    await act(async () => {
      fireEvent.click(screen.getByTestId('publish-recovery-link-and-update'));
    });

    // Wait for the new sendShareMessage call to land — linkAndUpdate runs
    // through publishWithGate → doPublish, which awaits identity.show before
    // firing upload.update.
    await waitFor(() => {
      expect(bridge.sendShareMessage.mock.calls.length).toBeGreaterThan(callsBeforeClick);
    });

    // Find the upload.update call in the spy's call log. There should be
    // exactly one such call across the whole test (the initial publish was
    // upload.publish, not upload.update).
    const updateCalls = bridge.sendShareMessage.mock.calls.filter(
      (args) => (args[0] as { type: string }).type === 'upload.update',
    );
    expect(updateCalls).toHaveLength(1);
    const updateMsg = updateCalls[0][0] as {
      type: string;
      params: { artifact_id: string };
    };
    expect(updateMsg.type).toBe('upload.update');
    expect(updateMsg.params.artifact_id).toBe(EXISTING_ARTIFACT_ID);
  });

  it('Rename-and-publish-new click navigates the dialog back to Step 2 (Details)', async () => {
    await renderDialogAtRecoveryCard();

    await act(async () => {
      fireEvent.click(screen.getByTestId('publish-recovery-rename-and-publish-new'));
    });

    // Step 2's Name input is mounted again, the recovery card unmounts.
    await waitFor(() => {
      expect(screen.getByTestId('upload-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('publish-recovery-card')).toBeNull();
    // Footer reverts to Step 2's Continue CTA.
    expect(screen.getByRole('button', { name: /Continue/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Retry/ })).toBeNull();
  });
});
