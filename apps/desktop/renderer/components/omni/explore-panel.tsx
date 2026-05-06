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

import { useMemo, useState } from 'react';
import { Compass, Upload as UploadIcon } from 'lucide-react';
import { useExploreFilters } from '../../hooks/use-explore-filters';
import { useExploreList } from '../../hooks/use-explore-list';
import { useMyUploads } from '../../hooks/use-my-uploads';
import { useShareWs } from '../../hooks/use-share-ws';
import { useOmniState } from '../../hooks/use-omni-state';
import { useInstalledArtifactIds } from '../../hooks/use-installed-artifact-ids';
import { useInstalledDetails } from '../../hooks/use-installed-details';
import { ExploreSidebar } from './explore-sidebar';
import { ExploreGrid } from './explore-grid';
import { ExploreDetail } from './explore-detail';
import { UninstallConfirmDialog } from './uninstall-confirm-dialog';
import { UploadDialog } from './upload-dialog';
import { toast } from '../../lib/toast';
import type { CachedArtifactDetail } from '../../lib/share-types';

export function ExplorePanel() {
  const filters = useExploreFilters();
  const { send } = useShareWs();
  const { refreshOverlays } = useOmniState();
  const installed = useInstalledArtifactIds();

  // (Cross-surface sync via `omni:artifact-installed` /
  // `omni:artifact-uninstalled` window events lives inside
  // `useInstalledArtifacts` itself now — every consumer auto-refetches
  // when either event fires, so the panel doesn't need its own listener.)

  const discoverList = useExploreList({
    tab: filters.tab,
    kind: filters.kind,
    sort: filters.sort,
    tags: filters.tags,
    q: filters.q,
  });
  const myUploads = useMyUploads();

  // Map local InstalledEntryRow → CachedArtifactDetail-shaped rows the grid
  // card expects. The registry stores ONLY local install state; display-only
  // fields (thumbnail, install count, tags) come from the batch live-fetch
  // below so cards see current install counts instead of a snapshot from
  // install time. Cards render immediately with name/kind/author from the
  // registry (works offline) and re-render with thumbnails + counts once
  // the batch returns. Network failure → cards stay with the placeholder.
  const installedDetails = useInstalledDetails(installed.entries);
  // Tombstoned-id set: artifacts whose upstream row has `is_removed = 1`.
  // Discover never sees these (server filters), so this is sourced solely
  // from the live batch fetch above. Used by the Installed grid to flip
  // the green "Installed" pill to amber "Removed upstream", and by the
  // detail pane (via the same status field) to surface the warning banner.
  const tombstonedIds = useMemo(() => {
    const out = new Set<string>();
    for (const detail of installedDetails.byId.values()) {
      if (detail.status === 'tombstoned') out.add(detail.artifact_id);
    }
    return out;
  }, [installedDetails.byId]);
  const installedItems = useMemo<CachedArtifactDetail[]>(
    () =>
      installed.entries.map((e) => {
        const live = installedDetails.byId.get(e.artifact_id);
        return {
          artifact_id: e.artifact_id,
          content_hash: e.content_hash,
          author_pubkey: e.author_pubkey,
          author_fingerprint_hex: e.author_fingerprint_hex,
          name: e.name,
          kind: e.kind,
          tags: [],
          installs: live?.installs ?? 0,
          r2_url: live?.r2_url ?? '',
          thumbnail_url: live?.thumbnail_url ?? '',
          created_at: live?.created_at ?? 0,
          updated_at: live?.updated_at ?? e.installed_at,
          author_display_name: live?.author_display_name ?? null,
        };
      }),
    [installed.entries, installedDetails.byId],
  );

  const installedList = useMemo(
    () => ({
      items: installedItems,
      loading: installed.loading,
      nextCursor: null as string | null,
      loadMore: () => Promise.resolve(),
    }),
    [installedItems, installed.loading],
  );

  const list =
    filters.tab === 'my-uploads'
      ? myUploads
      : filters.tab === 'installed'
        ? installedList
        : discoverList;

  const [uploadOpen, setUploadOpen] = useState(false);
  // Uninstall flow lives at the panel level so all surfaces (detail-pane
  // middle button + grid hover button) share one dialog mount. The state
  // is `null` when no uninstall is in progress; setting it to a target
  // opens the confirm dialog. Post-success we refetch installed + refresh
  // overlays directly here, then null out the target. This replaces the
  // earlier window-event hop, which had a subtle staleness bug: the dialog
  // was mounted inside ExploreDetail, and unmounting the pane after
  // `setSelectedId(null)` could race the dispatch and leave the listener
  // calling refetch on an already-detached set.
  const [uninstallTarget, setUninstallTarget] = useState<{
    artifact_id: string;
    name: string;
  } | null>(null);

  // Hover Uninstall on an installed card: open the panel-level dialog with
  // the card's name pre-filled. The grid only wires this when a card is
  // installed — see `pickHoverHandlers` in explore-grid.tsx.
  const handleHoverUninstall = (a: CachedArtifactDetail) => {
    setUninstallTarget({ artifact_id: a.artifact_id, name: a.name });
  };

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
      // Re-scan the workspace so the new <data_dir>/overlays/<id>/ folder
      // shows up in the header dropdown. Without this the user has to
      // hard-refresh the app to see what they just installed.
      await refreshOverlays();
      // Notify other surfaces (header, editor) so their useInstalledArtifacts
      // instances refetch — they listen for this window event. The panel's
      // own copy refetches via the same listener (in the hook) automatically.
      window.dispatchEvent(new CustomEvent('omni:artifact-installed'));
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
            installedIds={installed.ids}
            tombstonedIds={tombstonedIds}
            onSelect={filters.setSelectedId}
            onPreview={handleHoverPreview}
            onInstall={handleHoverInstall}
            onUninstall={handleHoverUninstall}
            onLoadMore={list.loadMore}
          />
        </main>
        {filters.selectedId !== null && (
          <aside className="w-[380px] flex-shrink-0 border-l border-[#27272A]">
            <ExploreDetail
              selectedId={filters.selectedId}
              tab={filters.tab}
              installed={installed.ids.has(filters.selectedId)}
              onRequestUninstall={(artifact_id, name) =>
                setUninstallTarget({ artifact_id, name })
              }
            />
          </aside>
        )}
      </div>

      {uninstallTarget && (
        <UninstallConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setUninstallTarget(null);
          }}
          artifactId={uninstallTarget.artifact_id}
          artifactName={uninstallTarget.name}
          onUninstalled={() => {
            // On Installed tab, also close the pane if it's showing the
            // just-uninstalled row (the row vanishes from the grid; an
            // open pane pointing at it would mismatch the tab's contents).
            // On Discover the pane stays open so the user can one-click
            // re-install — its middle button label flips back to "Install"
            // as soon as the registry refetch lands. The window event
            // triggers refetch in *every* useInstalledArtifacts instance
            // (panel, header, editor) so all surfaces stay synced.
            const removedId = uninstallTarget.artifact_id;
            void refreshOverlays();
            window.dispatchEvent(new CustomEvent('omni:artifact-uninstalled'));
            setUninstallTarget(null);
            if (filters.selectedId === removedId && filters.tab === 'installed') {
              filters.setSelectedId(null);
            }
          }}
        />
      )}

      <UploadDialog open={uploadOpen} onOpenChange={setUploadOpen} prefilledPath={null} />
    </div>
  );
}
