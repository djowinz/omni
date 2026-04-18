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

import { useState } from 'react';
import { Compass, Upload as UploadIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useExploreFilters, type ExploreTab } from '../../hooks/use-explore-filters';
import { useExploreList } from '../../hooks/use-explore-list';
import { ExploreSidebar } from './explore-sidebar';
import { ExploreGrid } from './explore-grid';
import { ExploreDetail } from './explore-detail';
import { ExploreEmptyState } from './explore-empty-state';
import { UploadDialog } from './upload-dialog';
import { MyUploadsView } from './my-uploads-view';

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

  const [uploadOpen, setUploadOpen] = useState(false);

  const handleUpload = () => {
    setUploadOpen(true);
  };

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Header mirrors the Components / Editor / Preview panel pattern:
          h-10 height, #18181B surface, zinc bottom border, colored section
          icon + label on the left, editor-style tabs with bottom-border active
          marker, and low-contrast secondary actions on the right. */}
      <header className="flex h-10 items-center border-b border-[#27272A] bg-[#18181B] overflow-x-auto">
        <div className="flex items-center gap-2 px-3 h-full">
          <Compass className="h-4 w-4 text-[#00D9FF]" />
          <h2 className="text-sm font-medium text-[#FAFAFA]">Explore</h2>
        </div>
        <nav className="flex h-full items-stretch">
          {SUBTABS.map((t) => (
            <button
              key={t.id}
              data-testid={`explore-subtab-${t.id}`}
              onClick={() => filters.setTab(t.id)}
              className={cn(
                'flex items-center gap-2 px-4 h-full text-xs border-l border-[#27272A] transition-colors whitespace-nowrap',
                filters.tab === t.id
                  ? 'bg-[#0D0D0F] text-[#FAFAFA] border-b-2 border-b-[#00D9FF]'
                  : 'text-[#71717A] hover:text-[#FAFAFA] hover:bg-[#27272A]/50',
              )}
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="flex-1" />
        <button
          data-testid="explore-upload-cta"
          onClick={handleUpload}
          className="flex items-center gap-1.5 px-3 h-full text-[10px] uppercase tracking-wider text-[#71717A] hover:text-[#FAFAFA] transition-colors"
          title="Publish a theme or bundle"
        >
          <UploadIcon className="h-3.5 w-3.5" aria-hidden />
          Upload
        </button>
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
          <MyUploadsView />
        )}
      </div>
      <UploadDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        sourcePath={null}
        mode="publish"
      />
    </div>
  );
}
