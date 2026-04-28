import { RotateCw, AlertTriangle } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useIdentity } from '../../lib/identity-context';
import { useShareWs } from '../../hooks/use-share-ws';
import { toast } from '../../lib/toast';

export interface RotateConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onBackupNow: () => void;
}

export function RotateConfirmDialog({ open, onOpenChange, onBackupNow }: RotateConfirmDialogProps) {
  const { identity, refresh } = useIdentity();
  const { send } = useShareWs();

  const carryName = identity?.display_name ?? null;

  const onConfirm = async () => {
    try {
      await send('identity.rotate', {});
      await refresh();
      onOpenChange(false);
      toast.warning('Identity rotated — back up your new identity', {
        description: 'Your previous .omniid backup no longer works. Save a fresh one now.',
        duration: Infinity,
        action: { label: 'Back up now', onClick: onBackupNow },
      });
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[480px] overflow-hidden p-0">
        <DialogHeader className="space-y-0 border-b border-[#27272a] px-5 py-4">
          <DialogTitle className="flex items-center gap-2.5 text-[15px] text-[#fafafa]">
            <RotateCw className="h-5 w-5 text-[#a1a1aa]" />
            Rotate your signing key?
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-3 p-5">
          <p className="text-[13px] leading-relaxed text-[#a1a1aa]">
            This generates a new keypair on this device. Your existing uploads keep their current
            author identity; future uploads will be signed by the new key.
          </p>
          <p className="text-[13px] leading-relaxed text-[#a1a1aa]">
            <strong className="text-[#fafafa]">Your display name will carry over</strong> to the
            new identity automatically.{' '}
            {carryName && (
              <>
                The new pubkey will be seeded on the worker as{' '}
                <code className="font-mono text-[#fafafa]">{carryName}</code> on your next upload
                (or sooner if the worker is reachable).
              </>
            )}
          </p>
          <div
            role="note"
            className="flex items-start gap-2 rounded-md border border-[#ef4444]/25 bg-[#ef4444]/[0.06] p-3 text-[12px] leading-relaxed text-[#fca5a5]"
          >
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0" />
            <span>
              <strong className="text-[#ef4444]">
                Your existing backup will no longer decrypt the new key.
              </strong>{' '}
              Back up the new identity right after rotating. The publish gate will re-arm until you
              do.
            </span>
          </div>
        </div>

        <DialogFooter className="border-t border-[#27272a] bg-[#18181b] px-5 py-3.5">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            Rotate
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
