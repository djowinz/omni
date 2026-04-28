import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { installShareIpcSpy } from '../../../test-utils/mock-share-ws';
import { IdentityContextProvider } from '../../../lib/identity-context';
import { RotateConfirmDialog } from '../rotate-confirm-dialog';
import { toast } from '../../../lib/toast';

vi.mock('../../../lib/toast', () => ({
  toast: {
    warning: vi.fn(),
    success: vi.fn(),
    info: vi.fn(),
    error: vi.fn(),
  },
}));

const POPULATED = {
  id: 'x',
  type: 'identity.showResult',
  params: {
    pubkey_hex: 'a'.repeat(64),
    fingerprint_hex: 'aaa',
    fingerprint_words: ['a', 'b', 'c'],
    fingerprint_emoji: ['1', '2', '3', '4', '5', '6'],
    created_at: 0,
    display_name: 'starfire',
    backed_up: true,
    last_backed_up_at: 0,
    last_rotated_at: null,
    last_backup_path: null,
  },
};

describe('RotateConfirmDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it('body covers the three required points (carry-over, future-only, backup-invalid)', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    render(
      <IdentityContextProvider>
        <RotateConfirmDialog open onOpenChange={vi.fn()} onBackupNow={vi.fn()} />
      </IdentityContextProvider>,
    );
    await waitFor(() => expect(screen.getByText(/generates a new keypair/i)).toBeInTheDocument());
    expect(screen.getByText(/display name will carry over/i)).toBeInTheDocument();
    expect(screen.getByText(/no longer decrypt the new key/i)).toBeInTheDocument();
  });

  it('Rotate dispatches identity.rotate, refreshes context, and fires the warning toast', async () => {
    const { sendSpy } = installShareIpcSpy({ defaultResponse: POPULATED });
    sendSpy.mockResolvedValueOnce(POPULATED);
    sendSpy.mockResolvedValueOnce({
      id: 'x',
      type: 'identity.rotateResult',
      params: { pubkey_hex: 'b'.repeat(64), fingerprint_hex: 'bbb' },
    });
    sendSpy.mockResolvedValueOnce(POPULATED);

    const onBackupNow = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <IdentityContextProvider>
        <RotateConfirmDialog open onOpenChange={onOpenChange} onBackupNow={onBackupNow} />
      </IdentityContextProvider>,
    );

    await userEvent.click(screen.getByRole('button', { name: /^rotate$/i }));
    await waitFor(() => {
      const rotate = sendSpy.mock.calls.find((c: any) => c[0]?.type === 'identity.rotate');
      expect(rotate).toBeDefined();
    });
    expect(toast.warning).toHaveBeenCalledWith(
      expect.stringMatching(/identity rotated/i),
      expect.objectContaining({ action: expect.any(Object) }),
    );
  });
});
