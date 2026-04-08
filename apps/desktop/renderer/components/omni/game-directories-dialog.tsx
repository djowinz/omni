import { useState, useMemo } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Search, Plus, X, FolderOpen } from 'lucide-react';

interface GameDirectoriesDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  directories: string[];
  onUpdate: (directories: string[]) => void;
}

export function GameDirectoriesDialog({
  open,
  onOpenChange,
  directories,
  onUpdate,
}: GameDirectoriesDialogProps) {
  const [search, setSearch] = useState('');
  const [addingNew, setAddingNew] = useState(false);
  const [newDirectory, setNewDirectory] = useState('');

  const filtered = useMemo(() => {
    if (!search) return [...directories].sort((a, b) => a.localeCompare(b));
    const q = search.toLowerCase();
    return directories
      .filter((d) => d.toLowerCase().includes(q))
      .sort((a, b) => a.localeCompare(b));
  }, [directories, search]);

  const handleRemove = (dir: string) => {
    onUpdate(directories.filter((d) => d !== dir));
  };

  const handleAdd = () => {
    if (!newDirectory.trim()) return;
    const dir = newDirectory.trim();
    if (!directories.includes(dir)) {
      onUpdate([...directories, dir]);
    }
    setNewDirectory('');
    setAddingNew(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-[#18181B] border-[#27272A] p-0 gap-0 overflow-hidden sm:max-w-2xl">
        <DialogHeader className="px-5 pt-5 pb-3">
          <DialogTitle className="text-[#FAFAFA]">Game Directories</DialogTitle>
        </DialogHeader>

        {/* Search */}
        <div className="px-4 pb-2">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-[#52525B]" />
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search directories..."
              className="pl-9 bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] h-8 text-xs"
            />
          </div>
        </div>

        <div className="border-b border-[#27272A]" />

        {/* Directory list */}
        <div className="px-3 py-2 max-h-[400px] overflow-y-auto">
          {addingNew && (
            <div className="flex items-center gap-2 px-2 py-1.5 mb-1">
              <Input
                value={newDirectory}
                onChange={(e) => setNewDirectory(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleAdd();
                  if (e.key === 'Escape') {
                    setAddingNew(false);
                    setNewDirectory('');
                  }
                }}
                placeholder="steamapps\common\"
                className="h-7 bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] text-xs font-mono flex-1"
                autoFocus
              />
              <Button
                size="sm"
                onClick={handleAdd}
                className="h-7 px-2 bg-[#A855F7] hover:bg-[#A855F7]/80 text-white text-xs"
              >
                Add
              </Button>
            </div>
          )}
          <div className="space-y-0">
            {filtered.map((dir) => (
              <div
                key={dir}
                className="group flex items-center justify-between px-2 py-1.5 rounded hover:bg-[#27272A] transition-colors"
              >
                <div className="flex items-center gap-2 min-w-0">
                  <FolderOpen className="h-3.5 w-3.5 text-[#52525B] shrink-0" />
                  <span className="font-mono text-xs text-[#A1A1AA] truncate">{dir}</span>
                </div>
                <button
                  onClick={() => handleRemove(dir)}
                  className="opacity-0 group-hover:opacity-100 text-[#52525B] hover:text-[#EF4444] transition-all shrink-0"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            ))}
          </div>
          {filtered.length === 0 && (
            <div className="text-center text-[#52525B] text-xs py-8">No directories found</div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-[#27272A]">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setAddingNew(true)}
            className="text-[#A855F7] hover:text-[#A855F7] hover:bg-[#A855F7]/10 text-xs h-7 gap-1"
          >
            <Plus className="h-3 w-3" />
            Add Directory
          </Button>
          <span className="text-[10px] text-[#52525B]">{directories.length} directories</span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
