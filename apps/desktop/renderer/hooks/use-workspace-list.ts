/**
 * useWorkspaceList — fetch workspace overlays + themes via the
 * `workspace.listPublishables` Share-WS RPC.
 *
 * upload-flow-redesign Wave A0 (OWI-34): replaces the previous `file.list`
 * IPC round-trip + per-row parallel reads with a single rich-row RPC. Each
 * row carries the data the Step 1 source picker needs to render itself
 * without further round-trips:
 *   - widget count (overlays)
 *   - mtime as ISO-8601 string (INV-7.1.10 modified-date subtitle)
 *   - `.omni-preview.png` presence (INV-7.1.9 thumbnail vs zinc placeholder)
 *   - `.omni-publish.json` sidecar (INV-7.1.13 linked-artifact banner +
 *     update-mode pivot)
 *
 * Wire contract: `apps/desktop/renderer/lib/share-types.ts`
 * (`WorkspaceListPublishablesResultSchema` + `PublishablesEntrySchema`).
 * Host source: `crates/host/src/share/ws_messages.rs handle_list_publishables`.
 *
 * Backwards compatibility: existing callers (`UploadDialog` SourceStep,
 * the existing test that asserts `result.current.overlays`) consume
 * `overlays: string[]` + `themes: string[]`. Those arrays are derived from
 * the new `entries` field (`entries.filter(e => e.kind === 'overlay').map(e => e.name)`)
 * so consumers continue to work unchanged. New consumers read `entries`
 * directly to access the rich metadata.
 *
 * Loading semantics unchanged: starts true, flips to false on resolve or
 * reject. Errors surface via `error: Error | null`.
 */

import { useCallback, useEffect, useState } from 'react';
import { useShareWs } from './use-share-ws';
import type { ShareWsError } from '../lib/share-types';
import type { PublishablesEntry } from '@omni/shared-types';

export interface WorkspaceListState {
  /**
   * Rich rows from `workspace.listPublishables`. Includes both kinds
   * (overlays + themes) interleaved in the order the host returned them
   * (overlays first, then themes — see `handle_list_publishables`).
   */
  entries: PublishablesEntry[];
  /**
   * Backwards-compatible name list for overlay rows. Equivalent to
   * `entries.filter(e => e.kind === 'overlay').map(e => e.name)`. Existing
   * callers (UploadDialog SourceStep, source-picker tests) consume this
   * shape; new callers should read `entries` directly to access widget
   * counts, mtime, preview presence, and sidecar metadata.
   */
  overlays: string[];
  /** Backwards-compatible name list for theme rows (filenames including `.css`). */
  themes: string[];
  loading: boolean;
  error: Error | null;
  refetch: () => Promise<void>;
}

export function useWorkspaceList(): WorkspaceListState {
  const { send } = useShareWs();
  const [entries, setEntries] = useState<PublishablesEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  const fetch = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const resp = await send('workspace.listPublishables', {});
      // Defensive default: real `useShareWs.send` Zod-validates the response
      // so `resp.params.entries` is always an array. Tests that mock the
      // hook can return non-conforming shapes (the mock bypasses validation),
      // so we coalesce to `[]` rather than crashing the consumer when mid-test
      // re-renders read `entries` before a mock update settles.
      setEntries(resp?.params?.entries ?? []);
    } catch (err) {
      // ShareWsError carries `message` per the D-004-J envelope; wrap it as
      // a real Error so the existing `error instanceof Error` check in
      // consumers works without type-narrowing.
      const e = err as ShareWsError;
      setError(new Error(e?.message ?? String(err)));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [send]);

  useEffect(() => {
    void fetch();
  }, [fetch]);

  // Derived arrays for backwards-compatible callers. The original `themes`
  // shape was the raw `.css` filename (e.g. "marathon.css") because the
  // host's `file.list` returned exactly that. The new RPC strips `.css` for
  // the display `name`, but `workspace_path` keeps the suffix
  // (`themes/marathon.css`). To preserve the prior contract we re-derive
  // theme filenames from the path's basename.
  const overlays = entries.filter((e) => e.kind === 'overlay').map((e) => e.name);
  const themes = entries
    .filter((e) => e.kind === 'theme')
    .map((e) => {
      // workspace_path is e.g. "themes/marathon.css"; take the basename so
      // existing callers see "marathon.css" not "themes/marathon.css".
      const idx = e.workspace_path.lastIndexOf('/');
      return idx >= 0 ? e.workspace_path.slice(idx + 1) : e.workspace_path;
    });

  return { entries, overlays, themes, loading, error, refetch: fetch };
}
