import { describe, it, expect, beforeEach } from 'vitest';
import {
  saveEditorState,
  loadEditorState,
  clearEditorState,
  type PersistedEditorState,
} from '../persistence';

// fake-indexeddb provides an in-memory IndexedDB for Node tests
import 'fake-indexeddb/auto';

const mockState: PersistedEditorState = {
  openTabs: [
    {
      id: 'overlay:Test',
      name: 'Test',
      type: 'overlay',
      content: '<widget name="fps"/>',
      isDirty: true,
    },
  ],
  activeTabId: 'overlay:Test',
  viewStates: {
    'overlay:Test': {
      cursorPosition: { lineNumber: 5, column: 10 },
      scrollTop: 120,
      scrollLeft: 0,
    },
  },
};

describe('persistence', () => {
  beforeEach(async () => {
    await clearEditorState();
  });

  it('should return null when no state is persisted', async () => {
    const result = await loadEditorState();
    expect(result).toBeNull();
  });

  it('should save and load editor state', async () => {
    await saveEditorState(mockState);
    const result = await loadEditorState();
    expect(result).toEqual(mockState);
  });

  it('should overwrite previous state on save', async () => {
    await saveEditorState(mockState);
    const updated = {
      ...mockState,
      activeTabId: 'overlay:Other',
    };
    await saveEditorState(updated);
    const result = await loadEditorState();
    expect(result?.activeTabId).toBe('overlay:Other');
  });

  it('should return null after clear', async () => {
    await saveEditorState(mockState);
    await clearEditorState();
    const result = await loadEditorState();
    expect(result).toBeNull();
  });
});
