import type { Overlay, GameAssignment, StorageAdapter } from '@/types/omni';
import { SAMPLE_DEFAULT_OVERLAY } from '@/types/omni';

/**
 * Storage adapter that uses localStorage. Designed to be swapped
 * for an Electron IPC adapter that calls the host's WebSocket API.
 */

export const localStorageAdapter: StorageAdapter = {
  loadOverlays: () => {
    try {
      const raw = localStorage.getItem('omni_overlays');
      if (raw) return JSON.parse(raw) as Overlay[];
    } catch { /* ignore */ }
    return [SAMPLE_DEFAULT_OVERLAY];
  },

  saveOverlays: (overlays: Overlay[]) => {
    try {
      localStorage.setItem('omni_overlays', JSON.stringify(overlays));
    } catch { /* ignore */ }
  },

  loadGameAssignments: () => {
    try {
      const raw = localStorage.getItem('omni_game_assignments');
      if (raw) return JSON.parse(raw) as GameAssignment[];
    } catch { /* ignore */ }
    return [];
  },

  saveGameAssignments: (assignments: GameAssignment[]) => {
    try {
      localStorage.setItem('omni_game_assignments', JSON.stringify(assignments));
    } catch { /* ignore */ }
  },

  loadActiveOverlayId: () => {
    try {
      return localStorage.getItem('omni_active_overlay_id');
    } catch { return null; }
  },

  saveActiveOverlayId: (id: string | null) => {
    try {
      if (id) {
        localStorage.setItem('omni_active_overlay_id', id);
      } else {
        localStorage.removeItem('omni_active_overlay_id');
      }
    } catch { /* ignore */ }
  },
};

export function getStorageAdapter(): StorageAdapter {
  return localStorageAdapter;
}
