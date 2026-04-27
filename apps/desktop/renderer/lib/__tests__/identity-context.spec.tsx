import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';

import { installShareIpcSpy } from '../../test-utils/mock-share-ws';
import { IdentityContextProvider, useIdentity } from '../identity-context';

const FRESH_INSTALL_RESPONSE = {
  id: 'ignored',
  type: 'identity.showResult',
  params: {
    pubkey_hex: 'a'.repeat(64),
    fingerprint_hex: 'a'.repeat(12),
    fingerprint_words: ['apple', 'banana', 'cobra'] as const,
    fingerprint_emoji: ['🦊', '🌲', '🚀', '🧊', '🌙', '⚡'] as const,
    created_at: 1700000000,
    display_name: null,
    backed_up: false,
    last_backed_up_at: null,
    last_rotated_at: null,
    last_backup_path: null,
  },
};

const POPULATED_RESPONSE = {
  id: FRESH_INSTALL_RESPONSE.id,
  type: FRESH_INSTALL_RESPONSE.type,
  params: {
    ...FRESH_INSTALL_RESPONSE.params,
    display_name: 'starfire',
    backed_up: true,
    last_backed_up_at: 1700000000,
  },
};

const wrapper = ({ children }: { children: ReactNode }) => (
  <IdentityContextProvider>{children}</IdentityContextProvider>
);

describe('IdentityContext', () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  it('dispatches identity.show on mount and exposes the response', async () => {
    const { sendSpy } = installShareIpcSpy({ defaultResponse: POPULATED_RESPONSE });

    const { result } = renderHook(() => useIdentity(), { wrapper });

    await waitFor(() => expect(result.current.identity).not.toBeNull());
    expect(result.current.identity?.display_name).toBe('starfire');
    expect(result.current.loading).toBe(false);
    expect(sendSpy).toHaveBeenCalledTimes(1);
    expect(sendSpy.mock.calls[0][0]).toMatchObject({
      type: 'identity.show',
      params: {},
    });
  });

  it('exposes is_fresh_install when null name + null timestamps + !backed_up', async () => {
    installShareIpcSpy({ defaultResponse: FRESH_INSTALL_RESPONSE });
    const { result } = renderHook(() => useIdentity(), { wrapper });
    await waitFor(() => expect(result.current.identity).not.toBeNull());
    expect(result.current.is_fresh_install).toBe(true);
  });

  it('refresh() re-dispatches identity.show', async () => {
    const { sendSpy } = installShareIpcSpy({ defaultResponse: POPULATED_RESPONSE });
    const { result } = renderHook(() => useIdentity(), { wrapper });
    await waitFor(() => expect(result.current.identity).not.toBeNull());

    await act(async () => {
      await result.current.refresh();
    });
    expect(sendSpy).toHaveBeenCalledTimes(2);
  });

  it('is_fresh_install flips to false after first_run_handled is set', async () => {
    installShareIpcSpy({ defaultResponse: FRESH_INSTALL_RESPONSE });
    const { result } = renderHook(() => useIdentity(), { wrapper });
    await waitFor(() => expect(result.current.is_fresh_install).toBe(true));

    act(() => result.current.markFirstRunHandled());

    expect(result.current.is_fresh_install).toBe(false);
  });
});
