/**
 * useWorkspaceList — fetch overlays + themes via the existing file.list IPC.
 *
 * Wraps `window.omni.sendMessage({ type: 'file.list' })` (the generic
 * ws-message channel, distinct from the share:ws-message channel used for
 * `explorer.*` / `upload.*`). The host's `handle_list` returns
 * `{ type: 'file.list', overlays: string[], themes: string[] }`.
 *
 * Consumed by #015's UploadDialog source-picker step and potentially by
 * other consumers that need to list workspace contents.
 */

import { useCallback, useEffect, useState } from 'react';

export interface WorkspaceListState {
  overlays: string[];
  themes: string[];
  loading: boolean;
  error: Error | null;
  refetch: () => Promise<void>;
}

interface FileListFrame {
  type: 'file.list';
  overlays?: string[];
  themes?: string[];
}

export function useWorkspaceList(): WorkspaceListState {
  const [overlays, setOverlays] = useState<string[]>([]);
  const [themes, setThemes] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  const fetch = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const bridge = window.omni?.sendMessage;
      if (!bridge) throw new Error('IPC bridge not available');
      const resp = (await bridge({ type: 'file.list' })) as FileListFrame;
      setOverlays(Array.isArray(resp.overlays) ? resp.overlays : []);
      setThemes(Array.isArray(resp.themes) ? resp.themes : []);
    } catch (err) {
      setError(err instanceof Error ? err : new Error(String(err)));
      setOverlays([]);
      setThemes([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetch();
  }, [fetch]);

  return { overlays, themes, loading, error, refetch: fetch };
}
