import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { installShareIpcSpy } from '../../../test-utils/mock-share-ws';
import { IdentityContextProvider } from '../../../lib/identity-context';
import { IdentitySection } from '../identity-section';

const PUBKEY = '3a7c9f2b' + '0'.repeat(56);

const POPULATED = {
  id: 'x',
  type: 'identity.showResult',
  params: {
    pubkey_hex: PUBKEY,
    fingerprint_hex: '3a7c9f2ba8b1',
    fingerprint_words: ['a', 'b', 'c'],
    fingerprint_emoji: ['🦊', '🌲', '🚀', '🧊', '🌙', '⚡'],
    created_at: 0,
    display_name: 'starfire',
    backed_up: true,
    last_backed_up_at: 1700000000,
    last_rotated_at: null,
    last_backup_path: null,
  },
};

const onBackup = vi.fn();
const onImport = vi.fn();
const onRotate = vi.fn();
const onCopyPubkey = vi.fn();

const wrap = () =>
  render(
    <IdentityContextProvider>
      <IdentitySection
        onBackup={onBackup}
        onImport={onImport}
        onRotate={onRotate}
        onCopyPubkey={onCopyPubkey}
      />
    </IdentityContextProvider>,
  );

describe('IdentitySection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it('renders #identity-section anchor for chip-click scrollIntoView', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => screen.getByText('starfire'));
    expect(document.getElementById('identity-section')).not.toBeNull();
  });

  it('renders the 8-hex fingerprint slice (never the 12-hex value)', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => screen.getByText(/starfire/));
    expect(screen.queryByText(/3a7c9f2ba8b1/)).not.toBeInTheDocument();
    expect(screen.getByText(/3a7c9f2b/)).toBeInTheDocument();
  });

  it('Back up button triggers onBackup; Import triggers onImport', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => screen.getByText('starfire'));
    await userEvent.click(screen.getByRole('button', { name: /^back up$/i }));
    await userEvent.click(screen.getByRole('button', { name: /^import$/i }));
    expect(onBackup).toHaveBeenCalledTimes(1);
    expect(onImport).toHaveBeenCalledTimes(1);
  });

  it('shows green "Backed up" with relative time when backed_up', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => expect(screen.getByText(/backed up/i)).toBeInTheDocument());
  });

  it('shows amber "Not backed up" when !backed_up', async () => {
    installShareIpcSpy({
      defaultResponse: {
        ...POPULATED,
        params: { ...POPULATED.params, backed_up: false, last_backed_up_at: null },
      },
    });
    wrap();
    await waitFor(() => expect(screen.getByText(/not backed up/i)).toBeInTheDocument());
  });
});
