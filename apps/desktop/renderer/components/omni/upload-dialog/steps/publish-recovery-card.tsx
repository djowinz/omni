/**
 * Step 4 amber recovery card — INV-7.6.3 / spec §7.6.
 *
 * Rendered by `<Publish>` when the worker returns
 * `error.code === 'AuthorNameConflict'` (see
 * `apps/worker/src/lib/errors.ts::authorNameConflictResponse` — wire oracle).
 * The error envelope's top-level `detail` field is a JSON-stringified
 * `AuthorNameConflictDetail` blob; the parent container parses it once and
 * passes the resolved object via `props.detail`.
 *
 * Two recovery actions:
 *   - "Link and update" (cyan primary): writes the missing sidecar from
 *     `existing_artifact_id` and re-fires as `upload.update` with bump=patch.
 *     The `+1` patch bump is rendered into the button label so the user sees
 *     the resulting version before clicking.
 *   - "Rename and publish new" (ghost outline): navigates Step 4 → Step 2 with
 *     the Name field focused.
 *
 * Mockup reference: `sidecar-deleted-recovery.html` Flow 2.
 */

import { AlertTriangle } from 'lucide-react';

export interface AuthorNameConflictDetail {
  existing_artifact_id: string;
  existing_version: string;
  last_published_at: string;
}

export interface PublishRecoveryCardProps {
  artifactName: string;
  detail: AuthorNameConflictDetail;
  onLinkAndUpdate: () => void;
  onRenameAndPublishNew: () => void;
}

/**
 * Patch-bump a `MAJOR.MINOR.PATCH` semver string. Falls back to appending
 * "+1" if the input doesn't look like semver — the worker contract guarantees
 * `existing_version` is a semver, but defensive parsing avoids a crash if a
 * future server build relaxes that.
 */
function bumpPatch(version: string): string {
  const parts = version.split('.');
  if (parts.length === 3) {
    const patch = Number.parseInt(parts[2] ?? '', 10);
    if (Number.isFinite(patch)) {
      return `${parts[0]}.${parts[1]}.${patch + 1}`;
    }
  }
  return `${version}+1`;
}

/** Trim ISO-8601 to the YYYY-MM-DD prefix for display. */
function formatPublishedDate(iso: string): string {
  return iso.slice(0, 10);
}

export function PublishRecoveryCard({
  artifactName,
  detail,
  onLinkAndUpdate,
  onRenameAndPublishNew,
}: PublishRecoveryCardProps) {
  const nextVersion = bumpPatch(detail.existing_version);
  const idShort = `${detail.existing_artifact_id.slice(0, 12)}…`;
  const date = formatPublishedDate(detail.last_published_at);

  return (
    <div
      data-testid="publish-recovery-card"
      className="flex flex-1 flex-col items-center justify-center gap-3.5 py-2.5"
    >
      <div
        className="flex items-center justify-center rounded-full text-[#f59e0b]"
        style={{
          width: 68,
          height: 68,
          background: 'rgba(245,158,11,0.12)',
          border: '2px solid rgba(245,158,11,0.6)',
        }}
      >
        <AlertTriangle width={26} height={26} strokeWidth={2} />
      </div>

      <div className="text-center">
        <div className="mb-1 text-[15px] font-semibold text-[#f59e0b]">Name already taken</div>
        <div className="mx-auto text-xs leading-relaxed text-[#d4d4d8]" style={{ maxWidth: 300 }}>
          You already have an artifact named <strong>"{artifactName}"</strong> under this identity.
        </div>
      </div>

      <div className="w-full rounded-lg border border-[#27272A] bg-[#0A0A0B] p-3">
        <div className="mb-3 flex items-center gap-2.5">
          <div
            className="shrink-0 rounded"
            style={{
              width: 44,
              height: 28,
              background: 'linear-gradient(135deg,#27272A,#3f3f46)',
            }}
          />
          <div className="flex-1">
            <div className="text-xs font-semibold">{artifactName}</div>
            <div className="font-mono text-[10px] text-[#a1a1aa]">
              {idShort} · v{detail.existing_version} · published {date}
            </div>
          </div>
        </div>

        <div className="flex flex-col gap-1.5">
          <button
            type="button"
            data-testid="publish-recovery-link-and-update"
            onClick={onLinkAndUpdate}
            className="flex w-full items-center justify-between rounded-md border-none bg-[#00D9FF] px-3 py-2 text-left text-xs font-semibold text-[#09090B]"
          >
            <span>Link and update → v{nextVersion}</span>
            <span>›</span>
          </button>
          <button
            type="button"
            data-testid="publish-recovery-rename-and-publish-new"
            onClick={onRenameAndPublishNew}
            className="w-full rounded-md border border-[#27272A] bg-transparent px-3 py-2 text-left text-xs font-medium text-[#d4d4d8]"
          >
            Rename and publish new
          </button>
        </div>
      </div>
    </div>
  );
}
