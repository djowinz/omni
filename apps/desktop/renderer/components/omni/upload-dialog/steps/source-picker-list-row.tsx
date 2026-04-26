/**
 * SourcePickerListRow — single row in the Step 1 overlay/theme list.
 *
 * Renders the chrome described by INV-7.1.8 through INV-7.1.11:
 *   - 56×36 thumbnail (preview if `entry.has_preview` else a zinc gradient
 *     placeholder; spec §8.3 backfill renders the missing preview later)
 *   - bold name + metadata subtitle:
 *       overlays → "{N} widgets · Modified YYYY-MM-DD"
 *       themes   → "Modified YYYY-MM-DD"
 *   - selection chrome: cyan border + 5% cyan tint + 20px ✓ badge on the right
 *
 * The preview source is derived from `entry.workspace_path` plus the host's
 * `data_dir`. The host RPC will eventually expose an absolute path resolver
 * (per spec §8.3); for now we render a `file:///` URL with a TODO marker so
 * the placeholder keeps working until that wiring lands.
 */

import type { PublishablesEntry } from '@omni/shared-types';

export interface SourcePickerListRowProps {
  entry: PublishablesEntry;
  selected: boolean;
  onClick: () => void;
}

export function SourcePickerListRow({ entry, selected, onClick }: SourcePickerListRowProps) {
  // TODO(upload-flow-redesign A2.1): thread the host data_dir through props
  // so this resolves to an actual on-disk preview file. Until then the
  // string is intentionally non-resolving — the `<img>` falls back to
  // transparent, leaving the gradient placeholder visible.
  const previewSrc = entry.has_preview
    ? `file:///__omni_preview__/${entry.workspace_path}/.omni-preview.png`
    : null;

  const modifiedDate = entry.modified_at ? entry.modified_at.slice(0, 10) : '';
  const subtitle =
    entry.kind === 'overlay'
      ? `${entry.widget_count ?? 0} widgets · Modified ${modifiedDate}`
      : `Modified ${modifiedDate}`;

  const borderClass = selected
    ? 'border-[#00D9FF] bg-[#00D9FF]/5'
    : 'border-[#27272A]';

  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={`source-row-${entry.workspace_path}`}
      aria-pressed={selected}
      className={`flex items-center gap-3 p-2.5 rounded-md border ${borderClass} text-left w-full`}
    >
      <div className="w-14 h-9 rounded bg-gradient-to-br from-[#27272A] to-[#3f3f46] shrink-0 overflow-hidden">
        {previewSrc && (
          <img
            src={previewSrc}
            alt=""
            className="w-full h-full object-cover rounded"
          />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <div className="text-[13px] font-semibold truncate">{entry.name}</div>
        <div className="text-[11px] text-[#a1a1aa] truncate">{subtitle}</div>
      </div>
      {selected && (
        <div
          data-testid={`source-row-check-${entry.workspace_path}`}
          className="w-5 h-5 rounded-full bg-[#00D9FF] text-[#09090B] flex items-center justify-center font-bold text-xs shrink-0"
        >
          ✓
        </div>
      )}
    </button>
  );
}
