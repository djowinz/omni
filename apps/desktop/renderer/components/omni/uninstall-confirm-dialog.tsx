/**
 * UninstallConfirmDialog — confirms uninstall, calls `explorer.uninstall`,
 * fires success callback so the panel can refetch installed state +
 * refresh the overlay dropdown.
 *
 * Modeled on RotateConfirmDialog: dialog owns the WS round-trip + toast
 * surface; the parent only supplies the artifact name/id and the
 * post-success refetch hooks. Errors stay inside the dialog and surface
 * via toast — the dialog stays open on failure so the user can retry.
 */

import { Trash2, AlertTriangle } from 'lucide-react';
import { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useShareWs } from '../../hooks/use-share-ws';
import { toast } from '../../lib/toast';

export interface UninstallConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  artifactId: string;
  /** Display name shown in the prompt. Falls back to the artifact_id slice if absent. */
  artifactName: string;
  /** Called after the WS round-trip succeeds AND the dialog closes. The
   *  panel uses this to refetch the installed registry, refresh overlays,
   *  and close the detail pane. */
  onUninstalled: () => void;
}

export function UninstallConfirmDialog({
  open,
  onOpenChange,
  artifactId,
  artifactName,
  onUninstalled,
}: UninstallConfirmDialogProps) {
  const { send } = useShareWs();
  const [busy, setBusy] = useState(false);

  const onConfirm = async () => {
    setBusy(true);
    // Diagnostic: this lets the user inspect DevTools when the dialog
    // appears to succeed but registry / on-disk content doesn't update.
    // The most common cause is a stale host binary that doesn't have the
    // `explorer.uninstall` dispatch arm — the WS request hangs or returns
    // a "missing arm" drift error. The before/after pair makes this
    // observable without redeploying.
    console.log('[UninstallConfirmDialog] sending explorer.uninstall', { artifactId });
    try {
      const result = await send('explorer.uninstall', { artifact_id: artifactId });
      console.log('[UninstallConfirmDialog] explorer.uninstall result', result);
      onOpenChange(false);
      toast.success(`Uninstalled ${artifactName}`);
      onUninstalled();
    } catch (err) {
      console.error('[UninstallConfirmDialog] explorer.uninstall failed', err);
      toast.error(err as Parameters<typeof toast.error>[0]);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => (busy ? null : onOpenChange(o))}>
      <DialogContent className="overflow-hidden p-0 sm:max-w-[480px]">
        <DialogHeader className="space-y-0 border-b border-[#27272a] px-5 py-4">
          <DialogTitle className="flex items-center gap-2.5 text-[15px] text-[#fafafa]">
            <Trash2 className="h-5 w-5 text-[#a1a1aa]" />
            Uninstall {artifactName}?
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-3 p-5">
          <p className="text-[13px] leading-relaxed text-[#a1a1aa]">
            This removes the installed files from your system and clears the entry from your
            installed-artifacts registry. You can reinstall it from the Discover tab anytime.
          </p>
          <div
            role="note"
            className="flex items-start gap-2 rounded-md border border-[#ef4444]/25 bg-[#ef4444]/[0.06] p-3 text-[12px] leading-relaxed text-[#fca5a5]"
          >
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0" />
            <span>
              <strong className="text-[#ef4444]">Local edits and forks are not touched</strong> —
              only the installed copy is removed. Any overlay you forked from this artifact stays
              put.
            </span>
          </div>
        </div>

        <DialogFooter className="border-t border-[#27272a] bg-[#18181b] px-5 py-3.5">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={onConfirm} disabled={busy}>
            {busy ? 'Removing…' : 'Remove'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
