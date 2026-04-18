/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

const PACK_RESULT = {
  id: 'r-pack',
  type: 'upload.packResult',
  params: {
    content_hash: 'deadbeef'.repeat(8),
    compressed_size: 10240,
    uncompressed_size: 40960,
    manifest: { name: 'Demo' },
    sanitize_report: {},
  },
};

const PUBLISH_RESULT = {
  id: 'r-publish',
  type: 'upload.publishResult',
  params: {
    artifact_id: 'art-new-1',
    content_hash: 'deadbeef'.repeat(8),
    status: 'created' as const,
    worker_url: 'https://themes.omni.prod',
  },
};

const VOCAB_RESULT = {
  id: 'r-vocab',
  type: 'config.vocabResult',
  params: { tags: ['dark', 'minimal'], version: 1 },
};

const IDENTITY_BACKED_UP = {
  id: 'r-identity',
  type: 'identity.showResult',
  params: {
    pubkey_hex: 'cc'.repeat(32),
    fingerprint_hex: '',
    fingerprint_emoji: [],
    fingerprint_words: [],
    created_at: 0,
    backed_up: true,
  },
};

const IDENTITY_UNBACKED = {
  id: 'r-identity',
  type: 'identity.showResult',
  params: {
    pubkey_hex: 'cc'.repeat(32),
    fingerprint_hex: '',
    fingerprint_emoji: [],
    fingerprint_words: [],
    created_at: 0,
    backed_up: false,
  },
};

describe('UploadDialog', () => {
  beforeEach(() => {
    vi.resetModules();
    // The IdentityBackupDialog (mounted by UploadDialog) calls useBackend().
    // Mock it to avoid constructing a real BackendApi.
    vi.doMock('../../../hooks/use-backend', () => ({
      useBackend: () => ({}),
    }));
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('with prefilled source skips step 1 and lands on Review', async () => {
    const send = vi.fn(async (type: string) => {
      if (type === 'config.vocab') return VOCAB_RESULT;
      throw new Error('unexpected send: ' + type);
    });
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    vi.doMock('../../../hooks/use-share-ws', () => ({
      useShareWs: () => ({ send, subscribe: vi.fn(() => () => {}) }),
    }));
    const { UploadDialog } = await import('../upload-dialog');

    render(
      <UploadDialog open onOpenChange={() => {}} sourcePath="overlays/Marathon" mode="publish" />,
    );

    await waitFor(() => expect(screen.getByTestId('upload-step-review')).toBeInTheDocument());
    // Source picker not visible
    expect(screen.queryByTestId('upload-step-source')).toBeNull();
  });

  it('without a source shows the workspace picker first', async () => {
    const send = vi.fn(async () => VOCAB_RESULT);
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(async () => ({
        type: 'file.list',
        overlays: ['Default', 'Marathon'],
        themes: [],
      })),
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    vi.doMock('../../../hooks/use-share-ws', () => ({
      useShareWs: () => ({ send, subscribe: vi.fn(() => () => {}) }),
    }));
    const { UploadDialog } = await import('../upload-dialog');

    render(<UploadDialog open onOpenChange={() => {}} sourcePath={null} mode="publish" />);

    await waitFor(() => expect(screen.getByTestId('upload-step-source')).toBeInTheDocument());
    expect(screen.getByText('Marathon')).toBeInTheDocument();
  });

  it('full flow: source → review → validate → publish → success', async () => {
    const user = userEvent.setup();
    const send = vi.fn(async (type: string) => {
      if (type === 'config.vocab') return VOCAB_RESULT;
      if (type === 'identity.show') return IDENTITY_BACKED_UP;
      if (type === 'upload.pack') return PACK_RESULT;
      if (type === 'upload.publish') return PUBLISH_RESULT;
      throw new Error('unexpected send: ' + type);
    });
    const subscribe = vi.fn(() => () => {});
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    vi.doMock('../../../hooks/use-share-ws', () => ({
      useShareWs: () => ({ send, subscribe }),
    }));
    const { UploadDialog } = await import('../upload-dialog');

    render(
      <UploadDialog open onOpenChange={() => {}} sourcePath="overlays/Marathon" mode="publish" />,
    );

    // Step 2 — review; fill name
    await waitFor(() => screen.getByTestId('upload-name'));
    await user.type(screen.getByTestId('upload-name'), 'Marathon');
    await user.click(screen.getByTestId('upload-next-button'));

    // Step 3 — validate
    await waitFor(() => screen.getByTestId('upload-validate-result'));
    await user.click(screen.getByTestId('upload-next-button'));

    // Step 4 — publish result
    await waitFor(() => screen.getByTestId('upload-publish-success'));
    expect(screen.getByText(/art-new-1/)).toBeInTheDocument();
  });

  it('surfaces SANITIZE_FAILED error on validate step without advancing', async () => {
    const user = userEvent.setup();
    const send = vi.fn(async (type: string) => {
      if (type === 'config.vocab') return VOCAB_RESULT;
      if (type === 'upload.pack') {
        throw {
          code: 'SANITIZE_FAILED',
          kind: 'Unsafe',
          detail: 'style.css:42 @import rule disallowed',
          message: 'Your bundle contains disallowed CSS.',
        };
      }
      throw new Error('unexpected: ' + type);
    });
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    vi.doMock('../../../hooks/use-share-ws', () => ({
      useShareWs: () => ({ send, subscribe: vi.fn(() => () => {}) }),
    }));
    const { UploadDialog } = await import('../upload-dialog');

    render(
      <UploadDialog open onOpenChange={() => {}} sourcePath="overlays/Marathon" mode="publish" />,
    );

    await user.type(await screen.findByTestId('upload-name'), 'Marathon');
    await user.click(screen.getByTestId('upload-next-button'));

    await waitFor(() => screen.getByTestId('upload-validate-error'));
    expect(screen.getByText(/disallowed CSS/)).toBeInTheDocument();
    expect(screen.getByText(/@import rule disallowed/)).toBeInTheDocument();
  });

  it('opens IdentityBackupDialog when publishing with backed_up=false', async () => {
    const user = userEvent.setup();
    const send = vi.fn(async (type: string) => {
      if (type === 'config.vocab') return VOCAB_RESULT;
      if (type === 'identity.show') return IDENTITY_UNBACKED;
      if (type === 'upload.pack') return PACK_RESULT;
      if (type === 'upload.publish') return PUBLISH_RESULT;
      throw new Error('unexpected: ' + type);
    });
    vi.stubGlobal('omni', {
      sendMessage: vi.fn(),
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    vi.doMock('../../../hooks/use-share-ws', () => ({
      useShareWs: () => ({ send, subscribe: vi.fn(() => () => {}) }),
    }));
    const { UploadDialog } = await import('../upload-dialog');

    render(
      <UploadDialog
        open
        onOpenChange={() => {}}
        sourcePath="overlays/Marathon"
        mode="publish"
        // Pass a mock saveBackup so the IdentityBackupDialog doesn't try to hit
        // the (not-yet-wired) main-process dialog:saveIdentityBackup IPC.
        backupSaveBackup={vi.fn(async () => '/fake/path.omniid')}
      />,
    );

    // Step 2 — review; advance to validate
    await user.type(await screen.findByTestId('upload-name'), 'Marathon');
    await user.click(screen.getByTestId('upload-next-button'));

    // Step 3 — validate; advance to publish (triggers gate)
    await waitFor(() => screen.getByTestId('upload-validate-result'));
    await user.click(screen.getByTestId('upload-next-button'));

    // Gate opens IdentityBackupDialog — title matches the 'first-publish' MODE_COPY
    await waitFor(() => expect(screen.getByText(/Back up your identity/i)).toBeInTheDocument());
  });
});
