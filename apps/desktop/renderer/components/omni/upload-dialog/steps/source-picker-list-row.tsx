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
 * The preview source is derived from `entry.workspace_path` and resolved
 * through the `omni-preview://` Electron protocol handler registered in
 * `apps/desktop/main/main.ts`. That handler maps `omni-preview://<segment>/<rest>`
 * to `<userData>/<segment>/<rest>` — `<userData>` matches the Rust host's
 * `config::data_dir()` (both `%APPDATA%/Omni`).
 */

import { CheckCircle2 } from 'lucide-react';
import type { PublishablesEntry } from '@omni/shared-types';

export interface SourcePickerListRowProps {
  entry: PublishablesEntry;
  selected: boolean;
  onClick: () => void;
  /**
   * The currently-loaded identity's pubkey hex. Used to decide whether a
   * row's sidecar represents an artifact authored by THIS user — only then
   * should the row surface a "v1.x.x Published" badge (otherwise the
   * sidecar belongs to a different author and the row is a fresh publish).
   */
  currentPubkey: string | null;
}

/**
 * Build the `omni-preview://` URL for a publishable entry, or `null` when
 * no save-time preview exists (the row then renders the zinc gradient).
 *
 * Overlays: preview lives at `<data_dir>/overlays/<name>/.omni-preview.png`
 *   (per `crates/host/src/share/save_preview.rs::OVERLAY_PREVIEW_FILENAME`).
 * Themes:   preview lives at `<data_dir>/themes/<base>.preview.png` where
 *   `<base>` is the theme filename minus its `.css` extension (per
 *   `crates/host/src/share/ws_messages.rs` listPublishables — the `has_preview`
 *   probe). `entry.workspace_path` for themes is `themes/<filename>.css`,
 *   so we strip `.css` from the trailing segment.
 */
function previewUrlFor(entry: PublishablesEntry): string | null {
  if (!entry.has_preview) return null;
  if (entry.kind === 'overlay') {
    return `omni-preview://${entry.workspace_path}/.omni-preview.png`;
  }
  // theme: `themes/<filename>.css` → `omni-preview://themes/<base>.preview.png`
  const base = entry.workspace_path.replace(/\.css$/i, '');
  return `omni-preview://${base}.preview.png`;
}

export function SourcePickerListRow({
  entry,
  selected,
  onClick,
  currentPubkey,
}: SourcePickerListRowProps) {
  const previewSrc = previewUrlFor(entry);

  const modifiedDate = entry.modified_at ? entry.modified_at.slice(0, 10) : '';
  const subtitle =
    entry.kind === 'overlay'
      ? `${entry.widget_count ?? 0} widgets · Modified ${modifiedDate}`
      : `Modified ${modifiedDate}`;

  const borderClass = selected ? 'border-[#00D9FF] bg-[#00D9FF]/5' : 'border-[#27272A]';

  // "Published by you" badge: only when the entry has a sidecar AND its
  // author_pubkey_hex matches the current identity. Mirrors the resolveMode
  // logic in use-upload-machine.ts — a sidecar from a different author is
  // a fresh-publish situation, not an update.
  const publishedByMe =
    entry.sidecar !== null &&
    currentPubkey !== null &&
    entry.sidecar.author_pubkey_hex === currentPubkey;

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
          <img src={previewSrc} alt="" className="w-full h-full object-cover rounded" />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <div className="text-[13px] font-semibold truncate">{entry.name}</div>
          {publishedByMe && entry.sidecar && (
            <span
              data-testid={`source-row-published-${entry.workspace_path}`}
              className="flex shrink-0 items-center gap-1 rounded-md border border-[#00D9FF]/30 bg-[#00D9FF]/[0.08] px-1.5 py-0.5 text-[10px] font-medium text-[#00D9FF]"
              title={`Published by you · last v${entry.sidecar.version} · clicking will update`}
            >
              <CheckCircle2 className="h-3 w-3" aria-hidden />
              v{entry.sidecar.version}
            </span>
          )}
        </div>
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
