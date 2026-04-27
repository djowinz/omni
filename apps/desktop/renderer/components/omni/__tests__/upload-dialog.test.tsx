/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * UploadDialog smoke tests — minimal coverage that the new directory
 * (`./upload-dialog/index.tsx`) mounts and routes correctly. The legacy
 * "full source → review → validate → publish" tests were superseded by
 * per-step tests under `./upload-dialog/__tests__/` (one per step, one for
 * the state machine, one for the footer chrome) when T-A2.1 (OWI-41)
 * deleted the monolithic `upload-dialog.tsx` and the four-step linear
 * test was no longer relevant.
 *
 * Mocks: `window.omni` is stubbed (sendShareMessage + onShareEvent) so the
 * production `useShareWs` hook (which is the integration point our dialog
 * actually uses) sees a working bridge. Unlike the legacy test we do NOT
 * vi.doMock `use-share-ws` — that pattern returns a fresh object on every
 * render, which makes the dialog's `useEffect([ws])` fire on every render
 * and OOM the test process.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';

const VOCAB_RESULT = {
  type: 'config.vocabResult',
  params: { tags: ['dark', 'minimal'], version: 1 },
};

const IDENTITY_RESULT = {
  type: 'identity.showResult',
  params: {
    pubkey_hex: 'cc'.repeat(32),
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
  params: {
    entries: [
      {
        kind: 'overlay',
        workspace_path: 'overlays/Marathon',
        name: 'Marathon',
        widget_count: 4,
        modified_at: '2026-04-21T00:00:00Z',
        has_preview: false,
        sidecar: null,
      },
    ],
  },
};

describe('UploadDialog (new dialog directory)', () => {
  beforeEach(() => {
    vi.resetModules();
    // IdentityBackupDialog (mounted by UploadDialog) calls useBackend().
    vi.doMock('../../../hooks/use-backend', () => ({ useBackend: () => ({}) }));
    // Stub the `window.omni` bridge that production useShareWs reads.
    // sendShareMessage echoes the request `id` back in the response so the
    // dispatcher pairs the right call to the right schema validator.
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(async (msg: { id: string; type: string }) => {
        if (msg.type === 'config.vocab') return { id: msg.id, ...VOCAB_RESULT };
        if (msg.type === 'identity.show') return { id: msg.id, ...IDENTITY_RESULT };
        if (msg.type === 'workspace.listPublishables') return { id: msg.id, ...LIST_RESULT };
        throw new Error('unexpected sendShareMessage in upload-dialog test: ' + msg.type);
      }),
      onShareEvent: vi.fn(() => () => {}),
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('mounts the new dialog and renders the source picker on Step 1', async () => {
    const { UploadDialog } = await import('../upload-dialog');
    render(<UploadDialog open onOpenChange={() => {}} />);

    await waitFor(() => expect(screen.getByTestId('upload-dialog-content')).toBeInTheDocument());
    expect(screen.getByTestId('source-picker')).toBeInTheDocument();
  });

  it('does not render the dialog when open=false', async () => {
    const { UploadDialog } = await import('../upload-dialog');
    render(<UploadDialog open={false} onOpenChange={() => {}} />);

    expect(screen.queryByTestId('upload-dialog-content')).toBeNull();
  });
});
