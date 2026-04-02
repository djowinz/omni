


import { useMemo } from 'react';
import { useOmniState } from '@/hooks/use-omni-state';
import { parseOmniContent } from '@/lib/omni-parser';
import { Layers, Gamepad2, Circle } from 'lucide-react';

export function StatusBar() {
  const { state, getCurrentOverlay } = useOmniState();
  const currentOverlay = getCurrentOverlay();

  const widgets = useMemo(
    () => currentOverlay?.content ? parseOmniContent(currentOverlay.content) : [],
    [currentOverlay?.content]
  );
  const enabledCount = widgets.filter(w => w.enabled).length;

  // Determine overlay type for current context
  const getOverlayType = (): { label: string; color: string } => {
    if (currentOverlay?.name === 'Default') return { label: 'Default', color: '#3B82F6' };
    if (currentOverlay?.name === state.config?.active_overlay) return { label: 'Active', color: '#00D9FF' };
    return { label: 'Custom', color: '#A855F7' };
  };

  const overlayType = getOverlayType();

  // Count games assigned to this overlay via config
  const assignedGames = currentOverlay
    ? Object.values(state.config?.overlay_by_game ?? {}).filter(
        name => name === currentOverlay.name
      ).length
    : 0;

  return (
    <footer className="flex h-7 items-center justify-between border-t border-[#27272A] bg-[#18181B] px-4 text-[10px]">
      <div className="flex items-center gap-4">
        {/* Connection Status */}
        <div className="flex items-center gap-1.5">
          <Circle className={`h-2 w-2 ${state.connected ? 'fill-[#22C55E] text-[#22C55E]' : 'fill-[#EF4444] text-[#EF4444]'}`} />
          <span className="text-[#71717A] uppercase tracking-wider">
            {state.connected ? 'Connected' : 'Disconnected'}
          </span>
        </div>

        {/* Divider */}
        <div className="h-3 w-px bg-[#27272A]" />

        {/* Overlay Type */}
        <div className="flex items-center gap-1.5">
          <span className="text-[#52525B]">Type:</span>
          <span style={{ color: overlayType.color }}>{overlayType.label}</span>
        </div>

        {/* Widget Count */}
        <div className="flex items-center gap-1.5">
          <Layers className="h-3 w-3 text-[#52525B]" />
          <span className="text-[#71717A]">
            <span className="text-[#FAFAFA]">{enabledCount}</span>/{widgets.length} widgets
          </span>
        </div>

        {/* Games Assigned */}
        {assignedGames > 0 && (
          <div className="flex items-center gap-1.5">
            <Gamepad2 className="h-3 w-3 text-[#52525B]" />
            <span className="text-[#71717A]">
              <span className="text-[#FAFAFA]">{assignedGames}</span> games
            </span>
          </div>
        )}
      </div>

      <div className="flex items-center gap-4">
        {/* Unsaved indicator */}
        {state.isDirty && (
          <div className="flex items-center gap-1.5">
            <Circle className="h-2 w-2 fill-[#F59E0B] text-[#F59E0B]" />
            <span className="text-[#F59E0B]">Unsaved changes</span>
          </div>
        )}

        {/* Version */}
        <span className="text-[#52525B] font-mono">v1.0.0</span>
      </div>
    </footer>
  );
}
