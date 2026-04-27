import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IdentityBackupDialog } from '../identity-backup-dialog';
import { installShareIpcSpy } from '../../../test-utils/mock-share-ws';

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

  it('on WS success calls onSuccess with saveBackup path; parent owns the close (no auto onOpenChange(false))', async () => {
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
    // Contract: dialog must NOT auto-close after onSuccess. Parent is
    // responsible for closing (e.g., upload-dialog/index.tsx closes via
    // resolveBackupGate → dispatch BACKUP_GATE open=false). Calling both
    // onSuccess AND onOpenChange(false) caused a duplicate publish — see
    // identity-backup-dialog.tsx for the full rationale.
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
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

describe('IdentityBackupDialog — blocking-before-upload mode', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('renders the amber callout when mode=blocking-before-upload', () => {
    render(
      <IdentityBackupDialog
        open
        mode="blocking-before-upload"
        onOpenChange={vi.fn()}
        onSuccess={vi.fn()}
      />,
    );
    expect(screen.getByText(/why this blocks your first upload/i)).toBeInTheDocument();
  });

  it('renders the [Skip and publish anyway] link when onSkip is provided', () => {
    render(
      <IdentityBackupDialog
        open
        mode="blocking-before-upload"
        onSkip={vi.fn()}
        onOpenChange={vi.fn()}
        onSuccess={vi.fn()}
      />,
    );
    expect(screen.getByRole('button', { name: /skip and publish anyway/i })).toBeInTheDocument();
  });
});

describe('IdentityBackupDialog — import mode', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('renders file picker + passphrase + overwrite checkbox', () => {
    render(
      <IdentityBackupDialog open mode="import" onOpenChange={vi.fn()} onSuccess={vi.fn()} />,
    );
    expect(screen.getByText(/import existing identity/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/overwrite existing identity/i)).toBeInTheDocument();
  });

  it('disables overwrite checkbox when lockOverwrite=true', () => {
    render(
      <IdentityBackupDialog
        open
        mode="import"
        lockOverwrite
        onOpenChange={vi.fn()}
        onSuccess={vi.fn()}
      />,
    );
    expect(screen.getByLabelText(/overwrite existing identity/i)).toBeDisabled();
    expect(screen.getByLabelText(/overwrite existing identity/i)).toBeChecked();
  });
});

describe('IdentityBackupDialog — markBackedUp wire shape (per feedback_wire_shape_tests.md)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('dispatches identity.markBackedUp with { path, timestamp } after save', async () => {
    const { sendSpy } = installShareIpcSpy();

    // First call: identity.backup → returns backup result
    sendSpy.mockResolvedValueOnce({
      id: 'ignored',
      type: 'identity.backupResult',
      params: { encrypted_bytes_b64: 'AAAA' },
    });
    // Second call: identity.markBackedUp → returns ok
    sendSpy.mockResolvedValueOnce({
      id: 'ignored',
      type: 'identity.markBackedUpResult',
      params: { ok: true },
    });

    const onSuccess = vi.fn();
    const saveBackup = async () => 'C:/Users/me/identity.omniid';
    render(
      <IdentityBackupDialog
        open
        mode="blocking-before-upload"
        onOpenChange={vi.fn()}
        onSuccess={onSuccess}
        saveBackup={saveBackup}
      />,
    );
    await userEvent.type(screen.getByLabelText(/^passphrase$/i), 'correct horse battery!');
    await userEvent.type(screen.getByLabelText(/confirm passphrase/i), 'correct horse battery!');
    await userEvent.click(screen.getByRole('button', { name: /save backup/i }));

    await waitFor(() => expect(onSuccess).toHaveBeenCalledWith('C:/Users/me/identity.omniid'));

    const calls = sendSpy.mock.calls.map((c) => c[0] as Record<string, unknown>);
    const markCall = calls.find((c) => c.type === 'identity.markBackedUp') as
      | { params?: { path?: unknown; timestamp?: unknown } }
      | undefined;
    expect(markCall).toBeDefined();
    expect(markCall!.params!.path).toBe('C:/Users/me/identity.omniid');
    expect(typeof markCall!.params!.timestamp).toBe('number');
    expect(
      Math.abs((markCall!.params!.timestamp as number) - Math.floor(Date.now() / 1000)),
    ).toBeLessThan(5);
  });
});
