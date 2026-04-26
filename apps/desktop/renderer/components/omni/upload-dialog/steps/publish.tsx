/**
 * Step 4 — Upload (uploading / success / error / recovery).
 *
 * Spec: §7.4 + INV-7.4.* + INV-7.6.3.
 * Mockups: `step4-upload.html`, `sidecar-deleted-recovery.html` Flow 2.
 *
 * Three mutually-exclusive states driven by `state.uploadState`:
 *   - 'uploading' — cyan ring spinner + linear progress bar + phase text.
 *     Subscribes (transitively, via the parent's upload-machine hook) to
 *     `upload.publishProgress` frames. The parent resolves `progress` to
 *     `{ phase, done, total }` before passing it in.
 *   - 'success'   — emerald ring CheckCircle + artifact summary card with
 *     emerald `● Live` badge.
 *   - 'error'     — checks `error.code === 'AuthorNameConflict'`. If yes,
 *     parses the JSON-stringified `error.detail` blob into
 *     `AuthorNameConflictDetail` and renders `<PublishRecoveryCard>`.
 *     Otherwise, renders the rose generic-error card with the structured
 *     `code · detail` monospace block (INV-7.4.4).
 *
 * Wire oracle for `error.detail` shape: the worker contract for
 * `AuthorNameConflict` (`apps/worker/src/lib/errors.ts::authorNameConflictResponse`)
 * JSON-stringifies `{existing_artifact_id, existing_version, last_published_at}`
 * into the envelope's top-level `detail` field.
 *
 * Footer chrome (INV-7.4.5) lives in the parent `<UploadDialog>` so this
 * component owns only the body region.
 */

import { CheckCircle, X } from 'lucide-react';
import { PublishRecoveryCard, type AuthorNameConflictDetail } from './publish-recovery-card';

export type PublishUploadState = 'uploading' | 'success' | 'error';

export interface PublishProgress {
  phase: string;
  done: number;
  total: number;
}

export interface PublishResult {
  artifact_id: string;
  name: string;
  kind: string;
  tags: string[];
}

export interface PublishError {
  code: string;
  message: string;
  detail: string;
}

export interface PublishProps {
  /**
   * The artifact name the user typed in Step 2 — surfaced into the amber
   * recovery card (and is the one piece of identifying copy the worker's
   * `AuthorNameConflict` envelope can NOT supply, since the envelope's
   * `error.message` is generic). The success card pulls its name from
   * `state.result.name` (post-publish, server-confirmed); only the recovery
   * card needs the pre-publish form value, which is why it lives at the
   * top level rather than under `state.error`.
   */
  artifactName: string;
  state: {
    uploadState: PublishUploadState;
    progress: PublishProgress | null;
    result: PublishResult | null;
    error: PublishError | null;
  };
  actions: {
    linkAndUpdate: (artifactId: string) => void;
    renameAndPublishNew: () => void;
  };
}

/** Human-friendly phase label. Defensive against unrecognised phases. */
function formatPhase(phase: string): string {
  switch (phase.toLowerCase()) {
    case 'pack':
      return 'Packing';
    case 'sanitize':
      return 'Sanitizing';
    case 'upload':
      return 'Uploading';
    default:
      return phase;
  }
}

function computePercent(progress: PublishProgress | null): number {
  if (!progress || progress.total <= 0) return 0;
  const pct = Math.floor((progress.done / progress.total) * 100);
  return Math.max(0, Math.min(100, pct));
}

/**
 * Best-effort parse of the AuthorNameConflict detail blob. Returns `null` if
 * the envelope's `detail` is missing or doesn't deserialize into the expected
 * shape — the caller falls back to the generic error card in that case.
 */
function parseAuthorNameConflictDetail(detail: string): AuthorNameConflictDetail | null {
  try {
    const parsed = JSON.parse(detail) as unknown;
    if (
      parsed &&
      typeof parsed === 'object' &&
      typeof (parsed as Record<string, unknown>).existing_artifact_id === 'string' &&
      typeof (parsed as Record<string, unknown>).existing_version === 'string' &&
      typeof (parsed as Record<string, unknown>).last_published_at === 'string'
    ) {
      return parsed as AuthorNameConflictDetail;
    }
  } catch {
    /* fall through */
  }
  return null;
}

export function Publish({ artifactName, state, actions }: PublishProps) {
  if (state.uploadState === 'uploading') {
    return <PublishUploading progress={state.progress} />;
  }
  if (state.uploadState === 'success' && state.result) {
    return <PublishSuccess result={state.result} />;
  }
  if (state.uploadState === 'error' && state.error) {
    if (state.error.code === 'AuthorNameConflict') {
      const detail = parseAuthorNameConflictDetail(state.error.detail);
      if (detail) {
        return (
          <PublishRecoveryCard
            artifactName={artifactName}
            detail={detail}
            onLinkAndUpdate={() => actions.linkAndUpdate(detail.existing_artifact_id)}
            onRenameAndPublishNew={actions.renameAndPublishNew}
          />
        );
      }
    }
    return <PublishErrorView error={state.error} />;
  }
  // Defensive fallback — shouldn't render in practice because the parent
  // mounts <Publish> only when in one of the three real states.
  return null;
}

function PublishUploading({ progress }: { progress: PublishProgress | null }) {
  const percent = computePercent(progress);
  const phaseLabel = progress ? formatPhase(progress.phase) : 'Starting';

  return (
    <div
      data-testid="publish-uploading"
      className="flex flex-1 flex-col items-center justify-center gap-4"
    >
      <div
        data-testid="publish-uploading-spinner"
        className="publish-spinner"
        style={{
          width: 56,
          height: 56,
          borderRadius: '50%',
          border: '3px solid rgba(0,217,255,0.25)',
          borderTopColor: '#00D9FF',
          animation: 'publish-spin 0.9s linear infinite',
        }}
      />
      <div className="text-center">
        <div className="mb-1 text-sm font-semibold">Uploading to Omni Hub</div>
        <div data-testid="publish-uploading-phase" className="text-xs text-[#a1a1aa]">
          {phaseLabel} · {percent}%
        </div>
      </div>
      <div
        data-testid="publish-uploading-bar"
        className="overflow-hidden rounded"
        style={{ width: 220, height: 4, background: '#27272A' }}
      >
        <div
          data-testid="publish-uploading-bar-fill"
          style={{
            width: `${percent}%`,
            height: '100%',
            background: '#00D9FF',
            transition: 'width 120ms linear',
          }}
        />
      </div>
      {/* Inline keyframes — co-located with the only consumer so the
          animation never silently breaks if a global stylesheet is reorganised. */}
      <style>{`@keyframes publish-spin { to { transform: rotate(360deg); } }`}</style>
    </div>
  );
}

function PublishSuccess({ result }: { result: PublishResult }) {
  const tagCount = result.tags.length;
  return (
    <div
      data-testid="publish-success"
      className="flex flex-1 flex-col items-center justify-center gap-3.5"
    >
      <div
        className="flex items-center justify-center rounded-full text-[#10b981]"
        style={{
          width: 68,
          height: 68,
          background: 'rgba(16,185,129,0.12)',
          border: '2px solid rgba(16,185,129,0.6)',
        }}
      >
        <CheckCircle width={32} height={32} strokeWidth={2} />
      </div>
      <div className="text-center">
        <div className="mb-1 text-base font-semibold">Successfully Published!</div>
        <div className="mx-auto text-xs leading-relaxed text-[#a1a1aa]" style={{ maxWidth: 280 }}>
          Your overlay "{result.name}" is now available for others to discover and install.
        </div>
      </div>

      <div
        data-testid="publish-success-card"
        className="mt-1 flex w-full items-center gap-2.5 rounded-lg border border-[#27272A] bg-[#0A0A0B] p-2.5"
      >
        <div
          className="shrink-0 rounded"
          style={{
            width: 44,
            height: 28,
            background: 'linear-gradient(135deg,#27272A,#3f3f46)',
          }}
        />
        <div className="flex-1">
          <div className="text-xs font-semibold">{result.name}</div>
          <div className="text-[10px] text-[#a1a1aa]">
            {result.kind} · {tagCount} {tagCount === 1 ? 'tag' : 'tags'}
          </div>
        </div>
        <span
          data-testid="publish-success-live-badge"
          className="rounded text-[10px] font-semibold text-[#10b981]"
          style={{
            padding: '3px 8px',
            background: 'rgba(16,185,129,0.15)',
            border: '1px solid rgba(16,185,129,0.6)',
          }}
        >
          ● Live
        </span>
      </div>
    </div>
  );
}

function PublishErrorView({ error }: { error: PublishError }) {
  return (
    <div
      data-testid="publish-error"
      className="flex flex-1 flex-col items-center justify-center gap-3.5"
    >
      <div
        className="flex items-center justify-center rounded-full text-[#f43f5e]"
        style={{
          width: 68,
          height: 68,
          background: 'rgba(244,63,94,0.12)',
          border: '2px solid rgba(244,63,94,0.6)',
        }}
      >
        <X width={28} height={28} strokeWidth={2} />
      </div>
      <div className="text-center">
        <div className="mb-1 text-base font-semibold text-[#f43f5e]">Upload Failed</div>
        <div className="mx-auto text-xs leading-relaxed text-[#fecdd3]" style={{ maxWidth: 280 }}>
          {error.message}
        </div>
      </div>
      <div
        data-testid="publish-error-detail"
        className="w-full break-all rounded-md border border-[#27272A] bg-[#0A0A0B] p-2.5 font-mono text-[11px] text-[#a1a1aa]"
      >
        <span className="text-[#71717a]">code</span> {error.code}
        {error.detail ? (
          <>
            {' · '}
            <span className="text-[#71717a]">detail</span> {error.detail}
          </>
        ) : null}
      </div>
    </div>
  );
}
