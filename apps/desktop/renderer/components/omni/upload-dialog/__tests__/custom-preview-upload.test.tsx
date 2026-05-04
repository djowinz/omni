/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * custom-preview-upload.test.tsx — pins the wiring that ships the user's
 * Step 2 Preview Image bytes through to the host's `upload.publish` /
 * `upload.update` / `upload.pack` calls.
 *
 * Bug pinned: the entire Preview Image picker (drag/drop + ONNX moderation +
 * IDB persist) was presentational only — `getCustomPreview` was called
 * exclusively by the component's own restore-on-mount effect, never by the
 * upload pipeline. Whatever image the user picked never reached the worker.
 *
 * Fix wires `use-upload-machine.ts::loadCustomPreviewB64` into both
 * `runPack` and `doPublish`. This test asserts a record present in IDB at
 * publish time lands on the wire under `custom_preview_b64`.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';

// ─────────────────────────────────────────────────────────────────────────────
// IDB mock — same pattern as preview-image.test.tsx.
// ─────────────────────────────────────────────────────────────────────────────

const setCustomPreviewMock = vi.fn().mockResolvedValue(undefined);
const getCustomPreviewMock = vi.fn().mockResolvedValue(null);
const removeCustomPreviewMock = vi.fn().mockResolvedValue(undefined);

vi.mock('@/lib/indexed-db/custom-preview-store', () => ({
  setCustomPreview: (...args: unknown[]) => setCustomPreviewMock(...args),
  getCustomPreview: (...args: unknown[]) => getCustomPreviewMock(...args),
  removeCustomPreview: (...args: unknown[]) => removeCustomPreviewMock(...args),
}));

// ─────────────────────────────────────────────────────────────────────────────
// Wire-shape fixtures
// ─────────────────────────────────────────────────────────────────────────────

const PUBKEY_HEX = 'cc'.repeat(32);
const WORKSPACE_PATH = 'overlays/custom-preview-fixture';
const ARTIFACT_NAME = 'Custom Preview Fixture';

const IDENTITY_RESULT = {
  type: 'identity.showResult',
  params: {
    pubkey_hex: PUBKEY_HEX,
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

const VOCAB_RESULT = {
  type: 'config.vocabResult',
  params: { tags: ['dark', 'minimal'], version: 1 },
};

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

const PUBLISH_SUCCESS = {
  type: 'upload.publishResult',
  params: {
    artifact_id: 'ov_01CCC',
    content_hash: 'def456',
    status: 'created' as const,
    worker_url: 'https://example.com/ov_01CCC',
  },
};

// ─────────────────────────────────────────────────────────────────────────────
// Bridge stub: collects every outgoing envelope so we can assert payloads.
// ─────────────────────────────────────────────────────────────────────────────

type FrameCallback = (frame: unknown) => void;

interface CapturedSends {
  sendShareMessage: ReturnType<typeof vi.fn>;
  fireShareEvent: FrameCallback;
}

function stubOmniBridge(): CapturedSends {
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
        return { id: msg.id, ...PUBLISH_SUCCESS };
      default:
        throw new Error('unexpected sendShareMessage type: ' + msg.type);
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
    fireShareEvent: (frame) => {
      if (captured === null) throw new Error('onShareEvent never subscribed');
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

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async function flush() {
  // Two micro-flushes covers the typical async chain (pack → publish).
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(() => {
  // Patch indexed-db globally so the dialog's restore-on-mount effect doesn't
  // throw (preview-image test suite already validates that path; here we just
  // care about the publish-time read).
  setCustomPreviewMock.mockClear();
  getCustomPreviewMock.mockClear();
  removeCustomPreviewMock.mockClear();
  getCustomPreviewMock.mockResolvedValue(null);

  if (typeof Blob.prototype.arrayBuffer !== 'function') {
    Object.defineProperty(Blob.prototype, 'arrayBuffer', {
      configurable: true,
      writable: true,
      value: function arrayBuffer(this: Blob) {
        return new Response(this).arrayBuffer();
      },
    });
  }
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

describe('UploadDialog — custom Preview Image rides through to upload.publish', () => {
  it('does NOT include custom_preview_b64 when IDB has no record (legacy auto-render path)', async () => {
    getCustomPreviewMock.mockResolvedValue(null);
    const bridge = stubOmniBridge();
    const { sendShareMessage } = bridge;

    const { UploadDialog } = await import('../index');
    render(
      <UploadDialog open={true} onOpenChange={() => {}} prefilledPath={WORKSPACE_PATH} />,
    );

    // Wait for Step 2 to mount
    await waitFor(() => {
      expect(screen.getByTestId('upload-name')).toBeInTheDocument();
    });

    // Step 2 → 3 (advances via Continue), Step 3 → 4 (Publish).
    await act(async () => {
      fireEvent.input(screen.getByTestId('upload-name'), {
        target: { value: ARTIFACT_NAME },
      });
      await flush();
    });
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
      await flush();
    });
    await waitFor(() =>
      expect(screen.getByTestId('packing-stage-schema-validation')).toBeInTheDocument(),
    );
    // Drive all five pack stages to passed so the Publish button enables.
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
      await flush();
    });
    await waitFor(() => {
      expect(
        sendShareMessage.mock.calls.some(
          (c) => (c[0] as { type: string }).type === 'upload.publish',
        ),
      ).toBe(true);
    });

    const publishCalls = sendShareMessage.mock.calls
      .map((c) => c[0] as { type: string; params: Record<string, unknown> })
      .filter((m) => m.type === 'upload.publish');
    expect(publishCalls).toHaveLength(1);
    expect('custom_preview_b64' in publishCalls[0].params).toBe(false);

    const packCalls = sendShareMessage.mock.calls
      .map((c) => c[0] as { type: string; params: Record<string, unknown> })
      .filter((m) => m.type === 'upload.pack');
    expect(packCalls).toHaveLength(1);
    expect('custom_preview_b64' in packCalls[0].params).toBe(false);
  });

  it('passes IDB-stored bytes as base64 in upload.pack AND upload.publish params', async () => {
    // 6 bytes of known content so we can decode + assert verbatim.
    const knownBytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]); // PNG magic prefix
    const blob = new Blob([knownBytes], { type: 'image/png' });
    getCustomPreviewMock.mockResolvedValue({
      blob,
      mimeType: 'image/png',
      size: knownBytes.length,
      addedAt: Date.now(),
    });

    const bridge = stubOmniBridge();
    const { sendShareMessage } = bridge;

    const { UploadDialog } = await import('../index');
    render(
      <UploadDialog open={true} onOpenChange={() => {}} prefilledPath={WORKSPACE_PATH} />,
    );

    await waitFor(() => {
      expect(screen.getByTestId('upload-name')).toBeInTheDocument();
    });
    await act(async () => {
      fireEvent.input(screen.getByTestId('upload-name'), {
        target: { value: ARTIFACT_NAME },
      });
      await flush();
    });
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
      await flush();
    });
    await waitFor(() =>
      expect(screen.getByTestId('packing-stage-schema-validation')).toBeInTheDocument(),
    );
    // Drive all five pack stages to passed so the Publish button enables.
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
      await flush();
    });
    await waitFor(() => {
      expect(
        sendShareMessage.mock.calls.some(
          (c) => (c[0] as { type: string }).type === 'upload.publish',
        ),
      ).toBe(true);
    });

    // Both calls must carry the base64-encoded blob.
    const expectedB64 = btoa(String.fromCharCode(...Array.from(knownBytes)));

    const packCall = sendShareMessage.mock.calls
      .map((c) => c[0] as { type: string; params: { custom_preview_b64?: string } })
      .find((m) => m.type === 'upload.pack');
    expect(packCall, 'upload.pack envelope captured').toBeDefined();
    expect(packCall!.params.custom_preview_b64).toBe(expectedB64);

    const publishCall = sendShareMessage.mock.calls
      .map((c) => c[0] as { type: string; params: { custom_preview_b64?: string } })
      .find((m) => m.type === 'upload.publish');
    expect(publishCall, 'upload.publish envelope captured').toBeDefined();
    expect(publishCall!.params.custom_preview_b64).toBe(expectedB64);

    // IDB was queried by the upload pipeline, not just by the on-mount
    // effect inside ReviewPreviewImage. Mount runs once; runPack +
    // doPublish run once each → at least 3 reads against the same key.
    const queriedWorkspacePaths = getCustomPreviewMock.mock.calls.map((c) => c[0]);
    const matchingReads = queriedWorkspacePaths.filter((p) => p === WORKSPACE_PATH).length;
    expect(matchingReads).toBeGreaterThanOrEqual(2);
  });
});
