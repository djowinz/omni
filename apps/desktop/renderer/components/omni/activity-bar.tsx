import { Puzzle, Settings } from 'lucide-react';
import { useOmniState } from '@/hooks/use-omni-state';
import { cn } from '@/lib/utils';

const tabs = [
  { id: 'components' as const, icon: Puzzle, label: 'Components' },
  { id: 'settings' as const, icon: Settings, label: 'Settings' },
];

export function ActivityBar() {
  const { state, dispatch } = useOmniState();

  return (
    <div className="flex w-11 flex-shrink-0 flex-col border-r border-[#27272A] bg-[#141416]">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          onClick={() => dispatch({ type: 'SET_ACTIVE_PANEL', payload: tab.id })}
          className={cn(
            'flex h-10 w-full items-center justify-center border-l-2 transition-colors',
            state.activePanel === tab.id
              ? 'border-l-[#00D9FF] bg-[#27272A]/40 text-[#FAFAFA]'
              : 'border-l-transparent text-[#52525B] hover:text-[#A1A1AA] hover:bg-[#27272A]/30',
          )}
          title={tab.label}
        >
          <tab.icon className="h-4 w-4" />
        </button>
      ))}
    </div>
  );
}
