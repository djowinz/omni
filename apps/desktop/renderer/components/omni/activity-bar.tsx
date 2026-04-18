import { Compass, Puzzle, Settings } from 'lucide-react';
import { useRouter } from 'next/router';
import { useOmniState } from '@/hooks/use-omni-state';
import { cn } from '@/lib/utils';

// Top slot: editor-first tabs (Components). Bottom slots: system/sharing (Explore above Settings).
const topTabs = [{ id: 'components' as const, icon: Puzzle, label: 'Components' }];
const bottomTabs = [
  { id: 'explore' as const, icon: Compass, label: 'Explore' },
  { id: 'settings' as const, icon: Settings, label: 'Settings' },
];

type TabId = 'components' | 'settings' | 'explore';

export function ActivityBar() {
  const { state, dispatch } = useOmniState();
  const router = useRouter();

  const handleTabClick = (tabId: TabId) => {
    dispatch({ type: 'SET_ACTIVE_PANEL', payload: tabId });
    // If switching to Components while on the logs page, navigate back to home
    if (tabId === 'components' && router.pathname === '/logs') {
      router.push('/home');
    }
  };

  const renderTab = (tab: { id: TabId; icon: typeof Puzzle; label: string }) => (
    <button
      key={tab.id}
      data-testid={`activity-bar-tab-${tab.id}`}
      onClick={() => handleTabClick(tab.id)}
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
  );

  return (
    <div className="flex w-11 flex-shrink-0 flex-col border-r border-[#27272A] bg-[#141416]">
      <div className="flex flex-col">{topTabs.map(renderTab)}</div>
      <div className="mt-auto flex flex-col">{bottomTabs.map(renderTab)}</div>
    </div>
  );
}
