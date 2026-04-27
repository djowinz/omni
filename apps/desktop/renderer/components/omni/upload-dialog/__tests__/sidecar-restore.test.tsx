/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * Sidecar silent-restore integration test (T-A2.6 / OWI-46, spec §8.1, §8.2,
 * INV-7.6.1, INV-7.6.2).
 *
 * Pins the renderer-side end of the silent-restore contract: when the host
 * (via the workspace.listPublishables RPC) returns a publishable entry whose
 * `sidecar` is populated AND whose `author_pubkey_hex` matches the running
 * identity, selecting that entry on Step 1 must:
 *
 *   1. Auto-detect update mode — the dialog header flips from
 *      "Publish Overlay" to "Update Overlay" (INV-7.5.1).
 *   2. Render the cyan LinkedArtifactBanner — INV-7.6.2 requires the
 *      "Linked to existing artifact — {prefix}… · v{X.Y.Z} on {date}.
 *      This upload will be an update." copy at the top of Step 1.
 *
 * The host-side detail (sidecar absent on disk → publish-index lookup →
 * sidecar restored on disk before the WS response is built) is invisible to
 * the renderer. The wire boundary is `workspace.listPublishables.entries[].sidecar`
 * being populated. This test stages the wire boundary directly and asserts
 * the renderer's downstream behaviour, which is exactly what spec §7 / §8.1
 * pin as the silent-restore contract.
 *
 * Mock pattern: stub the `window.omni` bridge that `useShareWs` reads
 * (matches the smoke test at `components/omni/__tests__/upload-dialog.test.tsx`).
 * Routing the test through the real `useShareWs` keeps the validation path
 * (Zod parse of the wire frame) honest — a malformed sidecar shape would
 * fail schema validation and the test would never reach the assertions.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';

// ── Fixtures ─────────────────────────────────────────────────────────────────

// Same hex pubkey is returned by identity.show AND embedded in the sidecar's
// author_pubkey_hex — that's the equality the machine's `detectMode` checks
// to decide update vs create (INV-7.5.* trigger condition).
const PUBKEY_HEX = 'abcd' + '0'.repeat(60); // 64-char lowercase hex (32 bytes)

const SIDECAR = {
  artifact_id: 'ov_01J8XKZ9F9ABCDEF',
  author_pubkey_hex: PUBKEY_HEX,
  version: '1.3.0',
  last_published_at: '2026-04-18T18:12:44Z',
};

const OVERLAY_ENTRY = {
  kind: 'overlay' as const,
  workspace_path: 'overlays/full-telemetry',
  name: 'Full Telemetry',
  widget_count: 12,
  modified_at: '2026-04-10T15:30:00Z',
  has_preview: false,
  sidecar: SIDECAR,
};

const IDENTITY_RESULT = {
  type: 'identity.showResult',
  params: {
    pubkey_hex: PUBKEY_HEX,
    fingerprint_hex: '',
    fingerprint_emoji: [],
    fingerprint_words: [],
    created_at: 0,
    backed_up: true,
    display_name: null,
    last_backed_up_at: null,
    last_rotated_at: null,
    last_backup_path: null,
  },
};

const LIST_RESULT = {
  type: 'workspace.listPublishablesResult',
  params: { entries: [OVERLAY_ENTRY] },
};

describe('UploadDialog — sidecar silent-restore (T-A2.6 / OWI-46)', () => {
  beforeEach(() => {
    vi.resetModules();
    // IdentityBackupDialog is mounted by UploadDialog and calls useBackend().
    // Stub it so the component tree mounts without hitting the real backend.
    vi.doMock('../../../../hooks/use-backend', () => ({ useBackend: () => ({}) }));
    // Stub window.omni so production useShareWs sees a working bridge. The
    // sendShareMessage echoes the request `id` back so the dispatcher pairs
    // each call to its schema validator.
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(async (msg: { id: string; type: string }) => {
        if (msg.type === 'identity.show') return { id: msg.id, ...IDENTITY_RESULT };
        if (msg.type === 'workspace.listPublishables') return { id: msg.id, ...LIST_RESULT };
        throw new Error('unexpected sendShareMessage in sidecar-restore test: ' + msg.type);
      }),
      onShareEvent: vi.fn(() => () => {}),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.doUnmock('../../../../hooks/use-backend');
  });

  it('selecting an overlay whose sidecar matches the running identity flips the header to "Update Overlay" and renders the LinkedArtifactBanner', async () => {
    // Dynamic import AFTER vi.doMock so the dialog module picks up the
    // mocked `use-backend` (`vi.resetModules` in beforeEach guarantees
    // a fresh import graph per test).
    const { UploadDialog } = await import('../index');

    render(<UploadDialog open={true} onOpenChange={() => {}} prefilledPath={null} />);

    // Wait for the dialog chrome to mount. Source picker renders
    // synchronously once the dialog's content slot is visible — but the
    // overlay list inside it is gated on `state.selectedKind` (the user
    // must click a type card first; until then the picker shows
    // `source-picker-no-kind`). So our wait condition is the type-card,
    // which is unconditional once the dialog content renders.
    const bundleCard = await screen.findByTestId('type-card-overlay');

    // Sanity check the before-state: header reads "Publish Overlay"
    // (default create mode + overlay subject per INV-7.0.4). Pinning the
    // before-state is what lets the after-state assertion prove the
    // header actually swapped in response to the row click.
    expect(screen.getByText('Publish Overlay')).toBeInTheDocument();

    // Click the Bundle (overlay) card — flips selectedKind to 'overlay',
    // which un-gates the filtered list of overlay rows. Entries are
    // already loaded (useWorkspaceList fetched them on mount), so the
    // row appears in the next render.
    fireEvent.click(bundleCard);

    // Wait for the overlay row to render. This also implicitly waits for
    // the workspace.listPublishables RPC to resolve — without entries the
    // picker would render `source-picker-empty` instead of the row.
    const overlayRow = await screen.findByTestId(`source-row-${OVERLAY_ENTRY.workspace_path}`);

    // Click the row — dispatches SELECT_ITEM which calls
    // detectMode(entry, currentPubkey). With the identity bootstrap
    // having already resolved (also pre-flighted by the useEffect that
    // fired at mount), currentPubkey === SIDECAR.author_pubkey_hex →
    // mode='update'.
    fireEvent.click(overlayRow);

    // Assertion 1 — header copy flipped to "Update Overlay" (INV-7.5.1).
    // `waitFor` covers the case where the identity.show response races
    // the row click: even if mode was momentarily 'create' (because the
    // pubkey hadn't landed yet), the SET_CURRENT_PUBKEY dispatch
    // re-derives mode from `state.selected` and re-renders the header.
    await waitFor(() => {
      expect(screen.getByText('Update Overlay')).toBeInTheDocument();
    });
    // Old "Publish Overlay" copy is gone — pins that the header swapped
    // rather than rendering both labels in sequence.
    expect(screen.queryByText('Publish Overlay')).toBeNull();

    // Assertion 2 — LinkedArtifactBanner renders with the spec-required
    // copy (INV-7.6.2: "Linked to existing artifact — {prefix}… · last
    // published v{X.Y.Z} on {date}. This upload will be an update.").
    // The banner only renders when `state.selected.sidecar` is set AND
    // `state.mode === 'update'` (per source-picker.tsx); the explicit
    // assertion on its testid pins both conditions.
    const banner = await screen.findByTestId('linked-artifact-banner');
    expect(banner).toHaveTextContent('Linked to existing artifact');
    // Artifact id is rendered as `slice(0, 12)…` per
    // source-picker-banners.tsx; "ov_01J8XKZ9F" is the first 12 chars
    // of the fixture's artifact_id (`ov_01J8XKZ9F9ABCDEF`).
    expect(banner).toHaveTextContent('ov_01J8XKZ9F…');
    expect(banner).toHaveTextContent('v1.3.0');
    expect(banner).toHaveTextContent('2026-04-18');
    expect(banner).toHaveTextContent('This upload will be an update.');

    // Assertion 3 — PubkeyMismatchBanner is NOT shown. The author key
    // matches the running identity, so the amber "different identity"
    // warning has no business here. Pinning the negative case prevents
    // a future regression where both banners might co-render (visual
    // confusion + ambiguous "is this an update or not?" UX).
    expect(screen.queryByTestId('pubkey-mismatch-banner')).toBeNull();
  });
});
