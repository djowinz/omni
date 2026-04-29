import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { installShareIpcSpy } from '../../../test-utils/mock-share-ws';
import { IdentityContextProvider } from '../../../lib/identity-context';
import { IdentityChip } from '../identity-chip';

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

const wrap = (onNavigate = vi.fn()) =>
  render(
    <IdentityContextProvider>
      <IdentityChip onNavigateToSettings={onNavigate} />
    </IdentityContextProvider>,
  );

describe('IdentityChip', () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders <display_name>#<8-hex> and aria-labelled green dot when backed_up', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => expect(screen.getByText(/starfire/)).toBeInTheDocument());
    expect(screen.getByText(/3a7c9f2b/)).toBeInTheDocument();
    expect(screen.getByLabelText(/backed up/i)).toBeInTheDocument();
  });

  it('renders amber dot when !backed_up but has backup history', async () => {
    installShareIpcSpy({
      defaultResponse: {
        ...POPULATED,
        params: { ...POPULATED.params, backed_up: false, last_backed_up_at: 1700000000 },
      },
    });
    wrap();
    await waitFor(() => expect(screen.getByLabelText(/not backed up/i)).toBeInTheDocument());
  });

  it('renders amber dot when display_name is set but !backed_up (user has interacted)', async () => {
    installShareIpcSpy({
      defaultResponse: {
        ...POPULATED,
        params: {
          ...POPULATED.params,
          display_name: 'starfire',
          backed_up: false,
          last_backed_up_at: null,
          last_rotated_at: null,
        },
      },
    });
    wrap();
    await waitFor(() => expect(screen.getByLabelText(/not backed up/i)).toBeInTheDocument());
  });

  it('renders neutral dot ONLY when fresh install (no name, no rotation, no backup)', async () => {
    installShareIpcSpy({
      defaultResponse: {
        ...POPULATED,
        params: {
          ...POPULATED.params,
          display_name: null,
          backed_up: false,
          last_backed_up_at: null,
          last_rotated_at: null,
        },
      },
    });
    wrap();
    await waitFor(() => expect(screen.getByLabelText(/no backup history/i)).toBeInTheDocument());
  });

  it('omits emoji and BIP-39 words (per mockup-decisions §1.3)', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => screen.getByText(/starfire/));
    expect(screen.queryByText('🦊')).not.toBeInTheDocument();
  });

  it('hex display is 8 characters, never 12 (per mockup-decisions §1.8)', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => screen.getByText(/starfire/));
    expect(screen.queryByText(/3a7c9f2ba8b1/)).not.toBeInTheDocument();
  });

  it('hover reveals tooltip with display_name + 8-hex slice + backup status + click hint', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap();
    await waitFor(() => screen.getByText(/starfire/));
    const button = screen.getByRole('button', { name: /your identity/i });
    await userEvent.hover(button);
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip).toHaveTextContent('Display name');
    expect(tooltip).toHaveTextContent('starfire');
    expect(tooltip).toHaveTextContent('Fingerprint');
    expect(tooltip).toHaveTextContent('3a7c9f2b');
    expect(tooltip).toHaveTextContent('Backup');
    expect(tooltip).toHaveTextContent(/Backed up/);
    expect(tooltip).toHaveTextContent(/Click to manage/);
    await userEvent.unhover(button);
    await waitFor(() => expect(screen.queryByRole('tooltip')).not.toBeInTheDocument());
  });

  it('click triggers onNavigateToSettings', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    const onNav = vi.fn();
    wrap(onNav);
    await waitFor(() => screen.getByText(/starfire/));
    await userEvent.click(screen.getByRole('button', { name: /your identity/i }));
    expect(onNav).toHaveBeenCalledTimes(1);
  });
});
