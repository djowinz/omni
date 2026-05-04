/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * preview-image.test.tsx — Wave B1 / Task B1.1-3 / OWI-52.
 *
 * Coverage:
 *  - INV-7.2.4 default state (auto preview + AUTO tag + upload link).
 *  - INV-7.2.4 drag-active state (dashed cyan drop zone + UploadCloud icon).
 *  - INV-7.2.4 validation-error state (size > 2 MB → rose chrome).
 *  - INV-7.2.4 validation-error state (wrong MIME → rose chrome).
 *  - INV-7.7.4 + INV-7.7.5 moderation rejection (amber chrome + INV-7.7.4 copy).
 *  - INV-7.9.1 successful accept persists to IDB and renders custom thumbnail.
 *  - INV-7.9.2 X click clears IDB + reverts to auto.
 *
 * Mock surface:
 *  - `share.moderationCheck` is mocked via `vi.stubGlobal('omni', ...)` so the
 *    real `useShareWs` hook drives the request-response cycle (matches the
 *    pattern OWI-47's recovery-card test established).
 *  - `lib/indexed-db/custom-preview-store` is `vi.mock`-ed so the test
 *    asserts the calls without spinning up fake-indexeddb under jsdom (the
 *    persistence layer has its own dedicated test under `lib/indexed-db/`).
 *  - `URL.createObjectURL` / `revokeObjectURL` are stubbed because jsdom
 *    doesn't implement them; the component uses them for thumbnail src URLs
 *    and the dimension-probe path.
 *  - `HTMLImageElement.src` setter triggers an async `onload` callback so
 *    `probeDimensions()` resolves with deterministic dimensions per test.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';

// Module-scoped IDB store mocks — the component imports
// `setCustomPreview` / `getCustomPreview` / `removeCustomPreview` from
// `@/lib/indexed-db/custom-preview-store`; we replace those with spies so
// each test asserts the calls cleanly.
const setCustomPreviewMock = vi.fn().mockResolvedValue(undefined);
const getCustomPreviewMock = vi.fn().mockResolvedValue(null);
const removeCustomPreviewMock = vi.fn().mockResolvedValue(undefined);

vi.mock('@/lib/indexed-db/custom-preview-store', () => ({
  setCustomPreview: (...args: unknown[]) => setCustomPreviewMock(...args),
  getCustomPreview: (...args: unknown[]) => getCustomPreviewMock(...args),
  removeCustomPreview: (...args: unknown[]) => removeCustomPreviewMock(...args),
}));

import { ReviewPreviewImage } from '../steps/review-preview-image';

// ─────────────────────────────────────────────────────────────────────────────
// jsdom shims — `URL.createObjectURL` + `Image.onload` driver.
// ─────────────────────────────────────────────────────────────────────────────

let nextDimensions: { width: number; height: number } = { width: 1920, height: 1080 };
let nextImageDecodeShouldFail = false;
let imageLoadCallbacks: Array<() => void> = [];

function flushImageLoads() {
  const cbs = imageLoadCallbacks;
  imageLoadCallbacks = [];
  for (const cb of cbs) cb();
}

beforeEach(() => {
  // Stub `URL.createObjectURL` — return a deterministic synthetic URL so
  // tests can assert it without inventing the implementation's UUID. The
  // production hook revokes via the same module so a no-op revoker is fine.
  if (!('createObjectURL' in URL)) {
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      writable: true,
      value: vi.fn(),
    });
  }
  if (!('revokeObjectURL' in URL)) {
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      writable: true,
      value: vi.fn(),
    });
  }
  let counter = 0;
  (URL.createObjectURL as ReturnType<typeof vi.fn>) = vi.fn(() => `blob:test/${++counter}`);
  (URL.revokeObjectURL as ReturnType<typeof vi.fn>) = vi.fn();

  // Patch `Image` so `probeDimensions()` resolves with `nextDimensions`
  // synchronously after the `src` setter fires. The native HTMLImageElement
  // would only resolve after a real network/decode round-trip; we shortcut.
  class TestImage {
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    naturalWidth = 0;
    naturalHeight = 0;
    set src(_value: string) {
      const failNow = nextImageDecodeShouldFail;
      const dimsNow = { ...nextDimensions };
      // Defer to a microtask so consumers can attach listeners after `new
      // Image()` and before src assignment lands.
      imageLoadCallbacks.push(() => {
        if (failNow) {
          this.onerror?.();
          return;
        }
        this.naturalWidth = dimsNow.width;
        this.naturalHeight = dimsNow.height;
        this.onload?.();
      });
    }
  }
  // Vitest's globals + react-testing-library look at `window.Image`.
  (window as unknown as { Image: typeof window.Image }).Image =
    TestImage as unknown as typeof window.Image;

  // Reset stub-managed state.
  nextDimensions = { width: 1920, height: 1080 };
  nextImageDecodeShouldFail = false;
  imageLoadCallbacks = [];
  setCustomPreviewMock.mockClear();
  getCustomPreviewMock.mockClear();
  removeCustomPreviewMock.mockClear();
  getCustomPreviewMock.mockResolvedValue(null);

  // Polyfill `Blob.prototype.arrayBuffer` if jsdom's blob doesn't carry it
  // (older versions). The `blobToBase64` helper relies on it.
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
});

// ─────────────────────────────────────────────────────────────────────────────
// WS bridge stub (`window.omni.sendShareMessage`).
// ─────────────────────────────────────────────────────────────────────────────

interface ModerationOutcome {
  unsafe_score: number;
  label: string;
  rejected: boolean;
  /** Defaults to `'onnx-falconsai-vit-v1'` so existing tests don't have to
   *  thread it through; fixtures that exercise the dual-gate-rejection path
   *  override with `'onnx-nudenet-v1+onnx-falconsai-vit-v1'`. */
  detector?: string;
}

function stubModerationBridge(outcome: ModerationOutcome) {
  const sendShareMessage = vi.fn(async (msg: { id: string; type: string; params?: unknown }) => {
    if (msg.type !== 'share.moderationCheck') {
      throw new Error(`unexpected sendShareMessage type: ${msg.type}`);
    }
    return {
      id: msg.id,
      type: 'share.moderationCheckResult',
      params: { detector: 'onnx-falconsai-vit-v1', ...outcome },
    };
  });
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage,
    onShareEvent: vi.fn().mockImplementation(() => () => {}),
  });
  return sendShareMessage;
}

// Helper for tests that don't expect any moderation calls (e.g. validation
// errors short-circuit before the RPC). Throws if any send is observed.
function stubNoBridgeCalls() {
  const sendShareMessage = vi.fn(async () => {
    throw new Error('bridge should not be called');
  });
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage,
    onShareEvent: vi.fn().mockImplementation(() => () => {}),
  });
  return sendShareMessage;
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers — drop, drag-over, file-input.
// ─────────────────────────────────────────────────────────────────────────────

function makeFile(name: string, type: string, size: number): File {
  // jsdom's File honors size only if you populate the blob with that many
  // bytes — we use a single Uint8Array to avoid copying a 5 MB string.
  const bytes = new Uint8Array(size);
  return new File([bytes], name, { type });
}

async function dropFile(file: File) {
  const dropzone = screen.getByTestId('review-preview-image-dropzone');
  fireEvent.dragEnter(dropzone, {
    dataTransfer: { files: [file], types: ['Files'] },
  });
  await act(async () => {
    fireEvent.drop(dropzone, {
      dataTransfer: { files: [file], types: ['Files'] },
    });
    // Yield microtasks so async validation + RPC chain settle.
    flushImageLoads();
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function pickFile(file: File) {
  const input = screen.getByTestId('review-preview-image-file-input') as HTMLInputElement;
  await act(async () => {
    fireEvent.change(input, { target: { files: [file] } });
    flushImageLoads();
    await Promise.resolve();
    await Promise.resolve();
  });
}

const OVERLAY_PATH = 'overlays/test-overlay';
const AUTO_PREVIEW_SRC = 'file:///c:/test/.omni-preview.png';

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

describe('ReviewPreviewImage (INV-7.2.4 / INV-7.7.* / INV-7.9.*)', () => {
  it('renders the auto state with AUTO tag + upload link when IDB has nothing stored', async () => {
    stubNoBridgeCalls();
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    // Wait for the IDB read to settle so the component definitively chose
    // the auto state. `getCustomPreview` is awaited inside the mount effect.
    await waitFor(() => expect(getCustomPreviewMock).toHaveBeenCalledWith(OVERLAY_PATH));
    expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument();
    // The "AUTO" corner badge is rendered as the literal "Auto" string;
    // exact-match avoids colliding with the `<img>` alt text.
    expect(screen.getByText('Auto')).toBeInTheDocument();
    expect(screen.getByTestId('review-preview-image-upload-link')).toHaveTextContent(
      'Upload custom image',
    );
  });

  it('switches to drag-active state on dragEnter (cyan dashed drop zone)', async () => {
    stubNoBridgeCalls();
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    const dropzone = screen.getByTestId('review-preview-image-dropzone');
    fireEvent.dragEnter(dropzone, { dataTransfer: { files: [], types: [] } });
    expect(screen.getByTestId('review-preview-image-drag-active')).toBeInTheDocument();
    expect(screen.getByText('Drop image to replace auto preview')).toBeInTheDocument();
  });

  it('renders the size-error state when the dropped file exceeds 2 MB', async () => {
    stubNoBridgeCalls();
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    const tooLarge = makeFile('huge.png', 'image/png', 3 * 1024 * 1024);
    await dropFile(tooLarge);
    expect(screen.getByTestId('review-preview-image-error')).toBeInTheDocument();
    expect(screen.getByText('File too large')).toBeInTheDocument();
    expect(screen.getByText(/huge\.png/)).toBeInTheDocument();
    // Size short-circuits before MIME-passing files reach moderation, so
    // setCustomPreview must not have fired.
    expect(setCustomPreviewMock).not.toHaveBeenCalled();
  });

  it('renders the format-error state when the dropped file has a non-image MIME', async () => {
    stubNoBridgeCalls();
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    const wrongType = makeFile('shot.gif', 'image/gif', 100 * 1024);
    await dropFile(wrongType);
    expect(screen.getByTestId('review-preview-image-error')).toBeInTheDocument();
    expect(screen.getByText('Wrong format')).toBeInTheDocument();
    expect(setCustomPreviewMock).not.toHaveBeenCalled();
  });

  it('renders the moderation-rejection state with amber chrome and INV-7.7.4 copy on ONNX rejection', async () => {
    stubModerationBridge({ unsafe_score: 0.93, label: 'EXPOSED_BREAST', rejected: true });
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    const file = makeFile('shot.png', 'image/png', 50 * 1024);
    await pickFile(file);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-moderation')).toBeInTheDocument(),
    );
    expect(screen.getByText("Image can't be used")).toBeInTheDocument();
    expect(
      screen.getByText(/shot\.png was flagged by local content safety checks/),
    ).toBeInTheDocument();
    // INV-7.7.6 detail block — assertion targets the data-testid so an
    // expanded label set doesn't churn the regex.
    const detail = screen.getByTestId('review-preview-image-moderation-detail');
    expect(detail).toHaveTextContent('Moderation:ClientRejected');
    expect(detail).toHaveTextContent('onnx-falconsai-vit-v1');
    expect(detail).toHaveTextContent('0.93');
    // Critically: the rejected file is NEVER persisted to IDB.
    expect(setCustomPreviewMock).not.toHaveBeenCalled();
  });

  it('persists to IDB and renders the custom-thumbnail state on a clean accept', async () => {
    const send = stubModerationBridge({ unsafe_score: 0.05, label: 'safe', rejected: false });
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    const file = makeFile('clean.png', 'image/png', 200 * 1024);
    await pickFile(file);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-custom')).toBeInTheDocument(),
    );
    expect(screen.getByText('clean.png')).toBeInTheDocument();
    expect(setCustomPreviewMock).toHaveBeenCalledWith(OVERLAY_PATH, file, 'image/png');
    // The host RPC was actually invoked (image_base64 was sent).
    expect(send).toHaveBeenCalledTimes(1);
    const call = send.mock.calls[0][0];
    expect(call.type).toBe('share.moderationCheck');
    expect(typeof (call.params as { image_base64: string }).image_base64).toBe('string');
  });

  it('removes the IDB entry and reverts to auto state when the X badge is clicked', async () => {
    stubModerationBridge({ unsafe_score: 0.05, label: 'safe', rejected: false });
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    const file = makeFile('clean.png', 'image/png', 200 * 1024);
    await pickFile(file);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-custom')).toBeInTheDocument(),
    );

    const removeBtn = screen.getByTestId('review-preview-image-remove');
    await act(async () => {
      fireEvent.click(removeBtn);
      await Promise.resolve();
    });

    await waitFor(() => expect(removeCustomPreviewMock).toHaveBeenCalledWith(OVERLAY_PATH));
    expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument();
  });

  it('restores a previously-stored custom preview from IDB on mount (INV-7.9.2)', async () => {
    stubNoBridgeCalls();
    const persistedBlob = new Blob([new Uint8Array(64)], { type: 'image/png' });
    getCustomPreviewMock.mockResolvedValueOnce({
      blob: persistedBlob,
      mimeType: 'image/png',
      size: 64,
      addedAt: Date.now(),
    });
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-custom')).toBeInTheDocument(),
    );
  });

  // ─── Regenerate auto-preview button ──────────────────────────────────────
  //
  // Wires up a Step 2 affordance that re-runs the host-side `save_preview`
  // pipeline without requiring the user to actually re-save the source file.
  // The host writes the new bytes to the existing on-disk path and returns
  // an epoch we append as `?v=<n>` to bust the browser cache for the
  // `omni-preview://` URL.
  it('does not render the Regenerate button when `kind` prop is omitted', async () => {
    stubNoBridgeCalls();
    render(<ReviewPreviewImage overlayPath={OVERLAY_PATH} autoPreviewSrc={AUTO_PREVIEW_SRC} />);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    expect(screen.queryByTestId('review-preview-image-regenerate-link')).toBeNull();
  });

  it('renders the Regenerate button in the auto state when `kind` is provided', async () => {
    stubNoBridgeCalls();
    render(
      <ReviewPreviewImage
        overlayPath={OVERLAY_PATH}
        autoPreviewSrc={AUTO_PREVIEW_SRC}
        kind="overlay"
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    expect(screen.getByTestId('review-preview-image-regenerate-link')).toBeInTheDocument();
    expect(screen.getByTestId('review-preview-image-regenerate-link')).toHaveTextContent(
      'Regenerate',
    );
  });

  it('sends share.regeneratePreview with the workspace_path + kind on click', async () => {
    const sendShareMessage = vi.fn(
      async (msg: { id: string; type: string; params?: unknown }) => {
        if (msg.type !== 'share.regeneratePreview') {
          throw new Error(`unexpected sendShareMessage type: ${msg.type}`);
        }
        return {
          id: msg.id,
          type: 'share.regeneratePreviewResult',
          params: { regenerated_at: 1_777_900_000 },
        };
      },
    );
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage,
      onShareEvent: vi.fn().mockImplementation(() => () => {}),
    });

    render(
      <ReviewPreviewImage
        overlayPath={OVERLAY_PATH}
        autoPreviewSrc={AUTO_PREVIEW_SRC}
        kind="overlay"
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );

    const button = screen.getByTestId('review-preview-image-regenerate-link');
    await act(async () => {
      fireEvent.click(button);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(sendShareMessage).toHaveBeenCalledTimes(1);
    const sentEnvelope = sendShareMessage.mock.calls[0][0] as {
      type: string;
      params: { workspace_path: string; kind: string };
    };
    expect(sentEnvelope.type).toBe('share.regeneratePreview');
    expect(sentEnvelope.params).toEqual({
      workspace_path: OVERLAY_PATH,
      kind: 'overlay',
    });
  });

  // ─── Moderation-gate callback (footer Continue button gate) ─────────────
  //
  // Step 2's Continue button is gated on the moderation-rejected state via
  // an `onModerationStateChange` callback. Without this, a user could
  // advance past a rose-chrome "Image can't be used" tile and publish — the
  // host would still derive the actual thumbnail server-side, but the UX is
  // misleading. These tests pin the contract that the callback fires `true`
  // when the rejected state is entered, `false` on every other state, and
  // `false` on unmount (so the gate never sticks past the dialog closing).
  it('fires onModerationStateChange(true) when an image gets flagged by moderation', async () => {
    const onModerationStateChange = vi.fn();
    stubModerationBridge({ unsafe_score: 0.93, label: 'EXPOSED_BREAST', rejected: true });
    render(
      <ReviewPreviewImage
        overlayPath={OVERLAY_PATH}
        autoPreviewSrc={AUTO_PREVIEW_SRC}
        onModerationStateChange={onModerationStateChange}
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );

    // Pre-rejection: callback was fired with `false` on mount (auto state).
    expect(onModerationStateChange).toHaveBeenLastCalledWith(false);

    const file = makeFile('flagged.png', 'image/png', 200 * 1024);
    await pickFile(file);
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-moderation')).toBeInTheDocument(),
    );

    expect(onModerationStateChange).toHaveBeenLastCalledWith(true);
  });

  it('fires onModerationStateChange(false) when the rejected image is cleared via the X badge', async () => {
    const onModerationStateChange = vi.fn();
    stubModerationBridge({ unsafe_score: 0.93, label: 'EXPOSED_BREAST', rejected: true });
    render(
      <ReviewPreviewImage
        overlayPath={OVERLAY_PATH}
        autoPreviewSrc={AUTO_PREVIEW_SRC}
        onModerationStateChange={onModerationStateChange}
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    await pickFile(makeFile('flagged.png', 'image/png', 200 * 1024));
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-moderation')).toBeInTheDocument(),
    );
    expect(onModerationStateChange).toHaveBeenLastCalledWith(true);

    // Click the X to revert to auto.
    await act(async () => {
      fireEvent.click(screen.getByTestId('review-preview-image-remove'));
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    expect(onModerationStateChange).toHaveBeenLastCalledWith(false);
  });

  it('fires onModerationStateChange(false) on unmount so the gate releases when the dialog closes', async () => {
    const onModerationStateChange = vi.fn();
    stubModerationBridge({ unsafe_score: 0.93, label: 'EXPOSED_BREAST', rejected: true });
    const { unmount } = render(
      <ReviewPreviewImage
        overlayPath={OVERLAY_PATH}
        autoPreviewSrc={AUTO_PREVIEW_SRC}
        onModerationStateChange={onModerationStateChange}
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );
    await pickFile(makeFile('flagged.png', 'image/png', 200 * 1024));
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-moderation')).toBeInTheDocument(),
    );
    expect(onModerationStateChange).toHaveBeenLastCalledWith(true);

    onModerationStateChange.mockClear();
    unmount();
    expect(onModerationStateChange).toHaveBeenLastCalledWith(false);
  });

  it('appends the regenerated_at epoch as ?v=<n> on the auto-preview <img> after success', async () => {
    const sendShareMessage = vi.fn(async (msg: { id: string }) => ({
      id: msg.id,
      type: 'share.regeneratePreviewResult',
      params: { regenerated_at: 1_777_900_001 },
    }));
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage,
      onShareEvent: vi.fn().mockImplementation(() => () => {}),
    });

    render(
      <ReviewPreviewImage
        overlayPath={OVERLAY_PATH}
        autoPreviewSrc={AUTO_PREVIEW_SRC}
        kind="overlay"
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId('review-preview-image-auto')).toBeInTheDocument(),
    );

    // Pre-click: <img> uses the prop-provided autoPreviewSrc unchanged.
    const imgBefore = screen.getByAltText('Auto-generated overlay preview') as HTMLImageElement;
    expect(imgBefore.src).toBe(AUTO_PREVIEW_SRC);

    await act(async () => {
      fireEvent.click(screen.getByTestId('review-preview-image-regenerate-link'));
      await Promise.resolve();
      await Promise.resolve();
    });

    // Post-click: the component constructs a fresh `omni-preview://` URL from
    // (overlayPath, kind) and appends `?v=<regenerated_at>` so the browser
    // refetches the freshly-written PNG.
    const imgAfter = screen.getByAltText('Auto-generated overlay preview') as HTMLImageElement;
    expect(imgAfter.src).toBe(
      `omni-preview://${OVERLAY_PATH}/.omni-preview.png?v=1777900001`,
    );
  });
});
