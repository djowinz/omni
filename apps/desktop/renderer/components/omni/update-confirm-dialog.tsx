/**
 * UpdateConfirmDialog — modal shown when the user clicks the header Update
 * pill on an Installed-tab artifact. Surfaces the version delta, author,
 * timestamp, and optional pubkey-rotation pre-warning, then fires
 * explorer.install with overwrite=true.
 */
import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';
import { X } from 'lucide-react';
import { useShareWs } from '@/hooks/use-share-ws';
import { installFolderPath } from '@/lib/artifact-actions';
import { toast } from 'sonner';
import type { InstalledEntryRow, ArtifactDetail } from '@/lib/share-types';

export interface UpdateConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  artifact: ArtifactDetail;
  installed: InstalledEntryRow;
  onApplied: () => void;
}

function relativeTime(epochSeconds: number): string {
  const delta = Math.floor(Date.now() / 1000) - epochSeconds;
  if (delta < 60) return 'just now';
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  return `${Math.floor(delta / 86400)}d ago`;
}

function shortFp(pubkey: string): string {
  return pubkey.slice(0, 8) + '…' + pubkey.slice(-4);
}

export function UpdateConfirmDialog({
  open,
  onOpenChange,
  artifact,
  installed,
  onApplied,
}: UpdateConfirmDialogProps) {
  const { send } = useShareWs();
  const [applying, setApplying] = useState(false);
  const keyRotated = installed.author_pubkey !== artifact.author_pubkey;

  const handleApply = async () => {
    setApplying(true);
    try {
      const installFolder = installFolderPath(artifact.manifest.name, artifact.artifact_id);
      // Same wire shape as a fresh install — `overwrite: true` is the only
      // delta. expected_pubkey pre-pins the locally-trusted key so the
      // install pipeline can detect rotation and surface TofuMismatchDialog.
      await send('explorer.install', {
        artifact_id: artifact.artifact_id,
        target_workspace_path: installFolder,
        overwrite: true,
        expected_pubkey: installed.author_pubkey,
      });
      onOpenChange(false);
      onApplied();
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    } finally {
      setApplying(false);
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 z-50 w-[440px] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-[#27272A] bg-[#18181B] p-5 text-[#FAFAFA] shadow-xl"
          data-testid="update-confirm-dialog"
        >
          <div className="flex items-start justify-between mb-3">
            <Dialog.Title className="text-base font-semibold">
              Update {artifact.manifest.name}?
            </Dialog.Title>
            <Dialog.Close className="text-[#71717A] hover:text-[#FAFAFA]" aria-label="Close">
              <X className="h-4 w-4" />
            </Dialog.Close>
          </div>
          <div className="text-xs text-[#71717A] mb-4">
            Bundle · by {artifact.author_display_name ?? 'unknown'}
          </div>
          <div className="flex items-center justify-between gap-3 mb-3">
            <div className="flex-1 rounded border border-[#27272A] bg-[#0a0a0c] p-3 text-center">
              <div className="text-[10px] uppercase text-[#71717A] tracking-wider">Installed</div>
              <div className="text-sm font-mono mt-1">v{installed.installed_version}</div>
            </div>
            <div className="text-[#34D399] text-lg">→</div>
            <div className="flex-1 rounded border border-[#34D399] bg-[#0a0a0c] p-3 text-center">
              <div className="text-[10px] uppercase text-[#34D399] tracking-wider">Latest</div>
              <div className="text-sm font-mono mt-1 text-[#34D399]">v{artifact.manifest.version}</div>
            </div>
          </div>
          <div className="text-xs text-[#71717A] mb-3">Published {relativeTime(artifact.updated_at)}</div>
          <div className="text-xs text-[#FCA5A5] bg-[rgba(127,29,29,0.15)] border border-[#7F1D1D] rounded p-2 mb-3">
            ⚠ Updating will replace the installed files. Local edits to overlays/{artifact.manifest.name}/ will be overwritten.
          </div>
          {keyRotated && (
            <div className="text-xs text-[#FCD34D] bg-[rgba(120,53,15,0.15)] border border-[#92400E] rounded p-2 mb-3">
              ⚠ Author key changed since you installed this.
              <div className="mt-1 font-mono text-[10px]">
                Previously: {shortFp(installed.author_pubkey)}
                <br />
                Now: {shortFp(artifact.author_pubkey)}
              </div>
              Applying the update will prompt to re-trust.
            </div>
          )}
          <div className="flex justify-end gap-2 pt-3 border-t border-[#27272A]">
            <button
              type="button"
              onClick={() => onOpenChange(false)}
              className="rounded border border-[#3F3F46] px-3 py-1.5 text-xs hover:bg-[#27272A]"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={handleApply}
              disabled={applying}
              className="rounded bg-[#34D399] text-[#022C22] px-3 py-1.5 text-xs font-semibold hover:brightness-110 disabled:opacity-50"
            >
              {applying ? 'Applying…' : 'Apply'}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
