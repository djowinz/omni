/**
 * IdentitySummaryCard — compact identity view with backup status.
 *
 * Shared between #015's My Uploads header and #016's Settings Identity
 * section. Keeps identity presentation consistent across surfaces.
 */

import { ShieldCheck, ShieldAlert } from 'lucide-react';

export interface IdentitySummaryCardProps {
  pubkeyHex: string;
  fingerprintHex: string;
  backedUp: boolean;
  className?: string;
}

export function IdentitySummaryCard({
  pubkeyHex,
  fingerprintHex,
  backedUp,
  className,
}: IdentitySummaryCardProps) {
  const shortHex = fingerprintHex.length > 0 ? fingerprintHex : pubkeyHex.slice(0, 12);
  return (
    <div
      data-testid="identity-summary-card"
      className={
        'flex items-center justify-between rounded-md border border-[#27272A] bg-[#18181B] px-4 py-3 ' +
        (className ?? '')
      }
    >
      <div className="flex items-center gap-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-full bg-[#00D9FF]/10 text-[#00D9FF]">
          <span className="font-mono text-xs">{shortHex.slice(0, 2)}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-sm font-medium text-[#FAFAFA]">Your identity</span>
          <code className="text-xs text-zinc-500">{shortHex}</code>
        </div>
      </div>
      <div
        data-testid="identity-backup-status"
        className={
          'flex items-center gap-1.5 text-xs ' + (backedUp ? 'text-emerald-400' : 'text-amber-400')
        }
      >
        {backedUp ? <ShieldCheck className="h-4 w-4" /> : <ShieldAlert className="h-4 w-4" />}
        {backedUp ? 'Backed up' : 'Not backed up'}
      </div>
    </div>
  );
}
