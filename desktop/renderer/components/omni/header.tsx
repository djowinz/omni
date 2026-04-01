

import { Settings, Star, Copy, Trash2, Plus, Gamepad2, Minus, Square, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Badge } from '@/components/ui/badge';
import { useOmniState } from '@/hooks/use-omni-state';
import { useState } from 'react';
import { CreateOverlayDialog } from './create-overlay-dialog';
import { GameAssignmentsDialog } from './game-assignments-dialog';

export function Header() {
  const { state, dispatch, setAsActive, duplicateOverlay, deleteOverlay, getCurrentOverlay } =
    useOmniState();
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [gamesDialogOpen, setGamesDialogOpen] = useState(false);

  const currentOverlay = getCurrentOverlay();
  const isActive = currentOverlay?.id === state.activeOverlayId;
  const isDefault = currentOverlay?.isDefault;

  const handleSelectOverlay = (id: string) => {
    dispatch({ type: 'SELECT_OVERLAY', payload: id });
  };

  const handleSetActive = async () => {
    if (currentOverlay) {
      await setAsActive(isActive ? null : currentOverlay.id);
    }
  };

  const handleDuplicate = async () => {
    if (currentOverlay) {
      await duplicateOverlay(currentOverlay.id);
    }
  };

  const handleDelete = async () => {
    if (currentOverlay && !currentOverlay.isDefault) {
      await deleteOverlay(currentOverlay.id);
    }
  };

  return (
    <>
      <header
        className="flex h-10 items-center justify-between border-b border-[#27272A] bg-[#18181B] pl-4 select-none"
        style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}
      >
        <div className="flex items-center gap-4" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
          {/* Logo with gradient glow */}
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-md bg-gradient-to-br from-[#00D9FF] to-[#A855F7] flex items-center justify-content-center" />
            <span className="text-lg font-bold tracking-wider text-[#FAFAFA]">OMNI</span>
          </div>

          {/* Divider */}
          <div className="h-6 w-px bg-[#27272A]" />

          {/* Overlay Selector */}
          <div className="flex items-center gap-2">
            <Select value={state.selectedOverlayId} onValueChange={handleSelectOverlay}>
              <SelectTrigger className="w-[220px] bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] hover:border-[#00D9FF]/50 transition-colors">
                <SelectValue placeholder="Select overlay" />
              </SelectTrigger>
              <SelectContent className="bg-[#18181B] border-[#27272A]">
                {state.overlays.map(overlay => (
                  <SelectItem key={overlay.id} value={overlay.id} className="text-[#FAFAFA] focus:bg-[#27272A] focus:text-[#FAFAFA]">
                    <div className="flex items-center gap-2">
                      <span>{overlay.name}</span>
                      {overlay.isDefault && (
                        <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-[#71717A] text-[#71717A]">
                          Default
                        </Badge>
                      )}
                      {overlay.id === state.activeOverlayId && (
                        <Badge className="text-[10px] px-1.5 py-0 bg-[#00D9FF] text-[#0D0D0F] hover:bg-[#00D9FF]">
                          Active
                        </Badge>
                      )}
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Overlay Actions */}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button 
                  variant="ghost" 
                  size="icon" 
                  className="h-9 w-9 text-[#71717A] hover:text-[#00D9FF] hover:bg-[#27272A] transition-colors"
                >
                  <Settings className="h-4 w-4" />
                  <span className="sr-only">Overlay options</span>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="bg-[#18181B] border-[#27272A]">
                <DropdownMenuItem onClick={() => setCreateDialogOpen(true)} className="text-[#FAFAFA] focus:bg-[#27272A] focus:text-[#00D9FF]">
                  <Plus className="mr-2 h-4 w-4" />
                  New Overlay
                </DropdownMenuItem>
                <DropdownMenuItem onClick={handleDuplicate} className="text-[#FAFAFA] focus:bg-[#27272A] focus:text-[#00D9FF]">
                  <Copy className="mr-2 h-4 w-4" />
                  Duplicate
                </DropdownMenuItem>
                <DropdownMenuSeparator className="bg-[#27272A]" />
                <DropdownMenuItem onClick={handleSetActive} className="text-[#FAFAFA] focus:bg-[#27272A] focus:text-[#00D9FF]">
                  <Star className={`mr-2 h-4 w-4 ${isActive ? 'fill-[#00D9FF] text-[#00D9FF]' : ''}`} />
                  {isActive ? 'Unset as Active' : 'Set as Active'}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => setGamesDialogOpen(true)} className="text-[#FAFAFA] focus:bg-[#27272A] focus:text-[#00D9FF]">
                  <Gamepad2 className="mr-2 h-4 w-4" />
                  Assign to Games
                </DropdownMenuItem>
                <DropdownMenuSeparator className="bg-[#27272A]" />
                <DropdownMenuItem
                  onClick={handleDelete}
                  disabled={isDefault}
                  className="text-[#EF4444] focus:bg-[#27272A] focus:text-[#EF4444]"
                >
                  <Trash2 className="mr-2 h-4 w-4" />
                  Delete
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>

        {/* Right side: status badges + window controls */}
        <div className="flex items-center h-full">
          <div className="flex items-center gap-2 pr-3" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
            {isDefault && (
              <Badge variant="outline" className="border-[#3B82F6]/50 text-[#3B82F6] bg-[#3B82F6]/10">
                Default Overlay
              </Badge>
            )}
            {isActive && !isDefault && (
              <Badge className="bg-[#00D9FF] text-[#0D0D0F] hover:bg-[#00D9FF]">
                Active Overlay
              </Badge>
            )}
            {state.isDirty && (
              <Badge className="bg-[#F59E0B]/20 text-[#F59E0B] border border-[#F59E0B]/30">
                Unsaved
              </Badge>
            )}
          </div>

          {/* Window controls */}
          <div className="flex h-full" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
            <button
              onClick={() => (window as any).omni?.minimizeWindow()}
              className="flex items-center justify-center w-11 h-full text-[#71717A] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
            >
              <Minus className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => (window as any).omni?.maximizeWindow()}
              className="flex items-center justify-center w-11 h-full text-[#71717A] hover:bg-[#27272A] hover:text-[#FAFAFA] transition-colors"
            >
              <Square className="h-3 w-3" />
            </button>
            <button
              onClick={() => (window as any).omni?.closeWindow()}
              className="flex items-center justify-center w-11 h-full text-[#71717A] hover:bg-[#EF4444] hover:text-[#FAFAFA] transition-colors"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </header>

      <CreateOverlayDialog open={createDialogOpen} onOpenChange={setCreateDialogOpen} />
      <GameAssignmentsDialog
        open={gamesDialogOpen}
        onOpenChange={setGamesDialogOpen}
        overlayId={currentOverlay?.id || ''}
      />
    </>
  );
}
