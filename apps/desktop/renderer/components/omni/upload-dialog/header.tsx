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
 * Chrome matches `step1-v3-outline-icons.html`: title left, Lucide `X`
 * close button right, both on a single flex row with `items-start
 * justify-between`. The close button uses `DialogClose asChild` so Radix
 * wires the close action and the host's Esc keybinding still works.
 */

import { X } from 'lucide-react';
import { DialogClose, DialogTitle } from '@/components/ui/dialog';

export interface UploadDialogHeaderProps {
  mode: 'create' | 'update';
  /** Selected artifact kind; null before the user picks an item on Step 1. */
  kind: 'overlay' | 'theme' | null;
}

export function UploadDialogHeader({ mode, kind }: UploadDialogHeaderProps) {
  const subject = kind === 'theme' ? 'Theme' : 'Overlay';
  const verb = mode === 'update' ? 'Update' : 'Publish';
  return (
    <div className="flex shrink-0 items-start justify-between">
      <DialogTitle className="text-lg font-semibold">
        {verb} {subject}
      </DialogTitle>
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
