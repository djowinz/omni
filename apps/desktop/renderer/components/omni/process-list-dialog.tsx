import { useState, useMemo } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Search, Plus, X } from 'lucide-react';
import { cn } from '@/lib/utils';

interface ProcessListDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  processes: string[];
  onUpdate: (processes: string[]) => void;
}

const SYSTEM_PROCESSES = [
  'dwm',
  'explorer',
  'svchost',
  'lsass',
  'csrss',
  'winlogon',
  'services',
  'spoolsv',
  'taskmgr',
  'conhost',
  'cmd',
  'powershell',
  'pwsh',
  'notepad',
  'mspaint',
  'calc',
  'regedit',
  'mmc',
  'werfault',
  'searchhost',
  'runtimebroker',
  'sihost',
  'fontdrvhost',
  'audiodg',
  'lockapp',
  'applicationframehost',
  'textinputhost',
  'shellexperiencehost',
  'startmenuexperiencehost',
  'systemsettings',
  'widgets',
  'windowsterminal',
  'gamebar',
  'gamebarpresencewriter',
  'gamebarftserver',
];
const BROWSER_PROCESSES = ['chrome', 'firefox', 'msedge', 'opera', 'brave'];
const LAUNCHER_PROCESSES = [
  'steam',
  'steamwebhelper',
  'epicgameslauncher',
  'eadesktop',
  'origin',
  'galaxyclient',
  'gogalaxy',
  'upc',
  'battlenet',
];
const DEVTOOL_PROCESSES = ['code', 'blender', 'devenv', 'rider', 'idea', 'notepad++'];
const GPU_PROCESSES = [
  'nvcontainer',
  'nvdisplay',
  'nvidia overlay',
  'nvidia share',
  'nvoawrappercache',
  'nvsphelper64',
  'nvspcaps64',
  'amdow',
  'amddvr',
  'radeonoverlay',
  'msiafterburner',
  'rtss',
  'hwinfo64',
  'hwinfo32',
];

function categorize(name: string): string {
  const base = name.toLowerCase().replace(/\.exe$/, '');
  if (SYSTEM_PROCESSES.includes(base)) return 'System';
  if (BROWSER_PROCESSES.includes(base)) return 'Browsers';
  if (LAUNCHER_PROCESSES.includes(base)) return 'Launchers';
  if (DEVTOOL_PROCESSES.includes(base)) return 'Dev Tools';
  if (GPU_PROCESSES.some((g) => base.includes(g))) return 'GPU/Monitoring';
  return 'Custom';
}

const ALL_CATEGORIES = [
  'All',
  'System',
  'Browsers',
  'Launchers',
  'Dev Tools',
  'GPU/Monitoring',
  'Custom',
];

export function ProcessListDialog({
  open,
  onOpenChange,
  title,
  processes,
  onUpdate,
}: ProcessListDialogProps) {
  const [search, setSearch] = useState('');
  const [activeCategory, setActiveCategory] = useState('All');
  const [addingNew, setAddingNew] = useState(false);
  const [newProcess, setNewProcess] = useState('');

  const categorized = useMemo(
    () => processes.map((p) => ({ name: p, category: categorize(p) })),
    [processes],
  );

  const filtered = useMemo(() => {
    let items = categorized;
    if (activeCategory !== 'All') {
      items = items.filter((p) => p.category === activeCategory);
    }
    if (search) {
      const q = search.toLowerCase();
      items = items.filter((p) => p.name.toLowerCase().includes(q));
    }
    return items.sort((a, b) => a.name.localeCompare(b.name));
  }, [categorized, activeCategory, search]);

  const handleRemove = (name: string) => {
    onUpdate(processes.filter((p) => p !== name));
  };

  const handleAdd = () => {
    if (!newProcess.trim()) return;
    let name = newProcess.trim().toLowerCase();
    if (!name.endsWith('.exe')) name += '.exe';
    if (!processes.includes(name)) {
      onUpdate([...processes, name]);
    }
    setNewProcess('');
    setAddingNew(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-[#18181B] border-[#27272A] p-0 gap-0 overflow-hidden sm:max-w-2xl">
        <DialogHeader className="px-5 pt-5 pb-3">
          <DialogTitle className="text-[#FAFAFA]">{title}</DialogTitle>
        </DialogHeader>

        {/* Search */}
        <div className="px-4 pb-2">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-[#52525B]" />
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search processes..."
              className="pl-9 bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] h-8 text-xs"
            />
          </div>
        </div>

        {/* Category tabs */}
        <div className="flex gap-0 px-4 border-b border-[#27272A]">
          {ALL_CATEGORIES.map((cat) => (
            <button
              key={cat}
              onClick={() => setActiveCategory(cat)}
              className={cn(
                'px-3 py-2 text-[11px] font-medium border-b-2 whitespace-nowrap transition-colors',
                activeCategory === cat
                  ? 'text-[#00D9FF] border-b-[#00D9FF]'
                  : 'text-[#52525B] border-b-transparent hover:text-[#A1A1AA]',
              )}
            >
              {cat}
            </button>
          ))}
        </div>

        {/* Process list */}
        <div className="px-3 py-2 max-h-[400px] overflow-y-auto">
          {addingNew && (
            <div className="flex items-center gap-2 px-2 py-1.5 mb-1">
              <Input
                value={newProcess}
                onChange={(e) => setNewProcess(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleAdd();
                  if (e.key === 'Escape') {
                    setAddingNew(false);
                    setNewProcess('');
                  }
                }}
                placeholder="process.exe"
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
          <div className="grid grid-cols-2 gap-x-2">
            {filtered.map((item) => (
              <div
                key={item.name}
                className="group flex items-center justify-between px-2 py-1.5 rounded hover:bg-[#27272A] transition-colors"
              >
                <div className="flex items-center gap-2 min-w-0">
                  <span className="font-mono text-xs text-[#A1A1AA] truncate">{item.name}</span>
                  <span className="text-[10px] text-[#52525B] shrink-0">{item.category}</span>
                </div>
                <button
                  onClick={() => handleRemove(item.name)}
                  className="opacity-0 group-hover:opacity-100 text-[#52525B] hover:text-[#EF4444] transition-all shrink-0"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            ))}
          </div>
          {filtered.length === 0 && (
            <div className="text-center text-[#52525B] text-xs py-8">No processes found</div>
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
            Add Process
          </Button>
          <span className="text-[10px] text-[#52525B]">{processes.length} processes</span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
