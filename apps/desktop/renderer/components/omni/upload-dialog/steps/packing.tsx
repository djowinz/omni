/**
 * Packing — Step 3 content region (INV-7.3.1).
 *
 * Layout:
 *   1 banner row (Security Verification, always neutral chrome — INV-7.3.2)
 *   5 stage rows in fixed order (INV-7.3.3) — pending/running/passed/failed
 *   1 summary card (INV-7.3.6):
 *     - all-passed → emerald CheckCircle "Verification Complete"
 *     - first failure (non-Dependency) → rose AlertCircle, stage-specific
 *       title + body + Retry Verification button
 *     - Dependency Check failure with violations → rose AlertCircle
 *       "N dependency violations" + grouped <PackingViolationsCard /> + Retry
 *
 * Stages drive off the {@link usePackProgress} hook (subscribes to
 * `upload.packProgress` directly). Retry is wired through
 * {@link PackingProps.actions.retry} so the parent state machine owns the
 * pipeline restart.
 *
 * Violations arrive via props because they're carried on the terminal
 * `upload.publish`/`upload.update` error envelope — not on `packProgress`.
 * The Wave A1.6 host emits per-stage status frames; the structured-error
 * payload at pipeline failure is what surfaces the violation list.
 */

import { AlertCircle, CheckCircle, Shield } from 'lucide-react';
import { PACK_STAGES, usePackProgress } from '../hooks/use-pack-progress';
import type { PackStageStatus } from '../hooks/use-pack-progress';
import { PackingStageRow } from './packing-stage-row';
import { PackingViolationsCard } from './packing-violations-card';
import type { PackingViolation } from './packing-violations-card';
import type { PackStage } from '@omni/shared-types';

export interface PackingActions {
  /** Re-runs the full Packing pipeline. */
  retry: () => void;
}

export interface PackingProps {
  actions: PackingActions;
  /**
   * Aggregate Dependency Check violations. Empty unless the host's terminal
   * error envelope surfaces them. The card only renders when the Dependency
   * stage has failed AND violations is non-empty.
   */
  violations?: PackingViolation[];
}

/** Stage definitions in the fixed INV-7.3.3 order. */
const STAGE_DEFS: Array<{ stage: PackStage; title: string; subtitle: string }> = [
  {
    stage: 'schema',
    title: 'Schema Validation',
    subtitle: 'Validating overlay structure and format',
  },
  {
    stage: 'content-safety',
    title: 'Content-safety Checks',
    subtitle: 'Scanning inline styles and URLs for disallowed content',
  },
  {
    stage: 'asset',
    title: 'Asset Verification',
    subtitle: 'Re-decoding images and fonts',
  },
  {
    stage: 'dependency',
    title: 'Dependency Check',
    subtitle: 'Resolving referenced themes, fonts, and images',
  },
  {
    stage: 'size',
    title: 'Size Check',
    subtitle: 'Verifying bundle fits within upload limits',
  },
];

/** Stage-specific failure body copy (INV-7.3.6 first-failure case). */
const STAGE_FAILURE_BODY: Record<PackStage, { title: string; body: string }> = {
  schema: {
    title: 'Schema Validation Failed',
    body: 'Your overlay structure is malformed. Fix the reported issues and retry.',
  },
  'content-safety': {
    title: 'Content-safety Failed',
    body: 'Inline styles or URLs contain disallowed content. Review and retry.',
  },
  asset: {
    title: 'Asset Verification Failed',
    body: 'One or more images or fonts failed re-decoding. Review your assets and retry.',
  },
  dependency: {
    title: 'Dependency Check Failed',
    body: 'Resolve the dependency violations below, then retry.',
  },
  size: {
    title: 'Size Check Failed',
    body: 'Bundle exceeds upload limits. Reduce overlay size and retry.',
  },
};

function findFirstFailure(
  stages: Record<PackStage, PackStageStatus>,
): PackStage | null {
  for (const s of PACK_STAGES) {
    if (stages[s] === 'failed') return s;
  }
  return null;
}

function allPassed(stages: Record<PackStage, PackStageStatus>): boolean {
  return PACK_STAGES.every((s) => stages[s] === 'passed');
}

export function Packing({ actions, violations = [] }: PackingProps) {
  const { stages } = usePackProgress();
  const firstFailure = findFirstFailure(stages);
  const passed = allPassed(stages);

  return (
    <div data-testid="packing-step" className="flex flex-col gap-2">
      {/* INV-7.3.2 banner row — always neutral chrome. */}
      <div
        data-testid="packing-banner"
        className="flex items-center gap-2.5 rounded-md border border-[#27272A] px-3 py-2.5"
      >
        <div className="flex-shrink-0 text-[#a1a1aa]">
          <Shield className="h-[18px] w-[18px]" strokeWidth={1.75} />
        </div>
        <div>
          <div className="text-xs font-semibold text-[#FAFAFA]">Security Verification</div>
          <div className="text-[11px] text-[#a1a1aa]">
            We&apos;re checking your overlay to ensure it&apos;s safe for others to use
          </div>
        </div>
      </div>

      {/* INV-7.3.3 five stage rows in fixed order. Failure subtitle override:
       * when Dependency Check failed with N violations, replace its subtitle
       * with "N violations — see details below" per INV-7.3.5. */}
      {STAGE_DEFS.map(({ stage, title, subtitle }) => {
        const status = stages[stage];
        let effectiveSubtitle = subtitle;
        if (
          stage === 'dependency' &&
          status === 'failed' &&
          violations.length > 0
        ) {
          effectiveSubtitle = `${violations.length} violation${violations.length === 1 ? '' : 's'} — see details below`;
        }
        return (
          <PackingStageRow
            key={stage}
            title={title}
            subtitle={effectiveSubtitle}
            status={status}
          />
        );
      })}

      {/* Summary card — three branches per INV-7.3.6. */}
      {passed ? (
        <div
          data-testid="packing-summary-passed"
          className="mt-2 flex items-center gap-2 rounded-md border border-[rgba(16,185,129,0.6)] bg-[rgba(16,185,129,0.06)] p-3"
        >
          <div className="flex-shrink-0 text-[#10b981]">
            <CheckCircle className="h-4 w-4" strokeWidth={1.75} />
          </div>
          <div>
            <div className="text-xs font-semibold text-[#10b981]">Verification Complete</div>
            <div className="text-[11px] text-[#10b981] opacity-75">
              Your overlay has passed all security checks
            </div>
          </div>
        </div>
      ) : firstFailure === 'dependency' && violations.length > 0 ? (
        <PackingViolationsCard violations={violations} onRetry={actions.retry} />
      ) : firstFailure !== null ? (
        <div
          data-testid="packing-summary-failed"
          data-failed-stage={firstFailure}
          className="mt-2 rounded-md border border-[rgba(244,63,94,0.6)] bg-[rgba(244,63,94,0.06)] p-3"
        >
          <div className="flex items-start gap-2">
            <div className="mt-0.5 flex-shrink-0 text-[#f43f5e]">
              <AlertCircle className="h-4 w-4" strokeWidth={1.75} />
            </div>
            <div>
              <div className="mb-0.5 text-xs font-semibold text-[#f43f5e]">
                {STAGE_FAILURE_BODY[firstFailure].title}
              </div>
              <div className="text-[11px] text-[#fecdd3]">
                {STAGE_FAILURE_BODY[firstFailure].body}
              </div>
              <button
                type="button"
                data-testid="packing-summary-retry"
                onClick={actions.retry}
                className="mt-2 cursor-pointer rounded border border-[#be123c] bg-transparent px-2.5 py-1 text-[11px] font-medium text-[#fecdd3]"
              >
                Retry Verification
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
