/**
 * ExplorePanel — top-level Explore composition.
 *
 * Layout per share-explorer-redesign spec §2:
 *   +- Header h-10 -------------------------------------------------+
 *   | Compass icon · "Explore"                       + Upload CTA   |
 *   +- Body -------------------------------------------------------+
 *   |                                                              |
 *   |  Sidebar (260px)  |  Grid (flex-1)  |  Detail (380px,        |
 *   |                   |                 |    mounted only when    |
 *   |                   |                 |    selectedId !== null)  |
 *   +--------------------------------------------------------------+
 *
 * Tab semantics:
 *   - discover    → useExploreList(filters)
 *   - installed   → useExploreList (returns empty until #016 wires registry)
 *   - my-uploads  → useMyUploads(filters)  [identity-derived author filter]
 *
 * MyUploadsView + IdentitySummaryCard are deleted; identity surfaces in
 * the titlebar IdentityChip + Settings IdentitySection per #016.
 */

import { useState } from 'react';
import { Compass, Upload as UploadIcon } from 'lucide-react';
import { useExploreFilters } from '../../hooks/use-explore-filters';
import { useExploreList } from '../../hooks/use-explore-list';
import { useMyUploads } from '../../hooks/use-my-uploads';
import { useShareWs } from '../../hooks/use-share-ws';
import { ExploreSidebar } from './explore-sidebar';
import { ExploreGrid } from './explore-grid';
import { ExploreDetail } from './explore-detail';
import { UploadDialog } from './upload-dialog';
import { toast } from '../../lib/toast';
import type { CachedArtifactDetail } from '../../lib/share-types';

export function ExplorePanel() {
  const filters = useExploreFilters();
  const { send } = useShareWs();

  const discoverList = useExploreList({
    tab: filters.tab,
    kind: filters.kind,
    sort: filters.sort,
    tags: filters.tags,
    q: filters.q,
  });
  const myUploads = useMyUploads();

  const list = filters.tab === 'my-uploads' ? myUploads : discoverList;

  const [uploadOpen, setUploadOpen] = useState(false);

  // Hover Install on a card: kicks off the install for that artifact's id.
  // The detail-pane's full state-machine (in-flight progress, TOFU mismatch
  // dialog, retry) is not driven from here — clicking Install on the card
  // simply selects the artifact AND starts the install; if TOFU mismatch
  // happens or progress needs tracking, the user has the detail pane open
  // by then. This is the v1 behavior per spec out-of-band follow-ups.
  const handleHoverInstall = async (a: CachedArtifactDetail) => {
    filters.setSelectedId(a.artifact_id);
    try {
      await send('explorer.install', { artifact_id: a.artifact_id });
      toast.success(`Installed ${a.name}`);
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  const handleHoverPreview = async (a: CachedArtifactDetail) => {
    try {
      await send('explorer.preview', { artifact_id: a.artifact_id });
      toast.info(`Previewing ${a.name}`);
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      <header className="flex h-10 flex-shrink-0 items-center border-b border-[#27272A] bg-[#18181B]">
        <div className="flex h-full items-center gap-2 px-3">
          <Compass className="h-4 w-4 text-[#00D9FF]" />
          <h2 className="text-sm font-medium text-[#FAFAFA]">Explore</h2>
        </div>
        <div className="flex-1" />
        <button
          data-testid="explore-upload-cta"
          onClick={() => setUploadOpen(true)}
          className="flex h-full items-center gap-1.5 px-3 text-[10px] uppercase tracking-wider text-[#71717A] transition-colors hover:text-[#FAFAFA]"
          title="Publish a theme or bundle"
        >
          <UploadIcon className="h-3.5 w-3.5" aria-hidden />
          Upload
        </button>
      </header>

      <div className="flex flex-1 overflow-hidden">
        <ExploreSidebar />
        <main className="flex-1 overflow-hidden">
          <ExploreGrid
            items={list.items}
            loading={list.loading}
            hasMore={list.nextCursor !== null}
            selectedId={filters.selectedId}
            onSelect={filters.setSelectedId}
            onPreview={handleHoverPreview}
            onInstall={handleHoverInstall}
            onLoadMore={list.loadMore}
          />
        </main>
        {filters.selectedId !== null && (
          <aside className="w-[380px] flex-shrink-0 border-l border-[#27272A]">
            <ExploreDetail selectedId={filters.selectedId} tab={filters.tab} />
          </aside>
        )}
      </div>

      <UploadDialog open={uploadOpen} onOpenChange={setUploadOpen} prefilledPath={null} />
    </div>
  );
}
