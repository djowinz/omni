/**
 * CardHoverOverlay — hover-revealed action overlay for the Explore grid card.
 *
 * Per share-explorer-redesign spec §4.4: visible only on parent card hover
 * via the `group-hover` Tailwind pattern. Both child buttons stop click
 * propagation so they don't bubble into the card's own onClick (which opens
 * the detail pane).
 */

import { Download, Eye } from 'lucide-react';

export interface CardHoverOverlayProps {
  onPreview: () => void;
  onInstall: () => void;
}

export function CardHoverOverlay({ onPreview, onInstall }: CardHoverOverlayProps) {
  return (
    <div className="pointer-events-none absolute inset-0 flex items-center justify-center gap-2.5 bg-black/[0.62] opacity-0 backdrop-blur-[2px] transition-opacity group-hover:pointer-events-auto group-hover:opacity-100">
      <button
        type="button"
        aria-label="Preview"
        onClick={(e) => {
          e.stopPropagation();
          onPreview();
        }}
        className="flex h-9 w-9 items-center justify-center rounded-full border border-zinc-600 bg-zinc-900/90 text-zinc-100 hover:bg-zinc-800"
      >
        <Eye className="h-4 w-4" aria-hidden />
      </button>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onInstall();
        }}
        className="flex h-9 items-center gap-1.5 rounded-full bg-[#00D9FF] px-4 text-xs font-semibold text-[#0D0D0F] shadow-[0_2px_8px_rgba(0,217,255,0.25)] hover:bg-[#33E0FF]"
      >
        <Download className="h-3.5 w-3.5" aria-hidden />
        Install
      </button>
    </div>
  );
}
