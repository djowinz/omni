import { describe, it, expect } from 'vitest';
import { PublishLookupWorkspaceResultSchema } from '../share-types';

describe('PublishLookupWorkspaceResultSchema', () => {
  it('parses status=ok with workspace_path', () => {
    const result = PublishLookupWorkspaceResultSchema.parse({
      id: 'abc',
      type: 'publish.lookupWorkspaceResult',
      artifact_id: 'A',
      status: 'ok',
      workspace_path: 'overlays/HWMon Compact',
      kind: 'overlay',
      name: 'HWMon Compact',
    });
    expect(result.status).toBe('ok');
  });

  it('parses status=missing_index with null fields', () => {
    const result = PublishLookupWorkspaceResultSchema.parse({
      id: 'abc',
      type: 'publish.lookupWorkspaceResult',
      artifact_id: 'A',
      status: 'missing_index',
      workspace_path: null,
      kind: null,
      name: null,
    });
    expect(result.status).toBe('missing_index');
  });

  it('parses status=missing_folder with partial fields', () => {
    const result = PublishLookupWorkspaceResultSchema.parse({
      id: 'abc',
      type: 'publish.lookupWorkspaceResult',
      artifact_id: 'A',
      status: 'missing_folder',
      workspace_path: null,
      kind: 'theme',
      name: 'some-theme',
    });
    expect(result.kind).toBe('theme');
  });

  it('rejects unknown status', () => {
    expect(() =>
      PublishLookupWorkspaceResultSchema.parse({
        id: 'abc',
        type: 'publish.lookupWorkspaceResult',
        artifact_id: 'A',
        status: 'bogus',
        workspace_path: null,
        kind: null,
        name: null,
      }),
    ).toThrow();
  });
});
