import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { installShareIpcSpy } from '../../../test-utils/mock-share-ws';
import { IdentityContextProvider } from '../../../lib/identity-context';
import { DisplayNameField } from '../display-name-field';

const PUBKEY = '3a7c9f2b' + '0'.repeat(56);

const POPULATED = {
  id: 'x',
  type: 'identity.showResult',
  params: {
    pubkey_hex: PUBKEY,
    fingerprint_hex: '3a7c9f2ba8b1',
    fingerprint_words: ['apple', 'banana', 'cobra'],
    fingerprint_emoji: ['🦊', '🌲', '🚀', '🧊', '🌙', '⚡'],
    created_at: 1700000000,
    display_name: 'starfire',
    backed_up: true,
    last_backed_up_at: 1700000000,
    last_rotated_at: null,
    last_backup_path: null,
  },
};

const FRESH = {
  ...POPULATED,
  params: { ...POPULATED.params, display_name: null, backed_up: false, last_backed_up_at: null },
};

const wrap = (children: ReactNode) =>
  render(<IdentityContextProvider>{children}</IdentityContextProvider>);

describe('DisplayNameField', () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  it('read mode shows the current display_name with an Edit pencil', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap(<DisplayNameField />);
    await waitFor(() => expect(screen.getByText('starfire')).toBeInTheDocument());
    expect(screen.getByRole('button', { name: /edit display name/i })).toBeInTheDocument();
  });

  it('placeholder "Set a display name" when display_name is null', async () => {
    installShareIpcSpy({ defaultResponse: FRESH });
    wrap(<DisplayNameField />);
    await waitFor(() => expect(screen.getByText(/set a display name/i)).toBeInTheDocument());
  });

  it('clicking Edit reveals input + Save/Cancel + live preview', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap(<DisplayNameField />);
    await waitFor(() => screen.getByText('starfire'));
    await userEvent.click(screen.getByRole('button', { name: /edit display name/i }));
    expect(screen.getByRole('textbox', { name: /display name/i })).toHaveValue('starfire');
    expect(screen.getByRole('button', { name: /save/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
    expect(screen.getByText(/starfire#3a7c9f2b/)).toBeInTheDocument();
  });

  it('rejects 33-codepoint input with §3.4 error message and disables Save', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap(<DisplayNameField />);
    await waitFor(() => screen.getByText('starfire'));
    await userEvent.click(screen.getByRole('button', { name: /edit display name/i }));
    const input = screen.getByRole('textbox', { name: /display name/i });
    await userEvent.clear(input);
    await userEvent.type(input, 'starfire-of-the-northern-wastes-!');
    expect(screen.getByText(/1.*32 characters/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /save/i })).toBeDisabled();
  });

  it('counts emoji as 1 code point (not 2 UTF-16 units)', async () => {
    installShareIpcSpy({ defaultResponse: POPULATED });
    wrap(<DisplayNameField />);
    await waitFor(() => screen.getByText('starfire'));
    await userEvent.click(screen.getByRole('button', { name: /edit display name/i }));
    const input = screen.getByRole('textbox', { name: /display name/i });
    await userEvent.clear(input);
    await userEvent.type(input, '😀');
    expect(screen.getByText(/1 \/ 32/)).toBeInTheDocument();
  });

  it('Save dispatches identity.setDisplayName with normalized value', async () => {
    const { sendSpy } = installShareIpcSpy({ defaultResponse: POPULATED });
    sendSpy.mockResolvedValueOnce(POPULATED);
    sendSpy.mockResolvedValueOnce({
      id: 'x',
      type: 'identity.setDisplayNameResult',
      params: { display_name: 'nightowl', pubkey_hex: PUBKEY },
    });
    sendSpy.mockResolvedValueOnce({
      ...POPULATED,
      params: { ...POPULATED.params, display_name: 'nightowl' },
    });

    wrap(<DisplayNameField />);
    await waitFor(() => screen.getByText('starfire'));
    await userEvent.click(screen.getByRole('button', { name: /edit display name/i }));
    const input = screen.getByRole('textbox', { name: /display name/i });
    await userEvent.clear(input);
    await userEvent.type(input, 'nightowl');
    await userEvent.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => {
      const set = sendSpy.mock.calls.find((c: any) => c[0]?.type === 'identity.setDisplayName');
      expect(set?.[0]).toMatchObject({
        type: 'identity.setDisplayName',
        params: { display_name: 'nightowl' },
      });
    });
  });
});
