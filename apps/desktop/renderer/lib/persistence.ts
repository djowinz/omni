import { openDB, type IDBPDatabase } from 'idb';
import type { EditorTab } from '@/types/omni';

const DB_NAME = 'omni-editor';
const DB_VERSION = 1;
const STORE_NAME = 'state';
const STATE_KEY = 'editor';

export interface EditorViewState {
  cursorPosition: { lineNumber: number; column: number };
  scrollTop: number;
  scrollLeft: number;
}

export interface PersistedEditorState {
  openTabs: EditorTab[];
  activeTabId: string | null;
  viewStates: Record<string, EditorViewState>;
}

function getDb(): Promise<IDBPDatabase> {
  return openDB(DB_NAME, DB_VERSION, {
    upgrade(db) {
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME);
      }
    },
  });
}

export async function saveEditorState(state: PersistedEditorState): Promise<void> {
  const db = await getDb();
  await db.put(STORE_NAME, state, STATE_KEY);
}

export async function loadEditorState(): Promise<PersistedEditorState | null> {
  const db = await getDb();
  const result = await db.get(STORE_NAME, STATE_KEY);
  return result ?? null;
}

export async function clearEditorState(): Promise<void> {
  const db = await getDb();
  await db.delete(STORE_NAME, STATE_KEY);
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Debounced save — buffers writes at 500ms intervals to avoid thrashing IndexedDB.
 * Call `flushEditorState` for immediate write (e.g., before navigation).
 */
export function persistEditorStateDebounced(state: PersistedEditorState): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    saveEditorState(state);
    debounceTimer = null;
  }, 500);
}

/**
 * Immediate non-debounced write — use before page navigation or unmount.
 */
export async function flushEditorState(state: PersistedEditorState): Promise<void> {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  await saveEditorState(state);
}
