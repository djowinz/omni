import { useIdentity } from '../../lib/identity-context';

export interface IdentityChipProps {
  onNavigateToSettings: () => void;
}

export function IdentityChip({ onNavigateToSettings }: IdentityChipProps) {
  const { identity } = useIdentity();
  if (!identity) return null;

  const slice = identity.pubkey_hex.slice(0, 8);

  let dotColor = 'bg-[#52525b]';
  let dotLabel = 'No backup history yet';
  let dotShadow = '';
  if (identity.backed_up) {
    dotColor = 'bg-[#22c55e]';
    dotLabel = 'Backed up';
    dotShadow = 'shadow-[0_0_4px_rgba(34,197,94,0.5)]';
  } else if (identity.last_backed_up_at !== null || identity.last_rotated_at !== null) {
    dotColor = 'bg-[#fbbf24]';
    dotLabel = 'Not backed up';
    dotShadow = 'shadow-[0_0_4px_rgba(251,191,36,0.5)]';
  }

  return (
    <button
      type="button"
      onClick={onNavigateToSettings}
      aria-label="Your identity"
      title={dotLabel}
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
      <span aria-label={dotLabel} className={`h-1.5 w-1.5 rounded-full ${dotColor} ${dotShadow}`} />
    </button>
  );
}
