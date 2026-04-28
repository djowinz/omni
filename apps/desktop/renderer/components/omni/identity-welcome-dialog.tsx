import { UserPlus, Upload, ChevronRight } from 'lucide-react';
import { Dialog, DialogContent, DialogTitle, DialogDescription } from '@/components/ui/dialog';

export interface IdentityWelcomeDialogProps {
  open: boolean;
  onSetUpNew: () => void;
  onImport: () => void;
}

export function IdentityWelcomeDialog({ open, onSetUpNew, onImport }: IdentityWelcomeDialogProps) {
  return (
    <Dialog open={open}>
      <DialogContent
        className="sm:max-w-[520px] overflow-hidden p-0"
        onPointerDownOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <div className="px-7 pt-7 text-center">
          <div className="relative mx-auto mb-4 h-12 w-12">
            <div
              aria-hidden
              className="absolute inset-0 rounded-[10px] opacity-55 blur-[14px]"
              style={{ background: 'linear-gradient(135deg,#00D9FF 0%,#A855F7 100%)' }}
            />
            <div
              className="relative flex h-12 w-12 items-center justify-center rounded-[10px]"
              style={{ background: 'linear-gradient(135deg,#00D9FF 0%,#A855F7 100%)' }}
            >
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0d0d0f" strokeWidth="2.5">
                <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
              </svg>
            </div>
          </div>
          <DialogTitle className="text-[20px] font-semibold text-[#fafafa]">
            Welcome to Omni
          </DialogTitle>
          <DialogDescription className="mt-1.5 text-[13px] leading-relaxed text-[#a1a1aa]">
            Pick how you want to set up your author identity.
          </DialogDescription>
        </div>

        <div className="px-7 pb-7 pt-2">
          <div className="flex flex-col gap-2">
            <button
              type="button"
              autoFocus
              onClick={onSetUpNew}
              className="flex w-full items-start gap-3.5 rounded-lg border border-[#00d9ff]/40 bg-[#00d9ff]/[0.05] p-3.5 text-left hover:border-[#00d9ff] hover:bg-[#00d9ff]/[0.10]"
            >
              <span className="flex h-[34px] w-[34px] flex-shrink-0 items-center justify-center rounded-md bg-[#00d9ff]/10 text-[#00d9ff]">
                <UserPlus className="h-[18px] w-[18px]" />
              </span>
              <span className="flex-1">
                <span className="block text-[13px] font-semibold text-[#fafafa]">
                  Set up a new identity
                </span>
                <span className="mt-1 block text-[11px] leading-relaxed text-[#a1a1aa]">
                  Generate a fresh signing key on this device. You can back it up later.
                </span>
              </span>
              <ChevronRight className="h-3.5 w-3.5 flex-shrink-0 self-center text-[#52525b]" />
            </button>
            <button
              type="button"
              onClick={onImport}
              className="flex w-full items-start gap-3.5 rounded-lg border border-[#27272a] bg-[#0d0d0f] p-3.5 text-left hover:border-[#3f3f46] hover:bg-[#1f1f24]"
            >
              <span className="flex h-[34px] w-[34px] flex-shrink-0 items-center justify-center rounded-md bg-[#27272a] text-[#a1a1aa]">
                <Upload className="h-[18px] w-[18px]" />
              </span>
              <span className="flex-1">
                <span className="block text-[13px] font-semibold text-[#fafafa]">
                  Import existing identity
                </span>
                <span className="mt-1 block text-[11px] leading-relaxed text-[#a1a1aa]">
                  Restore from an encrypted{' '}
                  <code className="font-mono text-[10px] text-[#fafafa]">.omniid</code> backup so
                  your prior uploads stay yours.
                </span>
              </span>
              <ChevronRight className="h-3.5 w-3.5 flex-shrink-0 self-center text-[#52525b]" />
            </button>
          </div>
          <div className="mt-3.5 rounded-md border border-[#27272a] bg-[#0d0d0f] p-3.5 text-[11px] leading-relaxed text-[#a1a1aa]">
            <strong className="text-[#fafafa]">Already published from another machine?</strong>{' '}
            Import your existing identity here so your uploads stay attributed to the same author.
            Once you set up a new identity, prior uploads from another device will appear under a
            different author.
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
