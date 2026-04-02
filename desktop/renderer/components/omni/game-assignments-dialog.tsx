


import { useState } from 'react';
import { Trash2, Plus, Gamepad2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { useOmniState } from '@/hooks/use-omni-state';

interface GameAssignmentsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  overlayName: string;
}

export function GameAssignmentsDialog({
  open,
  onOpenChange,
  overlayName,
}: GameAssignmentsDialogProps) {
  const { state, assignToGame, removeGameAssignment } = useOmniState();
  const [newExecutable, setNewExecutable] = useState('');

  // Get assignments for this overlay from config
  const assignments = Object.entries(state.config?.overlay_by_game ?? {})
    .filter(([, name]) => name === overlayName)
    .map(([executable]) => executable);

  const handleAddGame = async () => {
    if (!newExecutable.trim()) return;

    const executable = newExecutable.trim().toLowerCase();
    const normalizedExe = executable.endsWith('.exe') ? executable : `${executable}.exe`;

    await assignToGame(overlayName, normalizedExe);
    setNewExecutable('');
  };

  const handleRemoveGame = async (executable: string) => {
    await removeGameAssignment(executable);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && newExecutable.trim()) {
      handleAddGame();
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[500px] bg-[#18181B] border-[#27272A] text-[#FAFAFA]">
        <DialogHeader>
          <DialogTitle className="text-[#FAFAFA] flex items-center gap-2">
            <Gamepad2 className="h-5 w-5 text-[#A855F7]" />
            Game Assignments
          </DialogTitle>
          <DialogDescription className="text-[#71717A]">
            Assign this overlay to specific game executables. Per-game overlays take highest priority.
          </DialogDescription>
        </DialogHeader>

        <div className="py-4">
          {/* Add new game */}
          <div className="flex gap-2">
            <Input
              value={newExecutable}
              onChange={e => setNewExecutable(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="game.exe"
              className="flex-1 bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] placeholder:text-[#52525B] focus:ring-[#A855F7] focus:border-[#A855F7] font-mono"
            />
            <Button
              onClick={handleAddGame}
              disabled={!newExecutable.trim()}
              className="bg-[#A855F7] text-white hover:bg-[#A855F7]/90 disabled:bg-[#27272A] disabled:text-[#52525B]"
            >
              <Plus className="mr-2 h-4 w-4" />
              Add
            </Button>
          </div>

          {/* List of assigned games */}
          <div className="mt-4">
            <h4 className="mb-2 text-xs font-medium text-[#71717A] uppercase tracking-wider">
              Assigned Games ({assignments.length})
            </h4>
            {assignments.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-8 text-center rounded-lg border border-dashed border-[#27272A]">
                <Gamepad2 className="h-8 w-8 text-[#52525B] mb-2" />
                <p className="text-sm text-[#71717A]">No games assigned yet</p>
                <p className="text-xs text-[#52525B] mt-1">Add a game executable above</p>
              </div>
            ) : (
              <ScrollArea className="h-[200px]">
                <div className="space-y-1">
                  {assignments.map(executable => (
                    <div
                      key={executable}
                      className="flex items-center justify-between rounded-lg bg-[#0D0D0F] border border-[#27272A] px-3 py-2 group hover:border-[#A855F7]/30 transition-colors"
                    >
                      <span className="font-mono text-sm text-[#FAFAFA]">{executable}</span>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-[#52525B] hover:text-[#EF4444] hover:bg-[#EF4444]/10"
                        onClick={() => handleRemoveGame(executable)}
                      >
                        <Trash2 className="h-4 w-4" />
                        <span className="sr-only">Remove {executable}</span>
                      </Button>
                    </div>
                  ))}
                </div>
              </ScrollArea>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
