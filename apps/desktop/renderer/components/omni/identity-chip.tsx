import { useEffect, useRef, useState } from 'react';
import { formatDistanceToNow } from 'date-fns';
import { useIdentity } from '../../lib/identity-context';

export interface IdentityChipProps {
  onNavigateToSettings: () => void;
}

const HOVER_OPEN_DELAY_MS = 100;

export function IdentityChip({ onNavigateToSettings }: IdentityChipProps) {
  const { identity } = useIdentity();
  const [tooltipOpen, setTooltipOpen] = useState(false);
  const openTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (openTimerRef.current !== null) {
        window.clearTimeout(openTimerRef.current);
      }
    };
  }, []);

  if (!identity) return null;

  const slice = identity.pubkey_hex.slice(0, 8);

  // Dot color decision tree:
  //   green  — backed_up: true
  //   amber  — !backed_up AND user has interacted (set display_name OR has any
  //            backup/rotation history). Setting a display_name counts as
  //            interaction; once the user has invested in this identity they
  //            should see the "needs backup" prompt.
  //   neutral — fresh-install no-interaction case (no name, no backup history,
  //            no rotation history). Welcome dialog typically catches this
  //            window; chip just stays grey until the user does something.
  const hasInteracted =
    identity.display_name !== null ||
    identity.last_backed_up_at !== null ||
    identity.last_rotated_at !== null;

  let dotColor = 'bg-[#52525b]';
  let dotLabel = 'No backup history yet';
  let dotShadow = '';
  if (identity.backed_up) {
    dotColor = 'bg-[#22c55e]';
    dotLabel = 'Backed up';
    dotShadow = 'shadow-[0_0_4px_rgba(34,197,94,0.5)]';
  } else if (hasInteracted) {
    dotColor = 'bg-[#fbbf24]';
    dotLabel = 'Not backed up';
    dotShadow = 'shadow-[0_0_4px_rgba(251,191,36,0.5)]';
  }

  const handleMouseEnter = () => {
    if (openTimerRef.current !== null) window.clearTimeout(openTimerRef.current);
    openTimerRef.current = window.setTimeout(() => setTooltipOpen(true), HOVER_OPEN_DELAY_MS);
  };
  const handleMouseLeave = () => {
    if (openTimerRef.current !== null) {
      window.clearTimeout(openTimerRef.current);
      openTimerRef.current = null;
    }
    setTooltipOpen(false);
  };

  // Backup-state copy mirrors the dot color decision above.
  let backupStatusText: string;
  let backupStatusClass: string;
  if (identity.backed_up) {
    const ago =
      identity.last_backed_up_at !== null
        ? ` · ${formatDistanceToNow(new Date(identity.last_backed_up_at * 1000), {
            addSuffix: true,
          })}`
        : '';
    backupStatusText = `Backed up${ago}`;
    backupStatusClass = 'text-[#22c55e]';
  } else if (
    identity.display_name !== null ||
    identity.last_backed_up_at !== null ||
    identity.last_rotated_at !== null
  ) {
    backupStatusText = 'Not backed up';
    backupStatusClass = 'text-[#fbbf24]';
  } else {
    backupStatusText = 'No backup history yet';
    backupStatusClass = 'text-[#a1a1aa]';
  }

  return (
    <div className="relative" onMouseEnter={handleMouseEnter} onMouseLeave={handleMouseLeave}>
      <button
        type="button"
        onClick={onNavigateToSettings}
        aria-label="Your identity"
        className="flex h-8 items-center gap-2 rounded-full border border-[#27272a] bg-[#141416] px-2.5 hover:border-[#3f3f46] hover:bg-[#1f1f24]"
      >
        <span className="font-mono text-[12px]">
          {identity.display_name ? (
            <>
              <span className="text-[#fafafa]">{identity.display_name}</span>
              <span className="text-[#71717a]">#</span>
              <span className="text-[#71717a]">{slice}</span>
            </>
          ) : (
            <>
              <span className="text-[#71717a]">#</span>
              <span className="text-[#71717a]">{slice}</span>
            </>
          )}
        </span>
        <span
          aria-label={dotLabel}
          className={`h-1.5 w-1.5 rounded-full ${dotColor} ${dotShadow}`}
        />
      </button>
      {tooltipOpen && (
        <div
          role="tooltip"
          className="absolute right-0 top-[calc(100%+8px)] z-50 w-[260px] rounded-lg border border-[#27272a] bg-[#18181b] p-3 shadow-[0_12px_36px_rgba(0,0,0,0.6)]"
        >
          {/* caret */}
          <div className="absolute -top-[6px] right-12 h-2.5 w-2.5 rotate-45 border-l border-t border-[#27272a] bg-[#18181b]" />

          {identity.display_name && (
            <div className="flex justify-between gap-3 pb-2 text-[11px]">
              <span className="text-[9px] uppercase tracking-wider text-[#71717a] pt-0.5">
                Display name
              </span>
              <span className="font-mono text-right text-[#fafafa]">{identity.display_name}</span>
            </div>
          )}
          <div
            className={`flex justify-between gap-3 py-2 text-[11px] ${identity.display_name ? 'border-t border-[#27272a]' : ''}`}
          >
            <span className="text-[9px] uppercase tracking-wider text-[#71717a] pt-0.5">
              Fingerprint
            </span>
            <span className="font-mono text-right text-[#fafafa]">{slice}</span>
          </div>
          <div className="flex justify-between gap-3 py-2 text-[11px] border-t border-[#27272a]">
            <span className="text-[9px] uppercase tracking-wider text-[#71717a] pt-0.5">
              Backup
            </span>
            <span className={`font-mono text-right ${backupStatusClass}`}>{backupStatusText}</span>
          </div>
          <div className="border-t border-[#27272a] pt-2 mt-1 text-center text-[10px] text-[#52525b]">
            Click to manage →
          </div>
        </div>
      )}
    </div>
  );
}
