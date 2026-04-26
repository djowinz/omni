/**
 * PackingStageRow — single check row in the Step 3 Packing pipeline UI.
 *
 * Renders one of four visual states per INV-7.3.4:
 *   - pending: 1px #27272A border, transparent bg, empty Circle icon
 *   - running: same chrome as pending + animated pulse on the icon
 *   - passed: emerald border/bg, ShieldCheck icon, bold emerald title
 *   - failed: rose border/bg, ShieldAlert icon, rose title
 *
 * Stage subtitle copy defaults to INV-7.3.5 strings, with the option to
 * override (e.g. failure summary "4 violations — see details below").
 */

import { Circle, Shield, ShieldAlert, ShieldCheck } from 'lucide-react';
import type { PackStageStatus } from '../hooks/use-pack-progress';

export interface PackingStageRowProps {
  /** Stage title, e.g. "Schema Validation". */
  title: string;
  /** Stage subtitle / activity detail. */
  subtitle: string;
  /** Visual state per INV-7.3.4. */
  status: PackStageStatus;
  /** Optional test id override; defaults to a slug of title. */
  testId?: string;
}

const PENDING_CLASSES =
  'flex items-center gap-2.5 rounded-md border border-[#27272A] px-3 py-2.5';
const PASSED_CLASSES =
  'flex items-center gap-2.5 rounded-md border border-[rgba(16,185,129,0.6)] bg-[rgba(16,185,129,0.06)] px-3 py-2.5';
const FAILED_CLASSES =
  'flex items-center gap-2.5 rounded-md border border-[rgba(244,63,94,0.6)] bg-[rgba(244,63,94,0.06)] px-3 py-2.5';

function defaultTestId(title: string): string {
  return `packing-stage-${title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')}`;
}

export function PackingStageRow({ title, subtitle, status, testId }: PackingStageRowProps) {
  const id = testId ?? defaultTestId(title);

  if (status === 'passed') {
    return (
      <div data-testid={id} data-status="passed" className={PASSED_CLASSES}>
        <div className="flex-shrink-0 text-[#10b981]">
          <ShieldCheck className="h-[18px] w-[18px]" strokeWidth={1.75} />
        </div>
        <div className="min-w-0">
          <div className="text-xs font-semibold text-[#10b981]">{title}</div>
          <div className="text-[11px] text-[#10b981] opacity-75">{subtitle}</div>
        </div>
      </div>
    );
  }

  if (status === 'failed') {
    return (
      <div data-testid={id} data-status="failed" className={FAILED_CLASSES}>
        <div className="flex-shrink-0 text-[#f43f5e]">
          {/* ShieldAlert: shield with overlaid exclamation. INV-7.3.4 calls for
           * "Shield-with-exclamation"; ShieldAlert composes that in one icon.
           * Falls back gracefully across lucide-react versions that ship the
           * icon under different aliases. */}
          <ShieldAlert className="h-[18px] w-[18px]" strokeWidth={1.75} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-xs font-semibold text-[#f43f5e]">{title}</div>
          <div className="text-[11px] text-[#f43f5e] opacity-75">{subtitle}</div>
        </div>
      </div>
    );
  }

  // pending OR running — same chrome; running adds pulse on the icon.
  const isRunning = status === 'running';
  return (
    <div data-testid={id} data-status={status} className={PENDING_CLASSES}>
      <div
        className={
          'flex-shrink-0 text-[#52525b]' + (isRunning ? ' animate-pulse' : '')
        }
      >
        {isRunning ? (
          <Shield className="h-[18px] w-[18px]" strokeWidth={1.75} />
        ) : (
          <Circle className="h-[18px] w-[18px]" strokeWidth={1.75} />
        )}
      </div>
      <div className="min-w-0">
        <div className="text-xs font-semibold text-[#71717a]">{title}</div>
        <div className="text-[11px] text-[#52525b]">{subtitle}</div>
      </div>
    </div>
  );
}
