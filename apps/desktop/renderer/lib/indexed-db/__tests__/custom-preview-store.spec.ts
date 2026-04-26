/**
 * @vitest-environment node
 *
 * Node env (not jsdom) so that `Blob` round-trips through fake-indexeddb's
 * structured-clone with its prototype intact. Under jsdom, the cloned value
 * comes back as a plain object stripped of `Blob` methods/props — purely a
 * test-environment artifact (production runs in Electron's real Chromium).
 * Node 22+'s global `Blob` clones faithfully end-to-end.
 *
 * fake-indexeddb provides an in-memory IndexedDB for Node tests; matches the
 * `import 'fake-indexeddb/auto'` pattern used by `persistence.spec.ts`.
 */
import 'fake-indexeddb/auto';
import { describe, it, expect, beforeEach } from 'vitest';
import { getCustomPreview, setCustomPreview, removeCustomPreview } from '../custom-preview-store';

describe('custom-preview-store', () => {
  beforeEach(async () => {
    // Reset between tests by removing the keys we touch.
    await removeCustomPreview('overlays/x/index.html');
    await removeCustomPreview('overlays/missing/index.html');
    await removeCustomPreview('overlays/y/index.html');
    await removeCustomPreview('overlays/a/index.html');
    await removeCustomPreview('overlays/b/index.html');
  });

  it('set + get roundtrip preserves blob bytes and metadata', async () => {
    const blob = new Blob(['hello'], { type: 'image/png' });
    const before = Date.now();
    await setCustomPreview('overlays/x/index.html', blob, 'image/png');
    const after = Date.now();

    const record = await getCustomPreview('overlays/x/index.html');
    expect(record).not.toBeNull();
    expect(record!.mimeType).toBe('image/png');
    expect(record!.size).toBe(5);
    expect(record!.blob.size).toBe(5);
    expect(record!.addedAt).toBeGreaterThanOrEqual(before);
    expect(record!.addedAt).toBeLessThanOrEqual(after);

    const text = await record!.blob.text();
    expect(text).toBe('hello');
  });

  it('returns null for a missing key', async () => {
    const record = await getCustomPreview('overlays/missing/index.html');
    expect(record).toBeNull();
  });

  it('remove deletes the persisted entry', async () => {
    const blob = new Blob(['hi'], { type: 'image/jpeg' });
    await setCustomPreview('overlays/y/index.html', blob, 'image/jpeg');
    await removeCustomPreview('overlays/y/index.html');
    expect(await getCustomPreview('overlays/y/index.html')).toBeNull();
  });

  it('multiple keys are isolated from each other', async () => {
    const a = new Blob(['A'], { type: 'image/png' });
    const b = new Blob(['BB'], { type: 'image/webp' });
    await setCustomPreview('overlays/a/index.html', a, 'image/png');
    await setCustomPreview('overlays/b/index.html', b, 'image/webp');

    const ra = await getCustomPreview('overlays/a/index.html');
    const rb = await getCustomPreview('overlays/b/index.html');

    expect(ra!.size).toBe(1);
    expect(ra!.mimeType).toBe('image/png');
    expect(rb!.size).toBe(2);
    expect(rb!.mimeType).toBe('image/webp');
  });

  it('setting the same key overwrites the previous entry', async () => {
    const first = new Blob(['first'], { type: 'image/png' });
    const second = new Blob(['second-longer'], { type: 'image/jpeg' });
    await setCustomPreview('overlays/x/index.html', first, 'image/png');
    await setCustomPreview('overlays/x/index.html', second, 'image/jpeg');

    const record = await getCustomPreview('overlays/x/index.html');
    expect(record!.size).toBe('second-longer'.length);
    expect(record!.mimeType).toBe('image/jpeg');
    expect(await record!.blob.text()).toBe('second-longer');
  });
});
