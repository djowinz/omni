/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * retry-publish.test.tsx — pins INV-7.4.5 Retry behaviour.
 *
 * Bug pinned: prior implementation wired the Step 4 Retry button to
 * `actions.back()`, bouncing the user back to Step 3 (Packing) instead of
 * re-firing the upload. Retry must stay on Step 4 and emit a fresh
 * `upload.publish` call with the same params.
 *
 * Setup mirrors `recovery-card.test.tsx`: stub `window.omni` so the real
 * `useShareWs` runs, walk the dialog through Step 1 → Step 4 with the first
 * `upload.publish` returning a generic 5xx-style error envelope, then click
 * the Retry button and assert (a) we're still on Step 4 and (b) a second
 * `upload.publish` call was made.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';

const IDENTITY_RESULT = {
  type: 'identity.showResult',
  pubkey_hex: 'cc'.repeat(32),
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

const VOCAB_RESULT = {
  type: 'config.vocabResult',
  params: { tags: ['dark', 'minimal'], version: 1 },
};

const WORKSPACE_PATH = 'overlays/retry-fixture';
const ARTIFACT_NAME = 'Retry Fixture';

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

/** A generic non-recovery error — Retry must re-fire the publish. */
const GENERIC_PUBLISH_ERROR = {
  type: 'error',
  error: {
    code: 'UPSTREAM_5XX',
    kind: 'Network' as const,
    message: 'Worker returned 500',
    detail: 'transient',
  },
};

/** Successful publish envelope — returned the second time so Retry resolves. */
const PUBLISH_SUCCESS = {
  type: 'upload.publishResult',
  params: {
    artifact_id: 'ov_01ZZZ',
    content_hash: 'def456',
    status: 'created' as const,
    worker_url: 'https://example.com/ov_01ZZZ',
  },
};

type FrameCallback = (frame: unknown) => void;

interface MockBridge {
  sendShareMessage: ReturnType<typeof vi.fn>;
  fireShareEvent: FrameCallback;
}

function stubOmniBridge(): MockBridge {
  let captured: FrameCallback | null = null;
  let publishCallCount = 0;
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
        publishCallCount += 1;
        if (publishCallCount === 1) return { id: msg.id, ...GENERIC_PUBLISH_ERROR };
        return { id: msg.id, ...PUBLISH_SUCCESS };
      default:
        throw new Error('unexpected sendShareMessage in retry-publish test: ' + msg.type);
    }
  });
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage,
    onShareEvent: vi.fn().mockImplementation((cb: FrameCallback) => {
      captured = cb;
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

function packFrame(stage: 'schema' | 'content-safety' | 'asset' | 'dependency' | 'size') {
  return {
    id: 'pack-1',
    type: 'upload.packProgress',
    params: { stage, status: 'passed' as const, detail: null },
  };
}

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.doUnmock('../../../../hooks/use-backend');
});

describe('UploadDialog — Step 4 Retry (INV-7.4.5)', () => {
  it('Retry stays on Step 4 and re-fires upload.publish (does NOT navigate back to Packing)', async () => {
    const bridge = stubOmniBridge();
    vi.doMock('../../../../hooks/use-backend', () => ({ useBackend: () => ({}) }));

    const { UploadDialog } = await import('../index');
    render(<UploadDialog open onOpenChange={() => {}} prefilledPath={WORKSPACE_PATH} />);

    // Drive through Step 2 → Step 3 → Step 4 (publish error).
    await waitFor(() => expect(screen.getByTestId('upload-name')).toBeInTheDocument());
    fireEvent.change(screen.getByTestId('upload-name'), { target: { value: ARTIFACT_NAME } });

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(screen.getByTestId('packing-stage-schema-validation')).toBeInTheDocument(),
    );

    await act(async () => {
      bridge.fireShareEvent(packFrame('schema'));
      bridge.fireShareEvent(packFrame('content-safety'));
      bridge.fireShareEvent(packFrame('asset'));
      bridge.fireShareEvent(packFrame('dependency'));
      bridge.fireShareEvent(packFrame('size'));
    });

    const publishBtn = await screen.findByRole('button', { name: /Publish/ });
    await act(async () => {
      fireEvent.click(publishBtn);
    });
    // First publish lands as a generic error → Step 4 error view + Retry button.
    await waitFor(() => expect(screen.getByTestId('publish-error')).toBeInTheDocument());

    const publishCallsBeforeRetry = bridge.sendShareMessage.mock.calls.filter(
      (c) => (c[0] as { type: string }).type === 'upload.publish',
    ).length;
    expect(publishCallsBeforeRetry).toBe(1);

    // Click Retry.
    const retryBtn = await screen.findByRole('button', { name: /Retry/ });
    await act(async () => {
      fireEvent.click(retryBtn);
    });

    // (a) We did NOT navigate back to Packing — the publish-success card
    //     mounts directly from the in-place retry. The presence of
    //     `publish-success` is the strongest signal we stayed on Step 4.
    await waitFor(() => expect(screen.getByTestId('publish-success')).toBeInTheDocument());

    // (b) A second upload.publish call went out.
    const publishCallsAfterRetry = bridge.sendShareMessage.mock.calls.filter(
      (c) => (c[0] as { type: string }).type === 'upload.publish',
    ).length;
    expect(publishCallsAfterRetry).toBe(2);

    // (c) The Packing stage UI must NOT be visible after Retry.
    expect(screen.queryByTestId('packing-stage-schema-validation')).not.toBeInTheDocument();
  });
});
