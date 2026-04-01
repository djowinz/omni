import type { Overlay, GameAssignment, StorageAdapter } from '@/types/omni';
import { SAMPLE_DEFAULT_OVERLAY } from '@/types/omni';

/**
 * Storage adapter that uses localStorage. Designed to be swapped
 * for an Electron IPC adapter that calls the host's WebSocket API.
 */

export const localStorageAdapter: StorageAdapter = {
  loadOverlays: async () => {
    try {
      const raw = localStorage.getItem('omni_overlays');
      if (raw) return JSON.parse(raw) as Overlay[];
    } catch { /* ignore */ }
    return [SAMPLE_DEFAULT_OVERLAY];
  },

  saveOverlay: async (overlay: Overlay) => {
    try {
      const raw = localStorage.getItem('omni_overlays');
      const overlays: Overlay[] = raw ? JSON.parse(raw) : [];
      const idx = overlays.findIndex(o => o.id === overlay.id);
      if (idx >= 0) {
        overlays[idx] = overlay;
      } else {
        overlays.push(overlay);
      }
      localStorage.setItem('omni_overlays', JSON.stringify(overlays));
    } catch { /* ignore */ }
  },

  deleteOverlay: async (id: string) => {
    try {
      const raw = localStorage.getItem('omni_overlays');
      if (raw) {
        const overlays = (JSON.parse(raw) as Overlay[]).filter(o => o.id !== id);
        localStorage.setItem('omni_overlays', JSON.stringify(overlays));
      }
    } catch { /* ignore */ }
  },

  loadGameAssignments: async () => {
    try {
      const raw = localStorage.getItem('omni_game_assignments');
      if (raw) return JSON.parse(raw) as GameAssignment[];
    } catch { /* ignore */ }
    return [];
  },

  saveGameAssignments: async (assignments: GameAssignment[]) => {
    try {
      localStorage.setItem('omni_game_assignments', JSON.stringify(assignments));
    } catch { /* ignore */ }
  },

  getActiveOverlayId: async () => {
    try {
      return localStorage.getItem('omni_active_overlay_id');
    } catch { return null; }
  },

  setActiveOverlayId: async (id: string | null) => {
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
