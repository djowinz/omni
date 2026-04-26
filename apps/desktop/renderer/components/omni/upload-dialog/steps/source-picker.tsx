/**
 * Step 1 — Source Picker.
 *
 * Composes the three pieces described in spec §7.1:
 *   1. Conditional banner (INV-7.6.2 linked-artifact in update mode, or
 *      INV-7.6.4 pubkey mismatch when a sidecar's author differs from the
 *      current identity in create mode).
 *   2. "What would you like to publish?" header + 2-column type-card grid
 *      using Lucide `Package` (Bundle) + `Palette` (Theme) icons at 32px /
 *      stroke 1.75 (INV-7.1.3 / INV-7.1.4).
 *   3. "Select an Overlay" / "Select a Theme" header + filtered list of
 *      `SourcePickerListRow` rows (filter by `state.selectedKind`).
 *
 * The component is presentational — it receives `state` + `actions` from the
 * parent dialog (OWI-35 scaffolds the dialog with stub state initially; real
 * machine wiring lands at A2.1). Workspace data is fetched directly via
 * `useWorkspaceList` since the rich `entries` array is the rendering input
 * for INV-7.1.9 / INV-7.1.10 / INV-7.1.13 — passing it down through props
 * would just re-export the hook's shape verbatim.
 */

import type { ComponentType, SVGProps } from 'react';
import { Package, Palette } from 'lucide-react';
import type { PublishablesEntry } from '@omni/shared-types';
import { useWorkspaceList } from '../../../../hooks/use-workspace-list';
import { SourcePickerListRow } from './source-picker-list-row';
import { LinkedArtifactBanner, PubkeyMismatchBanner } from './source-picker-banners';

export type SelectedKind = 'overlay' | 'theme';

export interface SourcePickerState {
  selectedKind: SelectedKind | null;
  selected: PublishablesEntry | null;
  mode: 'create' | 'update';
  currentPubkey: string | null;
}

export interface SourcePickerActions {
  selectKind: (kind: SelectedKind) => void;
  selectItem: (entry: PublishablesEntry) => void;
}

export interface SourcePickerProps {
  state: SourcePickerState;
  actions: SourcePickerActions;
}

export function SourcePicker({ state, actions }: SourcePickerProps) {
  const { entries, loading } = useWorkspaceList();
  const selectedKind = state.selectedKind;
  const filteredEntries = selectedKind ? entries.filter((e) => e.kind === selectedKind) : [];

  const sidecar = state.selected?.sidecar ?? null;
  const showLinked = sidecar !== null && state.mode === 'update';
  const showMismatch =
    sidecar !== null &&
    state.mode === 'create' &&
    state.currentPubkey !== null &&
    sidecar.author_pubkey_hex !== state.currentPubkey;

  // Header copy depends on selectedKind. Default ("nothing chosen yet") falls
  // back to overlay phrasing — the type-card grid renders directly above so
  // there's no UX confusion.
  const isThemeSelected = selectedKind === 'theme';
  const itemsHeading = isThemeSelected ? 'Select a Theme' : 'Select an Overlay';
  const itemsSubtitle = isThemeSelected
    ? 'Choose from your existing themes built in Omni'
    : 'Choose from your existing overlays built in Omni';

  return (
    <div className="flex flex-col gap-4" data-testid="source-picker">
      {/* Banners (INV-7.6.2 / INV-7.6.4) */}
      {showLinked && sidecar && <LinkedArtifactBanner sidecar={sidecar} />}
      {showMismatch && sidecar && <PubkeyMismatchBanner sidecar={sidecar} />}

      {/* Section header (INV-7.1.1) */}
      <div>
        <div className="text-sm font-semibold mb-1">What would you like to publish?</div>
        <div className="text-xs text-[#a1a1aa]">Choose the type of content you want to share</div>
      </div>

      {/* Type cards (INV-7.1.2 through INV-7.1.6) */}
      <div className="grid grid-cols-2 gap-3">
        <TypeCard
          testId="type-card-overlay"
          icon={Package}
          label="Bundle"
          sublabel={'overlay.omni, fonts,\nimages, theme.css'}
          selected={selectedKind === 'overlay'}
          onClick={() => actions.selectKind('overlay')}
        />
        <TypeCard
          testId="type-card-theme"
          icon={Palette}
          label="Theme"
          sublabel="Theme file only"
          selected={selectedKind === 'theme'}
          onClick={() => actions.selectKind('theme')}
        />
      </div>

      {/* Items list (INV-7.1.7 through INV-7.1.11) */}
      <div>
        <div className="text-sm font-semibold mb-1">{itemsHeading}</div>
        <div className="text-xs text-[#a1a1aa] mb-3">{itemsSubtitle}</div>
        {loading ? (
          <p className="text-sm text-zinc-500" data-testid="source-picker-loading">
            Loading workspace…
          </p>
        ) : selectedKind === null ? (
          <p className="text-sm text-zinc-500" data-testid="source-picker-no-kind">
            Select a type above to see your {isThemeSelected ? 'themes' : 'overlays'}.
          </p>
        ) : filteredEntries.length === 0 ? (
          <p className="text-sm text-zinc-500" data-testid="source-picker-empty">
            No {isThemeSelected ? 'themes' : 'overlays'} in your workspace yet.
          </p>
        ) : (
          <div className="flex flex-col gap-2" data-testid="source-picker-list">
            {filteredEntries.map((entry) => (
              <SourcePickerListRow
                key={entry.workspace_path}
                entry={entry}
                selected={state.selected?.workspace_path === entry.workspace_path}
                onClick={() => actions.selectItem(entry)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// Lucide icons accept the same props as bare SVGs. Typed loosely so the
// `Package` / `Palette` exports line up without leaking lucide-react types
// into the public SourcePicker surface.
type IconComponent = ComponentType<
  SVGProps<SVGSVGElement> & { size?: number; strokeWidth?: number }
>;

interface TypeCardProps {
  testId: string;
  icon: IconComponent;
  label: string;
  sublabel: string;
  selected: boolean;
  onClick: () => void;
}

function TypeCard({ testId, icon: Icon, label, sublabel, selected, onClick }: TypeCardProps) {
  const borderClass = selected ? 'border-[#00D9FF] bg-[#00D9FF]/[0.07]' : 'border-[#27272A]';
  const iconColor = selected ? 'text-[#00D9FF]' : 'text-[#a1a1aa]';
  const labelColor = selected ? 'text-[#FAFAFA]' : 'text-[#d4d4d8]';
  const sublabelColor = selected ? 'text-[#a1a1aa]' : 'text-[#71717a]';

  return (
    <button
      type="button"
      data-testid={testId}
      aria-pressed={selected}
      onClick={onClick}
      className={`relative px-4 py-[22px] rounded-md border ${borderClass} text-center`}
    >
      {selected && (
        <div
          data-testid={`${testId}-check`}
          className="absolute top-2.5 right-2.5 w-5 h-5 rounded-full bg-[#00D9FF] text-[#09090B] flex items-center justify-center font-bold text-xs"
        >
          ✓
        </div>
      )}
      <Icon className={`mx-auto mb-2.5 ${iconColor}`} size={32} strokeWidth={1.75} />
      <div className={`text-sm font-semibold mb-1 ${labelColor}`}>{label}</div>
      <div className={`text-[11px] leading-snug whitespace-pre-line ${sublabelColor}`}>
        {sublabel}
      </div>
    </button>
  );
}
