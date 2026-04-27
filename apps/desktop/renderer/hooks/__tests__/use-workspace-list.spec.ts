/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';

// upload-flow-redesign Wave A0 (OWI-34): use-workspace-list now calls the
// `workspace.listPublishables` Share-WS RPC instead of the legacy `file.list`
// IPC. The hook returns rich `entries` plus backwards-compatible `overlays`
// + `themes` name arrays derived from those entries; tests cover both the
// new shape and the derived arrays so existing call sites keep working.

function makeEntry(overrides: {
  kind: 'overlay' | 'theme';
  name: string;
  workspace_path: string;
  widget_count?: number | null;
  has_preview?: boolean;
}) {
  return {
    kind: overrides.kind,
    workspace_path: overrides.workspace_path,
    name: overrides.name,
    widget_count: overrides.widget_count ?? null,
    modified_at: '2026-04-21T00:00:00Z',
    has_preview: overrides.has_preview ?? false,
    sidecar: null,
  };
}

// Loose typing on the mock callback — vitest's `vi.fn` produces a `Mock` type
// that doesn't structurally satisfy `(...args: unknown[]) => unknown` due to
// parameter-variance rules. The runtime side just forwards the call, so we
// type the parameter as `any` here and let each test site narrow as it needs.
function stubShareWs(sendShareMessage: any) {
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage,
    onShareEvent: vi.fn(() => () => {}),
  });
  // crypto.randomUUID is read by useShareWs.send to mint request ids.
  if (!('randomUUID' in (globalThis.crypto ?? {}))) {
    vi.stubGlobal('crypto', {
      ...(globalThis.crypto ?? {}),
      randomUUID: () => '00000000-0000-4000-8000-000000000000',
    });
  }
}

describe('useWorkspaceList', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('fetches overlays + themes via workspace.listPublishables', async () => {
    const sendShareMessage = vi.fn(async (msg: { id: string; type: string }) => ({
      id: msg.id,
      type: 'workspace.listPublishablesResult',
      params: {
        entries: [
          makeEntry({
            kind: 'overlay',
            name: 'Default',
            workspace_path: 'overlays/Default',
            widget_count: 3,
            has_preview: true,
          }),
          makeEntry({
            kind: 'overlay',
            name: 'Marathon',
            workspace_path: 'overlays/Marathon',
            widget_count: 5,
          }),
          makeEntry({
            kind: 'theme',
            name: 'marathon',
            workspace_path: 'themes/marathon.css',
          }),
        ],
      },
    }));
    stubShareWs(sendShareMessage);
    const { useWorkspaceList } = await import('../use-workspace-list');
    const { result } = renderHook(() => useWorkspaceList());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries).toHaveLength(3);
    expect(result.current.overlays).toEqual(['Default', 'Marathon']);
    expect(result.current.themes).toEqual(['marathon.css']);
    // Verify the request envelope shape — the renderer wraps the empty
    // params object so the host's serde can deserialize the `kind: None`
    // filter.
    expect(sendShareMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'workspace.listPublishables',
        params: {},
      }),
    );
  });

  it('captures errors from workspace.listPublishables', async () => {
    const sendShareMessage = vi.fn(async () => {
      // Emulate the D-004-J error envelope shape returned by the host.
      throw {
        code: 'BAD_INPUT',
        kind: 'Malformed',
        detail: null,
        message: 'workspace.listPublishables failed',
      };
    });
    stubShareWs(sendShareMessage);
    const { useWorkspaceList } = await import('../use-workspace-list');
    const { result } = renderHook(() => useWorkspaceList());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toBeTruthy();
    expect(result.current.error?.message).toBe('workspace.listPublishables failed');
    expect(result.current.entries).toEqual([]);
    expect(result.current.overlays).toEqual([]);
    expect(result.current.themes).toEqual([]);
  });

  it('refetch() re-runs the RPC', async () => {
    let calls = 0;
    const sendShareMessage = vi.fn(async (msg: { id: string; type: string }) => {
      calls += 1;
      return {
        id: msg.id,
        type: 'workspace.listPublishablesResult',
        params: {
          entries: [
            makeEntry({
              kind: 'overlay',
              name: 'A',
              workspace_path: 'overlays/A',
            }),
            ...(calls === 2
              ? [
                  makeEntry({
                    kind: 'overlay',
                    name: 'B',
                    workspace_path: 'overlays/B',
                  }),
                ]
              : []),
          ],
        },
      };
    });
    stubShareWs(sendShareMessage);
    const { useWorkspaceList } = await import('../use-workspace-list');
    const { result } = renderHook(() => useWorkspaceList());

    await waitFor(() => expect(result.current.overlays).toEqual(['A']));

    await result.current.refetch();

    await waitFor(() => expect(result.current.overlays).toEqual(['A', 'B']));
    expect(sendShareMessage).toHaveBeenCalledTimes(2);
  });
});
