/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * Update-mode prefill regression test (spec INV-7.5.3).
 *
 * Pins the contract that when the upload dialog detects update mode for a
 * selected entry (sidecar present + author_pubkey_hex matches running
 * identity), Step 2's react-hook-form fields are seeded from the sidecar's
 * cached manifest snapshot — Name, Description, Tags, License — rather
 * than left at DEFAULT_FORM blanks.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';

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
    version: '1.3.0',
    last_published_at: '2026-04-18T00:00:00Z',
    description: 'splits + pace + heart rate',
    tags: ['marathon', 'running', 'fitness'],
    license: 'Apache-2.0',
  },
};

describe('UploadDialog — update-mode form prefill (INV-7.5.3)', () => {
  beforeEach(() => {
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
          return {
            id: msg.id,
            type: 'workspace.listPublishablesResult',
            params: { entries: [ENTRY] },
          };
        }
        if (msg.type === 'config.vocab') {
          // Vocab supplies the universe of tag pills the ReviewTagBadges
          // component renders. The test's sidecar tags must appear in this
          // list for the corresponding pill DOM nodes (and their text) to
          // exist — selection state from `form.tags` only toggles the
          // pill's `data-selected` attribute, not its presence.
          return {
            id: msg.id,
            type: 'config.vocabResult',
            params: { tags: ['marathon', 'running', 'fitness'], version: 1 },
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

  it('seeds Step 2 form fields from the sidecar when update mode resolves', async () => {
    const { UploadDialog } = await import('../index');

    render(
      <UploadDialog
        open={true}
        onOpenChange={() => {}}
        prefilledPath="overlays/marathon-hud"
      />,
    );

    // Wait until Step 2 mounts (Name input is the canonical Review-step marker)
    // AND the form prefill effect has populated it from the sidecar.
    await waitFor(() => {
      const name = screen.getByTestId('upload-name') as HTMLInputElement;
      expect(name.value).toBe('Marathon HUD');
    });

    const desc = screen.getByTestId('upload-description') as HTMLTextAreaElement;
    expect(desc.value).toBe('splits + pace + heart rate');

    // Tags rendered as Badge pills via ReviewTagBadges. The vocab fixture
    // above exposes the same three tags as pills; the prefill effect's
    // form.reset({ tags: [...] }) flips each matching pill's
    // `data-selected` attribute to "true". Asserting that attribute (not
    // just pill presence) is what proves the prefill effect ran — the
    // pills exist regardless of selection state.
    expect(screen.getByText('marathon')).toBeInTheDocument();
    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('fitness')).toBeInTheDocument();
    expect(screen.getByTestId('review-tag-badge-marathon')).toHaveAttribute(
      'data-selected',
      'true',
    );
    expect(screen.getByTestId('review-tag-badge-running')).toHaveAttribute(
      'data-selected',
      'true',
    );
    expect(screen.getByTestId('review-tag-badge-fitness')).toHaveAttribute(
      'data-selected',
      'true',
    );
  });
});
