import { AlertTriangle } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

export interface TofuFingerprint {
  display_name: string | null;
  pubkey_hex: string;
  fingerprint_hex: string;
  fingerprint_words: readonly [string, string, string];
  fingerprint_emoji: readonly [string, string, string, string, string, string];
}

export interface TofuMismatchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  artifactName: string;
  previously: TofuFingerprint;
  incoming: TofuFingerprint;
  onCancel: () => void;
  onTrustNew: () => void;
}

function handle(fp: TofuFingerprint): string {
  const slice = fp.pubkey_hex.slice(0, 8);
  return fp.display_name ? `${fp.display_name}#${slice}` : `#${slice}`;
}

function FingerprintColumn({ fp, isNew }: { fp: TofuFingerprint; isNew: boolean }) {
  return (
    <div
      className={
        'rounded-md border p-3.5 ' +
        (isNew ? 'border-[#ef4444]/40 bg-[#ef4444]/[0.04]' : 'border-[#27272a] bg-[#0d0d0f]')
      }
    >
      <div className="mb-2.5 font-mono text-[13px] text-[#fafafa]">{handle(fp)}</div>
      <div
        className="mb-2 select-none font-mono text-lg leading-tight tracking-widest"
        aria-label="Fingerprint emoji"
      >
        {fp.fingerprint_emoji.join(' ')}
      </div>
      <div className="mb-1 font-mono text-[11px] text-[#a1a1aa]">
        {fp.fingerprint_words.join(' · ')}
      </div>
      <div className="font-mono text-[11px] text-[#71717a]">fp {fp.pubkey_hex.slice(0, 8)}</div>
    </div>
  );
}

export function TofuMismatchDialog({
  open,
  onOpenChange,
  artifactName,
  previously,
  incoming,
  onCancel,
  onTrustNew,
}: TofuMismatchDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[560px] overflow-hidden p-0">
        <DialogHeader className="space-y-0 border-b border-[#ef4444]/25 bg-[#ef4444]/10 px-5 py-4">
          <DialogTitle className="flex items-center gap-2.5 text-[15px] text-[#fafafa]">
            <AlertTriangle className="h-5 w-5 text-[#ef4444]" />
            Author identity changed
          </DialogTitle>
          <DialogDescription className="ml-7 mt-1 text-[11px] text-[#a1a1aa]">
            An artifact named <strong className="text-[#fafafa]">{artifactName}</strong> was
            previously published by a different author.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 p-5">
          <div
            role="note"
            className="flex items-start gap-2 rounded-md border border-[#fbbf24]/30 bg-[#f59e0b]/[0.08] p-3 text-xs leading-relaxed text-[#fbbf24]"
          >
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0" />
            <span>
              This could be the same author rotating their key, or someone impersonating them. If
              you didn&apos;t expect this change, cancel and verify with the original author through
              another channel.
            </span>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[#71717a]">
                Previously saw
              </div>
              <FingerprintColumn fp={previously} isNew={false} />
            </div>
            <div>
              <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[#ef4444]">
                This download · NEW
              </div>
              <FingerprintColumn fp={incoming} isNew />
            </div>
          </div>
        </div>

        <DialogFooter className="border-t border-[#27272a] bg-[#18181b] px-5 py-3.5">
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={onTrustNew}>
            Install as new author
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
