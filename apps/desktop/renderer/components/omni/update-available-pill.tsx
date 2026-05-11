/**
 * UpdateAvailablePill — visual indicator that a newer manifest.version is
 * available for an installed artifact.
 *
 * - variant="corner": absolutely-positioned, non-interactive (the parent
 *   card is the click target — clicks bubble through to open the detail).
 * - variant="header": inline, interactive — fires onClick to open the
 *   confirm dialog.
 *
 * Color palette per design Option A (approved 2026-05-11):
 *   bg #34D399 / fg #022C22 / dot #022C22
 */
import { cn } from '@/lib/utils';
import type { UpdateStatus } from '@/hooks/use-artifact-update-status';

export interface UpdateAvailablePillProps {
  status: UpdateStatus;
  variant: 'corner' | 'header';
  onClick?: () => void;
}

export function UpdateAvailablePill({ status, variant, onClick }: UpdateAvailablePillProps) {
  if (variant === 'corner') {
    return (
      <div
        className={cn(
          'absolute top-2 right-2 z-10',
          'flex items-center gap-1.5 rounded-full px-2 py-0.5',
          'bg-[#34D399] text-[#022C22]',
          'text-[10px] font-bold leading-none',
          'shadow-[0_1px_3px_rgba(0,0,0,0.3)]',
          'pointer-events-none', // clicks bubble to card
        )}
        data-testid="update-pill-corner"
      >
        <span className="h-1 w-1 rounded-full bg-[#022C22]" />
        v{status.latest_version}
      </div>
    );
  }
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex items-center gap-1.5 rounded-full px-2.5 py-1',
        'bg-[#34D399] text-[#022C22]',
        'text-[11px] font-bold leading-none',
        'hover:brightness-110 active:brightness-95',
      )}
      data-testid="update-pill-header"
    >
      <span className="h-1.5 w-1.5 rounded-full bg-[#022C22]" />
      Update v{status.latest_version}
    </button>
  );
}
