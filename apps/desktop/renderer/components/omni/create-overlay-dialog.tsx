import { useState } from 'react';
import { Plus } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useOmniState } from '@/hooks/use-omni-state';

interface CreateOverlayDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function CreateOverlayDialog({ open, onOpenChange }: CreateOverlayDialogProps) {
  const { createOverlay } = useOmniState();
  const [name, setName] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const handleCreate = async () => {
    if (!name.trim()) return;

    setIsCreating(true);
    try {
      await createOverlay(name.trim());
      setName('');
      onOpenChange(false);
    } finally {
      setIsCreating(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && name.trim()) {
      handleCreate();
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[400px] bg-[#18181B] border-[#27272A] text-[#FAFAFA]">
        <DialogHeader>
          <DialogTitle className="text-[#FAFAFA] flex items-center gap-2">
            <Plus className="h-5 w-5 text-[#00D9FF]" />
            Create New Overlay
          </DialogTitle>
          <DialogDescription className="text-[#71717A]">
            Create a new overlay with a basic widget template. You can customize it in the editor.
          </DialogDescription>
        </DialogHeader>
        <div className="py-4">
          <Label htmlFor="overlay-name" className="text-sm font-medium text-[#A1A1AA]">
            Overlay Name
          </Label>
          <Input
            id="overlay-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="My Custom Overlay"
            className="mt-2 bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] placeholder:text-[#52525B] focus:ring-[#00D9FF] focus:border-[#00D9FF]"
            autoFocus
          />
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            className="bg-transparent border-[#27272A] text-[#A1A1AA] hover:bg-[#27272A] hover:text-[#FAFAFA]"
          >
            Cancel
          </Button>
          <Button
            onClick={handleCreate}
            disabled={!name.trim() || isCreating}
            className="bg-[#00D9FF] text-[#0D0D0F] hover:bg-[#00D9FF]/90 disabled:bg-[#27272A] disabled:text-[#52525B]"
          >
            {isCreating ? 'Creating...' : 'Create Overlay'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
