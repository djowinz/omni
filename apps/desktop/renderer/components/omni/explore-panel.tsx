/**
 * ExplorePanel — top-level composition of the Explore tab.
 *
 * Layout:
 *   +- Header ----------------------------------------------------+
 *   | [Discover] [Installed] [My Uploads]        [+ Upload]       |
 *   +----------+------------------------------+------------------+
 *   | Sidebar  |            Grid              |    Detail        |
 *   | 240px    |         (flex-1)             |     260px        |
 *   +----------+------------------------------+------------------+
 *
 * Sub-tabs other than Discover render a full-width empty-state (no sidebar,
 * no grid) in Wave 3b — Installed needs a WS bridge that isn't shipped, and
 * My Uploads depends on #015's upload.list flow.
 */

import { Plus, Upload as UploadIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useExploreFilters, type ExploreTab } from '../../hooks/use-explore-filters';
import { useExploreList } from '../../hooks/use-explore-list';
import { toast } from '../../lib/toast';
import { ExploreSidebar } from './explore-sidebar';
import { ExploreGrid } from './explore-grid';
import { ExploreDetail } from './explore-detail';
import { ExploreEmptyState } from './explore-empty-state';

const SUBTABS: { id: ExploreTab; label: string }[] = [
  { id: 'discover', label: 'Discover' },
  { id: 'installed', label: 'Installed' },
  { id: 'my-uploads', label: 'My Uploads' },
];

export function ExplorePanel() {
  const filters = useExploreFilters();
  const list = useExploreList({
    tab: filters.tab,
    kind: filters.kind,
    sort: filters.sort,
    tags: filters.tags,
    q: filters.q,
  });

  const handleUpload = () => {
    toast.info('Upload dialog lands in sub-spec #015.');
  };

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      <header className="flex items-center justify-between border-b border-[#27272A] px-4 py-2">
        <nav className="flex items-center gap-1">
          {SUBTABS.map((t) => (
            <button
              key={t.id}
              data-testid={`explore-subtab-${t.id}`}
              onClick={() => filters.setTab(t.id)}
              className={cn(
                'rounded-md px-3 py-1.5 text-sm transition-colors',
                filters.tab === t.id
                  ? 'bg-[#27272A] text-[#FAFAFA]'
                  : 'text-zinc-400 hover:bg-[#27272A]/50 hover:text-zinc-200',
              )}
            >
              {t.label}
            </button>
          ))}
        </nav>
        <Button data-testid="explore-upload-cta" size="sm" onClick={handleUpload} className="gap-1">
          <Plus className="h-4 w-4" aria-hidden />
          <UploadIcon className="h-4 w-4" aria-hidden />
          Upload
        </Button>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {filters.tab === 'discover' ? (
          <>
            <ExploreSidebar />
            <main className="flex-1 overflow-hidden">
              <ExploreGrid
                items={list.items}
                loading={list.loading}
                hasMore={list.nextCursor !== null}
                selectedId={filters.selectedId}
                onSelect={filters.setSelectedId}
                onLoadMore={list.loadMore}
                emptyLabel="No artifacts match these filters."
                emptyHint="Adjust Kind or Tags to broaden your search."
              />
            </main>
            <aside className="w-64 flex-shrink-0 border-l border-[#27272A] bg-[#141416]">
              <ExploreDetail selectedId={filters.selectedId} tab={filters.tab} />
            </aside>
          </>
        ) : filters.tab === 'installed' ? (
          <ExploreEmptyState
            label="Nothing installed yet."
            hint="Head to Discover to browse themes and bundles."
          />
        ) : (
          <ExploreEmptyState
            label="You haven't published anything yet."
            hint="Click + Upload to share your first theme or bundle."
          />
        )}
      </div>
    </div>
  );
}
