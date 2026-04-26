/**
 * PackingViolationsCard — aggregate Dependency Check failure card (INV-7.8.5).
 *
 * Groups violations by category ("Missing files", "Unused files",
 * "Content-safety"). Each group renders a header + a list of compact
 * monospace path rows with a one-line reason. Footer shows
 * "Fix all of the following, then retry." copy + a Retry button that
 * re-runs the full Packing pipeline (INV-7.8.6).
 *
 * Wave B1.5 / OWI-55 added the "Content-safety (N)" group rendering. The
 * row reason renders the host's `detail` string verbatim when present and
 * already formatted (e.g. `"flagged · conf 0.91"`), AND tolerates raw
 * `confidence=0.XX` shapes from older host builds by formatting them as
 * `flagged · conf 0.XX` so the card stays readable across the wire-shape
 * transition documented in `crates/host/src/share/error.rs`.
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
   *
   * For `content-safety`, the host (Wave B1.4 / OWI-54) emits a
   * pre-formatted `"flagged · conf 0.XX"` string. As a defense-in-depth
   * fallback, raw `confidence=0.XX` (or bare numeric `0.XX`) shapes are
   * normalized at render time — see {@link formatContentSafetyDetail}.
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

/** Matches `confidence=0.91`, `confidence: 0.91`, or a bare `0.91`. */
const CONFIDENCE_PATTERN = /(?:^|\bconfidence\s*[:=]\s*)(0?\.\d{1,4}|1(?:\.0+)?)/i;

/**
 * Normalize a `content-safety` row reason into the human-friendly
 * `"flagged · conf 0.XX"` shape that the mockup specifies.
 *
 * - Already-formatted strings (anything containing `flagged`) pass through
 *   verbatim — Wave B1.4 / OWI-54's host emits exactly that shape.
 * - Raw `confidence=0.XX` / bare `0.XX` shapes get reformatted.
 * - Anything else (or absent detail) falls back to plain `"flagged"`.
 */
function formatContentSafetyDetail(detail: string | undefined): string {
  if (!detail) return 'flagged';
  if (/flagged/i.test(detail)) return detail;
  const match = detail.match(CONFIDENCE_PATTERN);
  if (match === null) return 'flagged';
  const raw = match[1];
  // Normalize `.91` → `0.91` and clamp to two-decimal display.
  const num = Number(raw.startsWith('.') ? `0${raw}` : raw);
  if (!Number.isFinite(num)) return 'flagged';
  return `flagged · conf ${num.toFixed(2)}`;
}

function rowReason(violation: PackingViolation): string {
  if (violation.kind === 'content-safety') {
    return formatContentSafetyDetail(violation.detail);
  }
  return violation.detail ?? KIND_DEFAULT_REASON[violation.kind];
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
                    {rowReason(row)}
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
