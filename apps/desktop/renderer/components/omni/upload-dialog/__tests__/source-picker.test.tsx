/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * SourcePicker tests — Step 1 chrome + sidecar banners.
 *
 * Mocks `useWorkspaceList` so the component renders deterministically
 * without spinning up a real Share-WS round-trip. Fixtures:
 *   - one overlay with a sidecar whose `author_pubkey_hex` matches the
 *     "current pubkey" used in the update-mode test
 *   - one theme without a sidecar
 *
 * Coverage matrix (per dispatch prompt):
 *   - both type cards render
 *   - clicking a type card invokes `actions.selectKind`
 *   - list rows render with the correct subtitle format (overlay vs theme)
 *   - selection chrome appears on the selected row
 *   - LinkedArtifactBanner appears in update mode when sidecar is present
 *   - PubkeyMismatchBanner appears in create mode when the sidecar's author
 *     differs from the current identity
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import type { PublishablesEntry, PublishSidecar } from '@omni/shared-types';

const SIDECAR: PublishSidecar = {
  artifact_id: 'ov_01J8XKZ9FABCDEF',
  author_pubkey_hex: 'aaaaaaaabbbbbbbbccccccccdddddddd',
  version: '1.3.0',
  last_published_at: '2026-04-18T12:00:00Z',
};

const OVERLAY_ENTRY: PublishablesEntry = {
  kind: 'overlay',
  workspace_path: 'overlays/full-telemetry',
  name: 'Full Telemetry',
  widget_count: 12,
  modified_at: '2026-04-10T15:30:00Z',
  has_preview: true,
  sidecar: SIDECAR,
};

const THEME_ENTRY: PublishablesEntry = {
  kind: 'theme',
  workspace_path: 'themes/synth.css',
  name: 'synth',
  widget_count: null,
  modified_at: '2026-04-05T09:00:00Z',
  has_preview: false,
  sidecar: null,
};

// Mock the hook — vi.mock is hoisted, so the factory must not close over
// outer variables. We swap the returned shape per-test by mutating the
// `mockState` object the factory reads at call time.
const mockState: {
  entries: PublishablesEntry[];
  loading: boolean;
} = { entries: [], loading: false };

vi.mock('../../../../hooks/use-workspace-list', () => ({
  useWorkspaceList: () => ({
    entries: mockState.entries,
    overlays: mockState.entries.filter((e) => e.kind === 'overlay').map((e) => e.name),
    themes: mockState.entries
      .filter((e) => e.kind === 'theme')
      .map((e) => e.workspace_path.split('/').pop() ?? e.workspace_path),
    loading: mockState.loading,
    error: null,
    refetch: vi.fn(),
  }),
}));

// Import AFTER vi.mock so the component picks up the mocked hook.
import { SourcePicker } from '../steps/source-picker';
import type {
  SourcePickerActions,
  SourcePickerState,
} from '../steps/source-picker';

function makeActions(overrides: Partial<SourcePickerActions> = {}): SourcePickerActions {
  return {
    selectKind: vi.fn(),
    selectItem: vi.fn(),
    ...overrides,
  };
}

function makeState(overrides: Partial<SourcePickerState> = {}): SourcePickerState {
  return {
    selectedKind: 'overlay',
    selected: null,
    mode: 'create',
    currentPubkey: SIDECAR.author_pubkey_hex,
    ...overrides,
  };
}

describe('SourcePicker', () => {
  beforeEach(() => {
    mockState.entries = [OVERLAY_ENTRY, THEME_ENTRY];
    mockState.loading = false;
  });

  it('renders both type cards with correct labels', () => {
    render(<SourcePicker state={makeState({ selectedKind: null })} actions={makeActions()} />);
    expect(screen.getByTestId('type-card-overlay')).toBeInTheDocument();
    expect(screen.getByTestId('type-card-theme')).toBeInTheDocument();
    expect(screen.getByText('Bundle')).toBeInTheDocument();
    expect(screen.getByText('Theme')).toBeInTheDocument();
  });

  it('marks the selected type card with cyan border + check badge', () => {
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={makeActions()} />);
    const overlayCard = screen.getByTestId('type-card-overlay');
    expect(overlayCard.className).toMatch(/00D9FF/);
    expect(screen.getByTestId('type-card-overlay-check')).toBeInTheDocument();
    expect(screen.queryByTestId('type-card-theme-check')).toBeNull();
  });

  it('clicking a type card invokes actions.selectKind with the right key', () => {
    const actions = makeActions();
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={actions} />);
    fireEvent.click(screen.getByTestId('type-card-theme'));
    expect(actions.selectKind).toHaveBeenCalledWith('theme');
  });

  it('renders overlay rows with the "N widgets · Modified YYYY-MM-DD" subtitle', () => {
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={makeActions()} />);
    const row = screen.getByTestId(`source-row-${OVERLAY_ENTRY.workspace_path}`);
    expect(row).toBeInTheDocument();
    expect(row).toHaveTextContent('Full Telemetry');
    expect(row).toHaveTextContent('12 widgets · Modified 2026-04-10');
  });

  it('renders theme rows with the "Modified YYYY-MM-DD" subtitle (no widget count)', () => {
    render(<SourcePicker state={makeState({ selectedKind: 'theme' })} actions={makeActions()} />);
    const row = screen.getByTestId(`source-row-${THEME_ENTRY.workspace_path}`);
    expect(row).toBeInTheDocument();
    expect(row).toHaveTextContent('synth');
    expect(row).toHaveTextContent('Modified 2026-04-05');
    expect(row).not.toHaveTextContent('widgets');
  });

  it('filters the list to entries matching state.selectedKind', () => {
    render(<SourcePicker state={makeState({ selectedKind: 'theme' })} actions={makeActions()} />);
    expect(screen.queryByTestId(`source-row-${OVERLAY_ENTRY.workspace_path}`)).toBeNull();
    expect(screen.getByTestId(`source-row-${THEME_ENTRY.workspace_path}`)).toBeInTheDocument();
  });

  it('clicking a list row invokes actions.selectItem with the entry', () => {
    const actions = makeActions();
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={actions} />);
    fireEvent.click(screen.getByTestId(`source-row-${OVERLAY_ENTRY.workspace_path}`));
    expect(actions.selectItem).toHaveBeenCalledWith(OVERLAY_ENTRY);
  });

  it('renders the cyan ✓ badge on the selected list row', () => {
    render(
      <SourcePicker
        state={makeState({ selectedKind: 'overlay', selected: OVERLAY_ENTRY })}
        actions={makeActions()}
      />,
    );
    expect(
      screen.getByTestId(`source-row-check-${OVERLAY_ENTRY.workspace_path}`),
    ).toBeInTheDocument();
  });

  it('renders LinkedArtifactBanner when a sidecar is selected in update mode', () => {
    render(
      <SourcePicker
        state={makeState({
          selectedKind: 'overlay',
          selected: OVERLAY_ENTRY,
          mode: 'update',
        })}
        actions={makeActions()}
      />,
    );
    const banner = screen.getByTestId('linked-artifact-banner');
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent('Linked to existing artifact');
    expect(banner).toHaveTextContent('ov_01J8XKZ9F…');
    expect(banner).toHaveTextContent('v1.3.0');
    expect(banner).toHaveTextContent('2026-04-18');
    expect(banner).toHaveTextContent('This upload will be an update.');
    expect(screen.queryByTestId('pubkey-mismatch-banner')).toBeNull();
  });

  it('renders PubkeyMismatchBanner when sidecar.author_pubkey_hex differs from currentPubkey in create mode', () => {
    render(
      <SourcePicker
        state={makeState({
          selectedKind: 'overlay',
          selected: OVERLAY_ENTRY,
          mode: 'create',
          currentPubkey: 'ffffffffffffffffffffffffffffffff',
        })}
        actions={makeActions()}
      />,
    );
    const banner = screen.getByTestId('pubkey-mismatch-banner');
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent('Originally published by a different identity');
    expect(banner).toHaveTextContent('aaaaaaaabbbb…');
    expect(banner).toHaveTextContent('This upload will be a new artifact.');
    expect(screen.queryByTestId('linked-artifact-banner')).toBeNull();
  });

  it('does not render either banner when nothing is selected', () => {
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={makeActions()} />);
    expect(screen.queryByTestId('linked-artifact-banner')).toBeNull();
    expect(screen.queryByTestId('pubkey-mismatch-banner')).toBeNull();
  });

  it('does not render PubkeyMismatchBanner when sidecar matches currentPubkey in create mode', () => {
    render(
      <SourcePicker
        state={makeState({
          selectedKind: 'overlay',
          selected: OVERLAY_ENTRY,
          mode: 'create',
          currentPubkey: SIDECAR.author_pubkey_hex,
        })}
        actions={makeActions()}
      />,
    );
    expect(screen.queryByTestId('pubkey-mismatch-banner')).toBeNull();
    expect(screen.queryByTestId('linked-artifact-banner')).toBeNull();
  });

  it('shows a loading message when useWorkspaceList is loading', () => {
    mockState.loading = true;
    mockState.entries = [];
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={makeActions()} />);
    expect(screen.getByTestId('source-picker-loading')).toBeInTheDocument();
  });

  it('shows an empty-state message when no entries match the selected kind', () => {
    mockState.entries = [THEME_ENTRY];
    render(<SourcePicker state={makeState({ selectedKind: 'overlay' })} actions={makeActions()} />);
    expect(screen.getByTestId('source-picker-empty')).toBeInTheDocument();
  });
});
