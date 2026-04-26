/**
 * PackingViolationsCard — aggregate Dependency Check failure card (INV-7.8.5).
 *
 * Groups violations by category ("Missing files", "Unused files",
 * "Content-safety"; the third lands in Wave B1.5 — the renderer accepts it
 * today so the wire shape doesn't churn later). Each group renders a header
 * + a list of compact monospace path rows with a one-line reason. Footer
 * shows "Fix all of the following, then retry." copy + a Retry button that
 * re-runs the full Packing pipeline (INV-7.8.6).
 */

import { AlertCircle } from 'lucide-react';

export type ViolationKind = 'missing-ref' | 'unused-file' | 'content-safety';

export interface PackingViolation {
  kind: ViolationKind;
  /** Workspace-relative path to the offending file. */
  path: string;
  /**
   * Optional reason / detail rendered in the right-side label slot. Falls
   * back to a kind-derived default if absent.
   */
  detail?: string;
}

export interface PackingViolationsCardProps {
  violations: PackingViolation[];
  /** Fires the retry action (re-runs the full Packing pipeline). */
  onRetry: () => void;
}

const KIND_LABELS: Record<ViolationKind, string> = {
  'missing-ref': 'Missing files',
  'unused-file': 'Unused files',
  'content-safety': 'Content-safety',
};

const KIND_DEFAULT_REASON: Record<ViolationKind, string> = {
  'missing-ref': 'not found',
  'unused-file': 'not referenced',
  'content-safety': 'flagged',
};

const KIND_ORDER: ViolationKind[] = ['missing-ref', 'unused-file', 'content-safety'];

function groupByKind(violations: PackingViolation[]): Record<ViolationKind, PackingViolation[]> {
  const out: Record<ViolationKind, PackingViolation[]> = {
    'missing-ref': [],
    'unused-file': [],
    'content-safety': [],
  };
  for (const v of violations) out[v.kind].push(v);
  return out;
}

export function PackingViolationsCard({ violations, onRetry }: PackingViolationsCardProps) {
  const grouped = groupByKind(violations);
  const total = violations.length;

  return (
    <div
      data-testid="packing-violations-card"
      className="mt-2 rounded-md border border-[rgba(244,63,94,0.6)] bg-[rgba(244,63,94,0.06)] p-3"
    >
      <div className="mb-2.5 flex items-start gap-2">
        <div className="mt-0.5 flex-shrink-0 text-[#f43f5e]">
          <AlertCircle className="h-4 w-4" strokeWidth={1.75} />
        </div>
        <div>
          <div className="mb-0.5 text-xs font-semibold text-[#f43f5e]">
            {total} dependency violation{total === 1 ? '' : 's'}
          </div>
          <div className="text-[11px] text-[#fecdd3]">Fix all of the following, then retry.</div>
        </div>
      </div>

      {KIND_ORDER.map((kind) => {
        const rows = grouped[kind];
        if (rows.length === 0) return null;
        return (
          <div
            key={kind}
            data-testid={`packing-violations-group-${kind}`}
            className="mb-2.5 last:mb-0"
          >
            <div className="mb-1.5 text-[10px] font-bold uppercase tracking-[0.6px] text-[#fecdd3]">
              {KIND_LABELS[kind]} ({rows.length})
            </div>
            <div className="flex flex-col gap-1">
              {rows.map((row) => (
                <div
                  key={`${kind}:${row.path}`}
                  data-testid={`packing-violation-row`}
                  className="flex items-center gap-2 rounded border border-[rgba(244,63,94,0.35)] bg-[rgba(9,9,11,0.5)] px-2.5 py-1.5"
                >
                  <code className="flex-1 truncate font-mono text-[11px] text-[#fecdd3]">
                    {row.path}
                  </code>
                  <span className="flex-shrink-0 text-[10px] text-[#f43f5e]">
                    {row.detail ?? KIND_DEFAULT_REASON[kind]}
                  </span>
                </div>
              ))}
            </div>
          </div>
        );
      })}

      <button
        type="button"
        data-testid="packing-violations-retry"
        onClick={onRetry}
        className="mt-2 cursor-pointer rounded border border-[#be123c] bg-transparent px-3 py-1.5 text-[11px] font-medium text-[#fecdd3]"
      >
        Retry Verification
      </button>
    </div>
  );
}
