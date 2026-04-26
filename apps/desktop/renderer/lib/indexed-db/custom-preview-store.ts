import { getDb } from '../persistence';

const STORE_NAME = 'customPreview';

/**
 * Per INV-7.9.1, custom preview bytes accepted in Step 2 are persisted in
 * IndexedDB keyed by the workspace-relative overlay path. The store name
 * (`customPreview`) provides namespacing so the key itself is the bare
 * overlay path — no `customPreview:` prefix needed in the key string.
 *
 * Value shape per INV-7.9.1: `{ blob, mimeType, size, addedAt }`.
 */
export interface CustomPreviewRecord {
  blob: Blob;
  mimeType: string;
  size: number;
  addedAt: number;
}

/** Returns the persisted custom preview for `overlayPath`, or `null` if absent. */
export async function getCustomPreview(
  overlayPath: string,
): Promise<CustomPreviewRecord | null> {
  const db = await getDb();
  const record = (await db.get(STORE_NAME, overlayPath)) as
    | CustomPreviewRecord
    | undefined;
  return record ?? null;
}

/**
 * Persists a custom preview blob for `overlayPath`. Overwrites any existing
 * entry. `size` and `addedAt` are derived from the blob and the current time
 * to keep callers from having to compute them.
 */
export async function setCustomPreview(
  overlayPath: string,
  blob: Blob,
  mimeType: string,
): Promise<void> {
  const db = await getDb();
  const record: CustomPreviewRecord = {
    blob,
    mimeType,
    size: blob.size,
    addedAt: Date.now(),
  };
  await db.put(STORE_NAME, record, overlayPath);
}

/** Removes the custom preview for `overlayPath`. No-op if absent. */
export async function removeCustomPreview(overlayPath: string): Promise<void> {
  const db = await getDb();
  await db.delete(STORE_NAME, overlayPath);
}
