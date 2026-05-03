/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * Reopen-prefill regression test (spec §7.5, INV-7.6.1).
 *
 * Pins the contract that closing and reopening the upload dialog with the
 * same `prefilledPath` prop re-runs the workspace.listPublishables lookup
 * and re-enters Step 2 with the entry selected — instead of leaving the
 * user on Step 1 with a blank source picker.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';

const PUBKEY_HEX = 'abcd' + '0'.repeat(60);

const ENTRY = {
  kind: 'overlay' as const,
  workspace_path: 'overlays/marathon-hud',
  name: 'Marathon HUD',
  widget_count: 4,
  modified_at: '2026-04-10T15:30:00Z',
  has_preview: false,
  sidecar: {
    artifact_id: 'ov_01J8XKZ',
    author_pubkey_hex: PUBKEY_HEX,
    version: '1.0.0',
    last_published_at: '2026-04-18T00:00:00Z',
    description: 'desc',
    tags: ['marathon'],
    license: 'MIT',
  },
};

describe('UploadDialog — prefill effect re-fires on reopen', () => {
  let listCalls: number;

  beforeEach(() => {
    listCalls = 0;
    vi.resetModules();
    vi.doMock('../../../../hooks/use-backend', () => ({ useBackend: () => ({}) }));
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(async (msg: { id: string; type: string }) => {
        if (msg.type === 'identity.show') {
          return {
            id: msg.id,
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
        }
        if (msg.type === 'workspace.listPublishables') {
          listCalls++;
          return {
            id: msg.id,
            type: 'workspace.listPublishablesResult',
            params: { entries: [ENTRY] },
          };
        }
        throw new Error('unexpected sendShareMessage: ' + msg.type);
      }),
      onShareEvent: vi.fn(() => () => {}),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it('re-runs workspace.listPublishables when the dialog is closed and reopened', async () => {
    const { UploadDialog } = await import('../index');

    function Harness({ open }: { open: boolean }) {
      return (
        <UploadDialog
          open={open}
          onOpenChange={() => {}}
          prefilledPath="overlays/marathon-hud"
        />
      );
    }

    // On the FIRST open the dialog issues TWO listPublishables calls:
    //   1. SourcePicker mounts (step='select') and useWorkspaceList fetches.
    //   2. The prefilled-path effect on the upload machine independently
    //      fetches so it can resolve the prefill to a `PublishablesEntry`
    //      and dispatch SELECT_KIND/SELECT_ITEM/NEXT.
    // Once NEXT lands, step flips to 'details' and SourcePicker unmounts.
    // The two callers are independent (each owns its own RPC); pinning the
    // exact count protects against either side regressing.
    const { rerender } = render(<Harness open={true} />);
    await waitFor(() => expect(listCalls).toBe(2));
    // Land on Step 2 — the Name input is the canonical Review-step marker.
    await waitFor(() => expect(screen.getByTestId('upload-name')).toBeInTheDocument());

    // Close — DialogContent unmounts (Radix Portal); UploadDialog's reset
    // effect fires `actions.reset()` so state goes back to INITIAL_STATE
    // (step='select', selected=null, prefilledHandledRef nulled).
    await act(async () => {
      rerender(<Harness open={false} />);
    });

    // Reopen with the SAME prefilledPath — DialogContent re-mounts so
    // SourcePicker fetches once more (call #3). The prefilled-path effect
    // must ALSO re-fire (call #4) and re-advance the dialog into Step 2.
    // Without the fix, only #3 happens — the dialog stays stuck on Step 1
    // with a blank source picker, which is the user-reported bug this
    // regression test pins.
    await act(async () => {
      rerender(<Harness open={true} />);
    });

    await waitFor(() => expect(listCalls).toBe(4));
    await waitFor(() => expect(screen.getByTestId('upload-name')).toBeInTheDocument());
  });
});
