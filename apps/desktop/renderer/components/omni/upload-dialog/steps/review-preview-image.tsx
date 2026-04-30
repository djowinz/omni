/**
 * ReviewPreviewImage — Step 2 Preview Image field.
 *
 * Spec: INV-7.2.4 (4 visual states), INV-7.7.2 site #1 (ONNX accept gate),
 * INV-7.7.3 (threshold 0.8 — applied host-side; renderer never reapplies),
 * INV-7.7.4 + INV-7.7.5 (amber moderation rejection chrome + copy),
 * INV-7.7.6 (collapsible detail block with detector + confidence),
 * INV-7.9.1 + INV-7.9.2 (IndexedDB persistence keyed by `overlayPath`).
 *
 * Mockups:
 *  - `custom-image-upload.html` (states A auto-default / B drag-active /
 *    C custom-with-validation-error)
 *  - `moderation-reject.html` (left panel — client-side reject)
 *
 * The component is fully self-contained: it owns its drag-over state, the
 * IndexedDB read/write/delete cycle (via `lib/indexed-db/custom-preview-store`),
 * the file-validation pipeline (MIME → size → dimensions → ONNX), and the
 * 5-state visual switch. The parent `review.tsx` simply embeds it between
 * Description and Tags — no shared form state required (custom previews are
 * IDB-only, never written into the react-hook-form `UploadFormValues`).
 */

import { AlertCircle, AlertTriangle, UploadCloud } from 'lucide-react';
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';

import {
  getCustomPreview,
  removeCustomPreview,
  setCustomPreview,
} from '@/lib/indexed-db/custom-preview-store';
import { useShareWs } from '@/hooks/use-share-ws';

const MAX_BYTES = 2 * 1024 * 1024;
const MAX_DIMENSION = 2000;
/** Lowercase MIME allowlist matching the file picker's `accept=` attribute. */
const ACCEPTED_MIME = new Set(['image/png', 'image/jpeg', 'image/webp']);

export interface ReviewPreviewImageProps {
  /**
   * Workspace-relative overlay path — used as the IndexedDB key for the
   * custom-preview record. Stable across dialog opens, so reopening the
   * dialog on the same overlay restores the previously-uploaded custom.
   */
  overlayPath: string;
  /**
   * `file://` URL of the auto-generated `.omni-preview.png` (host-side
   * save-time render). May be `null` when the overlay has no preview yet
   * — the default-state thumbnail then renders a zinc gradient with the
   * AUTO tag and no image fill.
   */
  autoPreviewSrc: string | null;
}

/**
 * Visual state machine. The mapping to mockup states:
 *
 *   `auto`          — State A (auto preview, default)
 *   `drag-active`   — State B (drop zone)
 *   `custom`        — State C (custom selected, valid)
 *   `error`         — State C variant (validation or technical error, rose chrome)
 *   `moderation`    — moderation-reject.html left panel (amber chrome, INV-7.7.5)
 *
 * `moderation` is split from `error` so the chrome color (amber #f59e0b vs
 * rose #f43f5e) and the INV-7.7.4 copy are rendered without conditional
 * branching inside one unified state.
 */
type State =
  | { kind: 'auto' }
  | { kind: 'drag-active' }
  | { kind: 'custom'; fileName: string; size: number; thumbnailUrl: string }
  | { kind: 'error'; fileName: string; size: number; reason: ValidationReason }
  | {
      kind: 'moderation';
      fileName: string;
      size: number;
      label: string;
      confidence: number;
      detector: string;
    };

type ValidationReason =
  | { type: 'mime' }
  | { type: 'size' }
  | { type: 'dimensions'; width: number; height: number }
  | { type: 'decode' };

/** Read a Blob as a base64 string (no `data:` prefix). */
async function blobToBase64(blob: Blob): Promise<string> {
  const buf = await blob.arrayBuffer();
  // Encode in 32 KB chunks so very-large bytes don't blow the call stack
  // through `String.fromCharCode.apply` — a 2 MB image yields ~64 chunks.
  const bytes = new Uint8Array(buf);
  const chunkSize = 32 * 1024;
  let binary = '';
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode.apply(null, Array.from(chunk));
  }
  return btoa(binary);
}

/** Probe image dimensions via a temporary `<img>` element (renderer-side). */
async function probeDimensions(blob: Blob): Promise<{ width: number; height: number }> {
  const url = URL.createObjectURL(blob);
  try {
    return await new Promise<{ width: number; height: number }>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
      img.onerror = () => reject(new Error('image decode failed'));
      img.src = url;
    });
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** Format a byte count for display ("4.8 MB", "382 KB"). */
function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  return `${bytes} B`;
}

export function ReviewPreviewImage({ overlayPath, autoPreviewSrc }: ReviewPreviewImageProps) {
  const inputId = useId();
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  // Track the live custom thumbnail's object URL so we can revoke it on
  // replace / remove. Auto previews use a `file://` URL we never own.
  const customUrlRef = useRef<string | null>(null);
  const [state, setState] = useState<State>({ kind: 'auto' });
  const ws = useShareWs();

  const replaceCustomUrl = useCallback((next: string | null) => {
    if (customUrlRef.current && customUrlRef.current !== next) {
      URL.revokeObjectURL(customUrlRef.current);
    }
    customUrlRef.current = next;
  }, []);

  // INV-7.9.2: on mount, query IDB for any previously-stored custom preview.
  // Errors from `getCustomPreview` are swallowed and the component falls back
  // to the auto state — the IDB read is opportunistic restoration, not a
  // correctness gate. Failures here include:
  //   - jsdom test runs where `indexedDB` isn't defined (integration tests
  //     for unrelated dialog flows that mount Step 2 — recovery-card,
  //     sidecar-restore, double-post-guard, etc.). Without this catch each
  //     such test emits an unhandled rejection that fails vitest's process
  //     exit even when the test assertions all pass.
  //   - Production failures (DB locked, quota exceeded, private-mode storage
  //     denied). The only effect of skipping the read is the user has to
  //     re-pick their custom preview; auto state remains valid.
  // Cross-sub-spec note (per invariant #23 / writing-lessons §C7):
  //   discovered while wiring this component into `review.tsx` for OWI-53
  //   (Task B1.2). The component shipped under OWI-52 (Task B1.1-3) without
  //   a guard because its standalone tests vi.mock the IDB module — the
  //   integration suite, which doesn't mock it, surfaces the missing catch.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let record: Awaited<ReturnType<typeof getCustomPreview>> = null;
      try {
        record = await getCustomPreview(overlayPath);
      } catch {
        return;
      }
      if (cancelled || record === null) return;
      const url = URL.createObjectURL(record.blob);
      replaceCustomUrl(url);
      // Filename isn't persisted (the IDB record carries blob + mime + size
      // + addedAt only). For restored entries we render a generic label —
      // the round-trip through Blob loses the original `File.name`.
      setState({
        kind: 'custom',
        fileName: 'preview',
        size: record.size,
        thumbnailUrl: url,
      });
    })();
    return () => {
      cancelled = true;
    };
  }, [overlayPath, replaceCustomUrl]);

  // Always release the live object URL when the component unmounts.
  useEffect(() => {
    return () => {
      if (customUrlRef.current) {
        URL.revokeObjectURL(customUrlRef.current);
        customUrlRef.current = null;
      }
    };
  }, []);

  const validateAndAccept = useCallback(
    async (file: File) => {
      // Order per INV-7.2.4 + custom-image-upload.html behavior list:
      //   MIME → size → dimensions → ONNX moderation. First failure wins.
      const mime = file.type.toLowerCase();
      if (!ACCEPTED_MIME.has(mime)) {
        setState({
          kind: 'error',
          fileName: file.name,
          size: file.size,
          reason: { type: 'mime' },
        });
        return;
      }
      if (file.size > MAX_BYTES) {
        setState({
          kind: 'error',
          fileName: file.name,
          size: file.size,
          reason: { type: 'size' },
        });
        return;
      }
      let dims: { width: number; height: number };
      try {
        dims = await probeDimensions(file);
      } catch {
        setState({
          kind: 'error',
          fileName: file.name,
          size: file.size,
          reason: { type: 'decode' },
        });
        return;
      }
      if (dims.width > MAX_DIMENSION || dims.height > MAX_DIMENSION) {
        setState({
          kind: 'error',
          fileName: file.name,
          size: file.size,
          reason: { type: 'dimensions', width: dims.width, height: dims.height },
        });
        return;
      }
      // INV-7.7.2 site #1 — host-side ONNX gate. The host applies the
      // INV-7.7.3 threshold (0.8) and returns the precomputed `rejected`
      // boolean. Failed RPCs (e.g. `Moderation:NotInitialized` while host
      // startup wiring is pending) surface as a generic decode error so
      // the user sees actionable copy instead of a crash.
      const image_base64 = await blobToBase64(file);
      let result;
      try {
        result = await ws.send('share.moderationCheck', { image_base64 });
      } catch {
        setState({
          kind: 'error',
          fileName: file.name,
          size: file.size,
          reason: { type: 'decode' },
        });
        return;
      }
      const { unsafe_score, label, detector, rejected } = result.params;
      if (rejected) {
        setState({
          kind: 'moderation',
          fileName: file.name,
          size: file.size,
          label,
          confidence: unsafe_score,
          detector,
        });
        return;
      }
      // Pass: persist to IDB + render the custom-thumbnail state.
      await setCustomPreview(overlayPath, file, mime);
      const url = URL.createObjectURL(file);
      replaceCustomUrl(url);
      setState({
        kind: 'custom',
        fileName: file.name,
        size: file.size,
        thumbnailUrl: url,
      });
    },
    [overlayPath, ws, replaceCustomUrl],
  );

  const handleFileInput = useCallback(
    async (event: React.ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0];
      // Reset the input so re-selecting the same file fires `onChange` again.
      event.target.value = '';
      if (!file) return;
      await validateAndAccept(file);
    },
    [validateAndAccept],
  );

  const openFilePicker = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleDragEnter = useCallback((event: React.DragEvent) => {
    event.preventDefault();
    setState((prev) => (prev.kind === 'drag-active' ? prev : { kind: 'drag-active' }));
  }, []);

  const handleDragOver = useCallback((event: React.DragEvent) => {
    event.preventDefault();
  }, []);

  const handleDragLeave = useCallback((event: React.DragEvent) => {
    event.preventDefault();
    // Only revert to auto if leaving the drop zone (not entering a child).
    if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
    setState((prev) => (prev.kind === 'drag-active' ? { kind: 'auto' } : prev));
  }, []);

  const handleDrop = useCallback(
    async (event: React.DragEvent) => {
      event.preventDefault();
      const file = event.dataTransfer.files?.[0];
      if (!file) {
        setState((prev) => (prev.kind === 'drag-active' ? { kind: 'auto' } : prev));
        return;
      }
      await validateAndAccept(file);
    },
    [validateAndAccept],
  );

  const handleRemove = useCallback(async () => {
    await removeCustomPreview(overlayPath);
    replaceCustomUrl(null);
    setState({ kind: 'auto' });
  }, [overlayPath, replaceCustomUrl]);

  return (
    <div data-testid="review-preview-image" className="flex flex-col gap-1.5">
      <span className="text-[13px] font-semibold text-[#FAFAFA]">Preview Image</span>
      {/* Hidden file picker — driven by the "Upload custom image" link or
          the file picker click handler in tests. `accept` mirrors
          `ACCEPTED_MIME` so the OS dialog filters the visible files. */}
      <input
        ref={fileInputRef}
        id={inputId}
        data-testid="review-preview-image-file-input"
        type="file"
        accept="image/png,image/jpeg,image/webp"
        className="hidden"
        onChange={handleFileInput}
      />
      <PreviewBody
        state={state}
        autoPreviewSrc={autoPreviewSrc}
        onOpenPicker={openFilePicker}
        onRemove={handleRemove}
        onDragEnter={handleDragEnter}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      />
    </div>
  );
}

interface PreviewBodyProps {
  state: State;
  autoPreviewSrc: string | null;
  onOpenPicker: () => void;
  onRemove: () => void | Promise<void>;
  onDragEnter: (e: React.DragEvent) => void;
  onDragOver: (e: React.DragEvent) => void;
  onDragLeave: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent) => void | Promise<void>;
}

function PreviewBody({
  state,
  autoPreviewSrc,
  onOpenPicker,
  onRemove,
  onDragEnter,
  onDragOver,
  onDragLeave,
  onDrop,
}: PreviewBodyProps) {
  // A single drop-target wrapper — mockup INV-7.2.4 says the entire row is
  // the drop target. Drag handlers always live on the wrapper; the inner
  // body switches by `state.kind`.
  return (
    <div
      data-testid="review-preview-image-dropzone"
      onDragEnter={onDragEnter}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      {state.kind === 'auto' && (
        <AutoView autoPreviewSrc={autoPreviewSrc} onOpenPicker={onOpenPicker} />
      )}
      {state.kind === 'drag-active' && <DragActiveView />}
      {state.kind === 'custom' && <CustomView state={state} onRemove={onRemove} />}
      {state.kind === 'error' && (
        <ErrorView state={state} onOpenPicker={onOpenPicker} onRemove={onRemove} />
      )}
      {state.kind === 'moderation' && (
        <ModerationView state={state} onOpenPicker={onOpenPicker} onRemove={onRemove} />
      )}
    </div>
  );
}

function AutoView({
  autoPreviewSrc,
  onOpenPicker,
}: {
  autoPreviewSrc: string | null;
  onOpenPicker: () => void;
}) {
  return (
    <div data-testid="review-preview-image-auto" className="flex gap-3">
      <div
        className="relative h-20 w-[140px] flex-shrink-0 overflow-hidden rounded-md"
        style={{ background: 'linear-gradient(135deg, #27272A, #3f3f46)' }}
      >
        {autoPreviewSrc !== null && (
          <img
            src={autoPreviewSrc}
            alt="Auto-generated overlay preview"
            className="h-full w-full object-cover"
          />
        )}
        <div
          className="absolute left-1 top-1 rounded-sm px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wider text-[#a1a1aa]"
          style={{ background: 'rgba(9,9,11,0.75)' }}
        >
          Auto
        </div>
      </div>
      <div className="flex-1 text-[12px] leading-relaxed text-[#a1a1aa]">
        Using auto-generated preview from your overlay.
        <br />
        <button
          type="button"
          onClick={onOpenPicker}
          data-testid="review-preview-image-upload-link"
          className="cursor-pointer bg-transparent p-0 text-[#00D9FF] hover:underline"
        >
          Upload custom image
        </button>
        <span> · drag-drop supported</span>
        <br />
        <span className="text-[#71717a]">1920×1080 recommended · PNG/JPG/WebP · ≤2 MB</span>
      </div>
    </div>
  );
}

function DragActiveView() {
  return (
    <div
      data-testid="review-preview-image-drag-active"
      className="rounded-lg p-[22px] text-center"
      style={{
        border: '2px dashed #00D9FF',
        background: 'rgba(0,217,255,0.06)',
      }}
    >
      <div className="mb-2 flex justify-center text-[#00D9FF]">
        <UploadCloud size={28} strokeWidth={1.75} />
      </div>
      <div className="mb-1 text-[13px] font-semibold text-[#00D9FF]">
        Drop image to replace auto preview
      </div>
      <div className="text-[11px] text-[#a1a1aa]">
        PNG / JPG / WebP · up to 2 MB · 1920×1080 recommended
      </div>
    </div>
  );
}

function CustomView({
  state,
  onRemove,
}: {
  state: Extract<State, { kind: 'custom' }>;
  onRemove: () => void | Promise<void>;
}) {
  return (
    <div data-testid="review-preview-image-custom" className="flex gap-3">
      <div className="relative h-20 w-[140px] flex-shrink-0 overflow-hidden rounded-md">
        <img
          src={state.thumbnailUrl}
          alt={`Custom preview: ${state.fileName}`}
          className="h-full w-full object-cover"
        />
        <RemoveBadge onRemove={onRemove} testId="review-preview-image-remove" />
      </div>
      <div className="flex-1 text-[12px] leading-relaxed text-[#a1a1aa]">
        <span className="font-medium text-[#FAFAFA]">{state.fileName}</span>
        <br />
        <span>{formatSize(state.size)}</span>
      </div>
    </div>
  );
}

function ErrorView({
  state,
  onOpenPicker,
  onRemove,
}: {
  state: Extract<State, { kind: 'error' }>;
  onOpenPicker: () => void;
  onRemove: () => void | Promise<void>;
}) {
  const { title, body } = useErrorCopy(state);
  return (
    <div data-testid="review-preview-image-error" className="flex gap-3">
      <div
        className="relative flex h-20 w-[140px] flex-shrink-0 items-center justify-center overflow-hidden rounded-md"
        style={{
          border: '1px solid rgba(244,63,94,0.6)',
          background: 'rgba(244,63,94,0.08)',
        }}
      >
        <AlertCircle size={26} strokeWidth={1.75} className="text-[#f43f5e]" />
        <RemoveBadge onRemove={onRemove} testId="review-preview-image-remove" />
      </div>
      <div className="flex-1 text-[12px] leading-relaxed text-[#fecdd3]">
        <span className="font-semibold text-[#f43f5e]">{title}</span>
        <br />
        <span className="text-[#a1a1aa]">
          {state.fileName} — {formatSize(state.size)}
        </span>
        <br />
        <span className="text-[#71717a]">
          {body}{' '}
          <button
            type="button"
            onClick={onOpenPicker}
            data-testid="review-preview-image-retry-link"
            className="cursor-pointer bg-transparent p-0 text-[#00D9FF] hover:underline"
          >
            choose another file
          </button>
          .
        </span>
      </div>
    </div>
  );
}

function ModerationView({
  state,
  onOpenPicker,
  onRemove,
}: {
  state: Extract<State, { kind: 'moderation' }>;
  onOpenPicker: () => void;
  onRemove: () => void | Promise<void>;
}) {
  // INV-7.7.4 copy + INV-7.7.5 amber chrome (#f59e0b).
  return (
    <div data-testid="review-preview-image-moderation">
      <div className="flex gap-3">
        <div
          className="relative flex h-20 w-[140px] flex-shrink-0 items-center justify-center overflow-hidden rounded-md"
          style={{
            border: '1px solid rgba(245,158,11,0.6)',
            background: 'rgba(245,158,11,0.06)',
          }}
        >
          <AlertTriangle size={26} strokeWidth={1.75} className="text-[#f59e0b]" />
          <RemoveBadge onRemove={onRemove} testId="review-preview-image-remove" />
        </div>
        <div className="flex-1 text-[12px] leading-relaxed">
          <span className="font-semibold text-[#f59e0b]">Image can&apos;t be used</span>
          <br />
          <span className="text-[#d4d4d8]">
            {state.fileName} was flagged by local content safety checks.
          </span>
          <br />
          <span className="text-[#71717a]">
            Please choose a different image or use the{' '}
            <button
              type="button"
              onClick={onOpenPicker}
              data-testid="review-preview-image-moderation-retry-link"
              className="cursor-pointer bg-transparent p-0 text-[#00D9FF] hover:underline"
            >
              auto-generated preview
            </button>
            .
          </span>
        </div>
      </div>
      {/* INV-7.7.6 — collapsible / dev-only output. Rendered as a static
          monospace strip; hiding behind a chevron is left to a follow-up. */}
      <div
        data-testid="review-preview-image-moderation-detail"
        className="mt-2.5 rounded-md px-2.5 py-2 font-mono text-[10px] leading-snug text-[#71717a]"
        style={{ background: '#0A0A0B', border: '1px solid #27272A' }}
      >
        code <span className="text-[#a1a1aa]">Moderation:ClientRejected</span> · detector{' '}
        <span className="text-[#a1a1aa]">{state.detector}</span> · confidence{' '}
        <span className="text-[#a1a1aa]">{state.confidence.toFixed(2)}</span>
        {state.label !== '' && (
          <>
            {' '}
            · label <span className="text-[#a1a1aa]">{state.label}</span>
          </>
        )}
      </div>
    </div>
  );
}

function RemoveBadge({
  onRemove,
  testId,
}: {
  onRemove: () => void | Promise<void>;
  testId: string;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      aria-label="Remove custom preview"
      onClick={() => {
        void onRemove();
      }}
      className="absolute right-1 top-1 flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded-full border-0 p-0 text-[11px] text-[#FAFAFA]"
      style={{ background: 'rgba(9,9,11,0.9)' }}
    >
      ×
    </button>
  );
}

/** Map `ValidationReason` to title + body copy. Memoized per state ref. */
function useErrorCopy(state: Extract<State, { kind: 'error' }>) {
  return useMemo(() => {
    switch (state.reason.type) {
      case 'mime':
        return {
          title: 'Wrong format',
          body: 'Use a PNG, JPEG, or WebP file, or',
        };
      case 'size':
        return {
          title: 'File too large',
          body: 'Max 2 MB. Re-export at lower quality, or',
        };
      case 'dimensions':
        return {
          title: 'Image too large',
          body: `Max 2000×2000 px (was ${state.reason.width}×${state.reason.height}). Resize and`,
        };
      case 'decode':
        return {
          title: "Couldn't process image",
          body: 'The file may be corrupt or unreadable.',
        };
    }
  }, [state.reason]);
}
