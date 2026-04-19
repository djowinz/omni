import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IdentityBackupDialog } from '../identity-backup-dialog';

// The dialog references `useBackend()` for future migration of `identity.backup`
// into BackendApi. Today it calls `window.omni.sendShareMessage` directly
// (share:ws-message IPC channel, where the host's handle_identity_backup is
// routed), so we mock the hook to avoid constructing a real BackendApi (which
// would try to open a WebSocket on mount) and stub the global for the actual
// WS call.
vi.mock('@/hooks/use-backend', () => ({
  useBackend: () => ({}),
}));

const VALID_PASSPHRASE = 'Abcdefghijk1'; // 12 chars, 3 classes → medium

function stubOmni(sendShareMessage: ReturnType<typeof vi.fn>) {
  vi.stubGlobal('omni', { sendShareMessage });
}

describe('IdentityBackupDialog', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('renders nothing when open={false}', () => {
    render(
      <IdentityBackupDialog
        open={false}
        onOpenChange={vi.fn()}
        onSuccess={vi.fn()}
        mode="first-publish"
      />,
    );
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('disables submit until passphrase+confirm match and length ≥ 12', async () => {
    const user = userEvent.setup();
    render(
      <IdentityBackupDialog open onOpenChange={vi.fn()} onSuccess={vi.fn()} mode="first-publish" />,
    );

    const submit = screen.getByRole('button', { name: /save backup/i });
    expect(submit).toBeDisabled();

    const passphraseInput = screen.getByLabelText('Passphrase');
    const confirmInput = screen.getByLabelText('Confirm passphrase');

    // 11 chars — still disabled
    await user.type(passphraseInput, 'Abcdefghij1');
    await user.type(confirmInput, 'Abcdefghij1');
    expect(submit).toBeDisabled();

    // Clear and retype 12 matching chars meeting medium strength
    await user.clear(passphraseInput);
    await user.clear(confirmInput);
    await user.type(passphraseInput, VALID_PASSPHRASE);
    await user.type(confirmInput, VALID_PASSPHRASE);
    expect(passphraseInput).toHaveValue(VALID_PASSPHRASE);
    expect(confirmInput).toHaveValue(VALID_PASSPHRASE);

    await waitFor(() => expect(submit).toBeEnabled());
  });

  it('on WS success calls onSuccess with saveBackup path and closes dialog', async () => {
    // Host returns `{ id, type: 'identity.backupResult', params: { encrypted_bytes_b64 } }`
    // over the share:ws-message channel. The dialog must reach into params,
    // not the response root, and route via sendShareMessage not sendMessage.
    const sendShareMessage = vi.fn().mockResolvedValue({
      id: 'some-uuid',
      type: 'identity.backupResult',
      params: { encrypted_bytes_b64: 'YWJj' }, // "abc"
    });
    stubOmni(sendShareMessage);

    const saveBackup = vi.fn(async (_bytes: Uint8Array) => '/fake/path.omniid');
    const onSuccess = vi.fn();
    const onOpenChange = vi.fn();
    const user = userEvent.setup();

    render(
      <IdentityBackupDialog
        open
        onOpenChange={onOpenChange}
        onSuccess={onSuccess}
        mode="first-publish"
        saveBackup={saveBackup}
      />,
    );

    await user.type(screen.getByLabelText('Passphrase'), VALID_PASSPHRASE);
    await user.type(screen.getByLabelText('Confirm passphrase'), VALID_PASSPHRASE);

    const submit = screen.getByRole('button', { name: /save backup/i });
    await waitFor(() => expect(submit).toBeEnabled());
    await user.click(submit);

    await waitFor(() => expect(onSuccess).toHaveBeenCalledWith('/fake/path.omniid'));
    // Wire-shape assertion: frame routed to share channel, params nested.
    expect(sendShareMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'identity.backup',
        params: expect.objectContaining({ passphrase: VALID_PASSPHRASE }),
      }),
    );
    // A fresh request id should be attached.
    const callArg = sendShareMessage.mock.calls[0][0] as { id?: unknown };
    expect(typeof callArg.id).toBe('string');
    expect((callArg.id as string).length).toBeGreaterThan(0);
    expect(saveBackup).toHaveBeenCalledTimes(1);
    const bytesArg = saveBackup.mock.calls[0][0];
    expect(bytesArg).toBeInstanceOf(Uint8Array);
    expect(Array.from(bytesArg)).toEqual([0x61, 0x62, 0x63]);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('on host error envelope surfaces message and does NOT leak detail', async () => {
    const sentinelDetail = 'DETAIL-SENTINEL-42';
    const userMessage = 'Backup could not be saved.';
    // Host returns a structured error frame over the share channel; the dialog
    // must read frame.error?.message, not throw-convert the whole response.
    const sendShareMessage = vi.fn().mockResolvedValue({
      id: 'some-uuid',
      type: 'error',
      error: {
        code: 'BACKUP_FAILED',
        kind: 'HostLocal',
        detail: sentinelDetail,
        message: userMessage,
      },
    });
    stubOmni(sendShareMessage);

    // Suppress the intentional console.error from the dialog's catch block.
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const user = userEvent.setup();
    render(
      <IdentityBackupDialog
        open
        onOpenChange={vi.fn()}
        onSuccess={vi.fn()}
        mode="first-publish"
        saveBackup={vi.fn()}
      />,
    );

    await user.type(screen.getByLabelText('Passphrase'), VALID_PASSPHRASE);
    await user.type(screen.getByLabelText('Confirm passphrase'), VALID_PASSPHRASE);

    const submit = screen.getByRole('button', { name: /save backup/i });
    await waitFor(() => expect(submit).toBeEnabled());
    await user.click(submit);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(userMessage);
    // D-004-J / invariant #20: detail must never be rendered.
    expect(alert.textContent ?? '').not.toContain(sentinelDetail);
    expect(document.body.textContent ?? '').not.toContain(sentinelDetail);

    errSpy.mockRestore();
  });
});
