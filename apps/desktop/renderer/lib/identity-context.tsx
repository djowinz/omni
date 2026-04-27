import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useShareWs } from '../hooks/use-share-ws';

export interface IdentitySnapshot {
  pubkey_hex: string;
  fingerprint_hex: string;
  fingerprint_words: readonly [string, string, string];
  fingerprint_emoji: readonly [string, string, string, string, string, string];
  created_at: number;
  display_name: string | null;
  backed_up: boolean;
  last_backed_up_at: number | null;
  last_rotated_at: number | null;
  last_backup_path: string | null;
}

export interface IdentityContextValue {
  identity: IdentitySnapshot | null;
  loading: boolean;
  is_fresh_install: boolean;
  first_run_handled: boolean;
  refresh: () => Promise<void>;
  markFirstRunHandled: () => void;
}

const IdentityContext = createContext<IdentityContextValue | null>(null);

function isFreshInstall(snap: IdentitySnapshot, handled: boolean): boolean {
  return (
    !handled &&
    snap.display_name === null &&
    snap.last_rotated_at === null &&
    snap.last_backed_up_at === null &&
    !snap.backed_up
  );
}

export function IdentityContextProvider({ children }: { children: ReactNode }) {
  const { send } = useShareWs();
  const [identity, setIdentity] = useState<IdentitySnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [first_run_handled, setFirstRunHandled] = useState(false);

  const fetchIdentity = useCallback(async () => {
    setLoading(true);
    try {
      const response = (await send('identity.show', {})) as unknown as IdentitySnapshot & {
        type?: string;
        id?: string;
      };
      const { type: _t, id: _i, ...snap } = response;
      setIdentity(snap);
    } finally {
      setLoading(false);
    }
  }, [send]);

  useEffect(() => {
    void fetchIdentity();
  }, [fetchIdentity]);

  const value = useMemo<IdentityContextValue>(
    () => ({
      identity,
      loading,
      is_fresh_install: identity ? isFreshInstall(identity, first_run_handled) : false,
      first_run_handled,
      refresh: fetchIdentity,
      markFirstRunHandled: () => setFirstRunHandled(true),
    }),
    [identity, loading, first_run_handled, fetchIdentity],
  );

  return <IdentityContext.Provider value={value}>{children}</IdentityContext.Provider>;
}

export function useIdentity(): IdentityContextValue {
  const ctx = useContext(IdentityContext);
  if (ctx === null) {
    throw new Error('useIdentity must be used inside <IdentityContextProvider>');
  }
  return ctx;
}
