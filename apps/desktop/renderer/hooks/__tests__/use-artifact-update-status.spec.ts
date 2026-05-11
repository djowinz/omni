import { renderHook } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { useArtifactUpdateStatus } from '../use-artifact-update-status';
import type { InstalledEntryRow, ArtifactDetail } from '../../lib/share-types';

// Field names match the renderer-facing InstalledEntrySchema in share-types.ts,
// which is the worker.listInstalledResult row shape (not the rust registry row).
function entry(artifact_id: string, installed_version: string): InstalledEntryRow {
  return {
    artifact_id,
    name: artifact_id,
    kind: 'bundle',
    content_hash: 'h',
    author_pubkey: 'pk',
    author_fingerprint_hex: 'fp',
    installed_version,
    installed_path: '',
    installed_at: 0,
  };
}

// ArtifactDetail has more fields than we exercise; double-cast through unknown
// so the test fixture only carries the fields the hook reads.
function detail(artifact_id: string, version: string): ArtifactDetail {
  return {
    artifact_id,
    author_pubkey: 'pk',
    author_display_name: null,
    author_fingerprint_hex: 'fp',
    content_hash: 'h',
    created_at: 0,
    updated_at: 0,
    installs: 0,
    kind: 'bundle',
    manifest: {
      name: artifact_id,
      description: '',
      tags: [],
      license: '',
      version,
      omni_min_version: '0.1.0',
    },
    r2_url: '',
    reports: 0,
    status: 'live',
    thumbnail_url: '',
  } as unknown as ArtifactDetail;
}

describe('useArtifactUpdateStatus', () => {
  it('returns empty Map for empty inputs', () => {
    const { result } = renderHook(() => useArtifactUpdateStatus([], new Map()));
    expect(result.current.size).toBe(0);
  });

  it('skips entries without a byId match (placeholder render)', () => {
    const { result } = renderHook(() =>
      useArtifactUpdateStatus([entry('A', '1.0.0')], new Map()),
    );
    expect(result.current.has('A')).toBe(false);
  });

  it('marks available when worker version > local', () => {
    const byId = new Map([['A', detail('A', '1.0.1')]]);
    const { result } = renderHook(() =>
      useArtifactUpdateStatus([entry('A', '1.0.0')], byId),
    );
    expect(result.current.get('A')).toEqual({
      available: true,
      latest_version: '1.0.1',
      installed_version: '1.0.0',
    });
  });

  it('marks NOT available when worker version === local', () => {
    const byId = new Map([['A', detail('A', '1.0.0')]]);
    const { result } = renderHook(() =>
      useArtifactUpdateStatus([entry('A', '1.0.0')], byId),
    );
    expect(result.current.get('A')?.available).toBe(false);
  });

  it('marks NOT available when worker version < local (downgrade)', () => {
    const byId = new Map([['A', detail('A', '1.0.0')]]);
    const { result } = renderHook(() =>
      useArtifactUpdateStatus([entry('A', '1.0.1')], byId),
    );
    expect(result.current.get('A')?.available).toBe(false);
  });

  it('does not throw on malformed local semver', () => {
    const byId = new Map([['A', detail('A', '1.0.0')]]);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { result } = renderHook(() =>
      useArtifactUpdateStatus([entry('A', 'oops')], byId),
    );
    expect(result.current.get('A')?.available).toBe(false);
    warn.mockRestore();
  });

  it('does not throw on malformed worker semver', () => {
    const byId = new Map([['A', detail('A', '~~~')]]);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { result } = renderHook(() =>
      useArtifactUpdateStatus([entry('A', '1.0.0')], byId),
    );
    expect(result.current.get('A')?.available).toBe(false);
    warn.mockRestore();
  });
});
