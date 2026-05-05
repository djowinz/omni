/**
 * UploadDialogHeader — title + close affordance for the new UploadDialog.
 *
 * Title copy follows INV-7.0.4:
 *   create + overlay → "Publish Overlay"
 *   create + theme   → "Publish Theme"
 *   update + overlay → "Update Overlay"   (INV-7.5.1)
 *   update + theme   → "Update Theme"     (INV-7.5.1)
 *   any  + null kind → "Publish Overlay"  (default before user selects)
 *
 * In update mode a cyan pill renders next to the title carrying the
 * currently-published version (e.g. `v1.0.0`) so the user sees they're
 * updating an existing artifact across all 4 steps — the title verb alone
 * was easy to miss.
 *
 * Chrome matches `step1-v3-outline-icons.html`: title left, Lucide `X`
 * close button right, both on a single flex row with `items-start
 * justify-between`. The close button uses `DialogClose asChild` so Radix
 * wires the close action and the host's Esc keybinding still works.
 */

import { CheckCircle2, X } from 'lucide-react';
import { DialogClose, DialogTitle } from '@/components/ui/dialog';

export interface UploadDialogHeaderProps {
  mode: 'create' | 'update';
  /** Selected artifact kind; null before the user picks an item on Step 1. */
  kind: 'overlay' | 'theme' | null;
  /**
   * Currently-published semver (from the selected entry's sidecar) when in
   * update mode. Renders as a cyan "v1.0.0" pill next to the title so the
   * user sees which version they're updating.
   */
  currentVersion?: string | null;
}

export function UploadDialogHeader({ mode, kind, currentVersion = null }: UploadDialogHeaderProps) {
  const subject = kind === 'theme' ? 'Theme' : 'Overlay';
  const verb = mode === 'update' ? 'Update' : 'Publish';
  return (
    <div className="flex shrink-0 items-start justify-between">
      <div className="flex items-center gap-2.5">
        <DialogTitle className="text-lg font-semibold">
          {verb} {subject}
        </DialogTitle>
        {mode === 'update' && currentVersion && (
          <span
            data-testid="upload-dialog-update-pill"
            className="flex items-center gap-1 rounded-md border border-[#00D9FF]/30 bg-[#00D9FF]/[0.08] px-2 py-0.5 text-[11px] font-medium text-[#00D9FF]"
            title={`You're updating an existing artifact you previously published at v${currentVersion}.`}
          >
            <CheckCircle2 className="h-3 w-3" aria-hidden />
            Updating v{currentVersion}
          </span>
        )}
      </div>
      <DialogClose asChild>
        <button
          type="button"
          aria-label="Close"
          className="text-zinc-500 hover:text-zinc-300 transition-colors"
        >
          <X className="h-5 w-5" />
        </button>
      </DialogClose>
    </div>
  );
}
