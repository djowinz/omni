import { cn } from '@/lib/utils';

export type InstallPhase = 'download' | 'verify' | 'sanitize' | 'write' | 'done' | 'error';

export interface InstallProgressProps {
  phase: InstallPhase;
  done: number;
  total: number;
  label?: string;
}

const IN_FLIGHT_PHASES = ['download', 'verify', 'sanitize', 'write'] as const;
type InFlightPhase = (typeof IN_FLIGHT_PHASES)[number];

function inFlightIndex(phase: InstallPhase): number {
  return IN_FLIGHT_PHASES.indexOf(phase as InFlightPhase);
}

export function InstallProgress({ phase, done, total, label }: InstallProgressProps) {
  const safeDone = total === 0 ? 0 : done;
  const safeTotal = total === 0 ? 1 : total;

  const barWidthPct = phase === 'done' ? 100 : (safeDone / safeTotal) * 100;

  const barColor =
    phase === 'done' ? 'bg-emerald-500' : phase === 'error' ? 'bg-red-500' : 'bg-cyan-400';

  // Index of the active in-flight phase (-1 for 'done'/'error' with no matching slot)
  const currentIdx = inFlightIndex(phase);

  return (
    <div data-testid="install-progress" className="flex flex-col gap-2 w-full">
      {label !== undefined && <span className="text-xs text-muted-foreground">{label}</span>}

      {/* Progress bar track */}
      <div className="w-full h-2 bg-muted rounded-full overflow-hidden">
        <div
          data-testid="install-progress-bar"
          className={cn('h-full rounded-full transition-all duration-300', barColor)}
          style={{ width: `${barWidthPct}%` }}
        />
      </div>

      {/* Phase pills */}
      <div className="flex gap-1.5">
        {IN_FLIGHT_PHASES.map((p, idx) => {
          // A pill is "done" when phase === 'done' OR when the active phase is further right
          const pillDone = phase === 'done' || (currentIdx > idx && phase !== 'error');
          // A pill is the active/current pill
          const pillCurrent = currentIdx === idx && phase !== 'done';
          // A pill is in the error state when phase === 'error' and it's the active slot
          const pillError = phase === 'error' && currentIdx === idx;

          const pillContent = pillDone ? `✓ ${p}` : p;

          // The highlighted pill carries the testid for the current phase
          const isHighlighted = pillCurrent;

          return (
            <span
              key={p}
              data-testid={isHighlighted ? `install-progress-phase-${phase}` : undefined}
              className={cn(
                'text-xs px-2 py-0.5 rounded-full border transition-all duration-300 select-none',
                phase === 'done'
                  ? 'border-emerald-500/40 text-emerald-400 bg-emerald-500/10'
                  : pillError
                    ? 'border-red-500/40 text-red-400 bg-red-500/10 font-semibold ring-1 ring-red-500/50'
                    : pillCurrent
                      ? 'border-cyan-400/40 text-cyan-300 bg-cyan-400/10 font-semibold ring-1 ring-cyan-400/50'
                      : pillDone
                        ? 'border-emerald-500/30 text-emerald-500/70 bg-emerald-500/5'
                        : 'border-border text-muted-foreground bg-muted/30',
              )}
            >
              {pillContent}
            </span>
          );
        })}
      </div>
    </div>
  );
}
