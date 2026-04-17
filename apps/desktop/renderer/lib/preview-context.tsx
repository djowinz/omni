import { createContext, useContext, useState, useMemo, type ReactNode } from 'react';
import type { CachedArtifactDetail } from './share-types';

export interface PreviewState {
  activeToken: string | null;
  activeArtifact: CachedArtifactDetail | null;
}

export interface PreviewContextValue extends PreviewState {
  setPreview: (token: string, artifact: CachedArtifactDetail) => void;
  clearPreview: () => void;
}

const PreviewContext = createContext<PreviewContextValue | null>(null);

export function PreviewContextProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<PreviewState>({
    activeToken: null,
    activeArtifact: null,
  });
  const value = useMemo<PreviewContextValue>(
    () => ({
      ...state,
      setPreview: (token, artifact) => setState({ activeToken: token, activeArtifact: artifact }),
      clearPreview: () => setState({ activeToken: null, activeArtifact: null }),
    }),
    [state],
  );
  return <PreviewContext.Provider value={value}>{children}</PreviewContext.Provider>;
}

export function usePreview(): PreviewContextValue {
  const ctx = useContext(PreviewContext);
  if (ctx === null) {
    throw new Error('usePreview() must be called inside <PreviewContextProvider>');
  }
  return ctx;
}
