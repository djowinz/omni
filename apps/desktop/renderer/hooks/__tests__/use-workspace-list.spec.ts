/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';

describe('useWorkspaceList', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('fetches overlays + themes on mount via file.list', async () => {
    const sendMessage = vi.fn(async () => ({
      type: 'file.list',
      overlays: ['Default', 'Marathon'],
      themes: ['marathon.css'],
    }));
    vi.stubGlobal('omni', {
      sendMessage,
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    const { useWorkspaceList } = await import('../use-workspace-list');
    const { result } = renderHook(() => useWorkspaceList());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.overlays).toEqual(['Default', 'Marathon']);
    expect(result.current.themes).toEqual(['marathon.css']);
    expect(sendMessage).toHaveBeenCalledWith({ type: 'file.list' });
  });

  it('captures errors from file.list', async () => {
    const sendMessage = vi.fn(async () => {
      throw new Error('IPC failed');
    });
    vi.stubGlobal('omni', {
      sendMessage,
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    const { useWorkspaceList } = await import('../use-workspace-list');
    const { result } = renderHook(() => useWorkspaceList());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toBeTruthy();
    expect(result.current.overlays).toEqual([]);
  });

  it('refetch() re-runs the fetch', async () => {
    let calls = 0;
    const sendMessage = vi.fn(async () => {
      calls += 1;
      return {
        type: 'file.list',
        overlays: calls === 1 ? ['A'] : ['A', 'B'],
        themes: [],
      };
    });
    vi.stubGlobal('omni', {
      sendMessage,
      sendShareMessage: vi.fn(),
      onShareEvent: vi.fn(() => () => {}),
    });
    const { useWorkspaceList } = await import('../use-workspace-list');
    const { result } = renderHook(() => useWorkspaceList());

    await waitFor(() => expect(result.current.overlays).toEqual(['A']));

    await result.current.refetch();

    await waitFor(() => expect(result.current.overlays).toEqual(['A', 'B']));
    expect(sendMessage).toHaveBeenCalledTimes(2);
  });
});
