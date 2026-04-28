import { Shield, ShieldAlert, ShieldCheck, MoreVertical, Download, Upload } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useIdentity } from '../../lib/identity-context';
import { DisplayNameField } from './display-name-field';

export interface IdentitySectionProps {
  onBackup: () => void;
  onImport: () => void;
  onRotate: () => void;
  onCopyPubkey: () => void;
}

export function IdentitySection({
  onBackup,
  onImport,
  onRotate,
  onCopyPubkey,
}: IdentitySectionProps) {
  const { identity } = useIdentity();
  if (!identity) return null;

  const slice = identity.pubkey_hex.slice(0, 8);

  return (
    <section id="identity-section">
      <h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-[#52525b]">
        Identity
      </h3>
      <div className="rounded-lg border border-[#27272a] bg-[#18181b]/50 p-3.5">
        <div className="mb-2.5 flex items-center justify-between">
          <span className="flex items-center gap-2 text-[12px] font-medium text-[#fafafa]">
            <Shield className="h-3 w-3 text-[#00d9ff]" />
            Your Identity
          </span>
          <DropdownMenu>
            <DropdownMenuTrigger
              aria-label="More options"
              className="rounded p-0.5 text-[#71717a] hover:text-[#a1a1aa]"
            >
              <MoreVertical className="h-3.5 w-3.5" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={onCopyPubkey}>Copy public key</DropdownMenuItem>
              <DropdownMenuItem onSelect={onRotate}>Rotate keys…</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <DisplayNameField />

        <div
          className="py-1.5 font-mono text-[13px]"
          title={identity.display_name ? `${identity.display_name}#${slice}` : `#${slice}`}
        >
          <span className="text-[#71717a]">
            #<span className="text-[#fafafa]">{slice}</span>
          </span>
        </div>

        <div className="flex items-center gap-1.5 pt-1.5 text-[11px]">
          {identity.backed_up ? (
            <>
              <ShieldCheck className="h-3 w-3 text-[#22c55e]" />
              <span className="text-[#22c55e]">Backed up</span>
              {identity.last_backed_up_at !== null && (
                <span className="text-[#71717a]">
                  &middot;{' '}
                  {formatDistanceToNow(new Date(identity.last_backed_up_at * 1000), {
                    addSuffix: true,
                  })}
                </span>
              )}
            </>
          ) : (
            <>
              <ShieldAlert className="h-3 w-3 text-[#fbbf24]" />
              <span className="text-[#fbbf24]">Not backed up</span>
            </>
          )}
        </div>

        <div className="flex gap-2 pt-2.5">
          <button
            type="button"
            onClick={onBackup}
            className="flex h-7 flex-1 items-center justify-center gap-1.5 rounded border border-[#27272a] bg-[#27272a]/50 text-[11px] text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#fafafa]"
          >
            <Download className="h-3 w-3" />
            Back up
          </button>
          <button
            type="button"
            onClick={onImport}
            className="flex h-7 flex-1 items-center justify-center gap-1.5 rounded border border-[#27272a] bg-[#27272a]/50 text-[11px] text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#fafafa]"
          >
            <Upload className="h-3 w-3" />
            Import
          </button>
        </div>
      </div>
    </section>
  );
}
