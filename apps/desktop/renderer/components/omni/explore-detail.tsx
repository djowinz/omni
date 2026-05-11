/**
 * ExploreDetail — 380px right detail pane.
 *
 * Per share-explorer-redesign spec §3.4: structured header (h-16) +
 * scrollable body + sticky footer. Replaces the previous
 * <ArtifactCard variant='detail'> usage entirely. The 'detail' variant
 * is retired in Task 8.
 *
 * The pane is mounted only when selectedId !== null. Clicking the header's
 * ✕ calls filters.setSelectedId(null), which unmounts the pane (cards
 * widen to absorb the freed space — column count stays at 3).
 */

import { useMemo, useState } from 'react';
import { AlertTriangle, Calendar, CheckSquare, Clock, Download, Layers, MoreVertical, Palette, Tag as TagIcon, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { InstallProgress, type InstallPhase } from './install-progress';
import { TofuMismatchDialog, type TofuFingerprint } from './tofu-mismatch-dialog';
import { ForkDialog } from './fork-dialog';
import { InlineError } from './inline-error';
import { UpdateAvailablePill } from './update-available-pill';
import { UpdateConfirmDialog } from './update-confirm-dialog';
import { useExploreDetail } from '../../hooks/use-explore-detail';
import { useExploreFilters } from '../../hooks/use-explore-filters';
import { usePreview } from '../../lib/preview-context';
import { useShareWs } from '../../hooks/use-share-ws';
import { useOmniState } from '../../hooks/use-omni-state';
import { useIdentity } from '../../lib/identity-context';
import { toast } from '../../lib/toast';
import { mapErrorToUserMessage, type OmniError } from '../../lib/map-error-to-user-message';
import {
  actionLabelsFor,
  installFolderName,
  installFolderPath,
  kebabLabelsFor,
  type ExploreTab,
} from '../../lib/artifact-actions';
import type { UpdateStatus } from '../../hooks/use-artifact-update-status';
import type { InstalledEntryRow } from '../../lib/share-types';

export interface ExploreDetailProps {
  selectedId: string;
  tab: ExploreTab;
  /** True when the selected artifact is in the local install registry.
   *  Used to flip Discover's middle slot from "Install" → "Uninstall"
   *  (destructive variant) so the user doesn't see a no-op Install button
   *  on something they've already installed. */
  installed?: boolean;
  /** Full installed-registry rows from the panel. Keyed lookup by
   *  artifact_id supplies the `installed` argument to `<UpdateConfirmDialog>`
   *  so it can render the version delta + author-key rotation warning. */
  installedEntries?: InstalledEntryRow[];
  /** Per-artifact update-detection result computed by the panel via
   *  `useArtifactUpdateStatus(installed.entries, installedDetails.byId)`.
   *  Drives the header pill + confirm dialog. */
  updateStatus?: Map<string, UpdateStatus>;
  /** Opens the panel-level uninstall confirm dialog with the supplied
   *  display name. The panel owns the dialog mount + the post-success
   *  refetch / overlay refresh / pane-close logic so all uninstall
   *  surfaces (detail-pane button + grid hover button) share one flow. */
  onRequestUninstall?: (artifactId: string, name: string) => void;
  /** Author-side Update CTA on the my-uploads tab. Detail-pane resolves the
   *  workspace path via `publish.lookupWorkspace` and then asks the panel
   *  to open <UploadDialog> pre-filled with it. */
  onRequestUpload?: (workspace_path: string) => void;
}

type InstallState =
  | { kind: 'idle' }
  | { kind: 'in-flight'; phase: InstallPhase; done: number; total: number }
  | { kind: 'error'; message: string };

function manifestString(manifest: Record<string, unknown> | undefined, key: string): string | null {
  if (!manifest) return null;
  const v = manifest[key];
  return typeof v === 'string' && v.length > 0 ? v : null;
}

export function ExploreDetail({
  selectedId,
  tab,
  installed = false,
  installedEntries,
  updateStatus,
  onRequestUninstall,
  onRequestUpload,
}: ExploreDetailProps) {
  const { artifact, loading } = useExploreDetail(selectedId);
  const { setSelectedId } = useExploreFilters();
  const { setPreview } = usePreview();
  const { send } = useShareWs();
  const { state: omniState, dispatch, refreshOverlays, ensureOverlayLoaded } = useOmniState();
  const { identity } = useIdentity();

  const [installState, setInstallState] = useState<InstallState>({ kind: 'idle' });
  const [tofuOpen, setTofuOpen] = useState(false);
  const [tofuPair, setTofuPair] = useState<{
    previously: TofuFingerprint;
    incoming: TofuFingerprint;
  } | null>(null);
  const [forkOpen, setForkOpen] = useState(false);
  // Update-confirm dialog state. Opened from the header pill and from the
  // (future) corner pill click bubble path. Closes silently on Apply success;
  // the install pipeline fires `omni:artifact-installed` which the
  // useInstalledArtifacts listener picks up — registry + grid + pill all
  // refresh without us doing anything else here.
  const [confirmOpen, setConfirmOpen] = useState(false);

  const existingNames = useMemo(
    () => omniState.overlays.map((o) => o.name),
    [omniState.overlays],
  );
  const selfHandle = identity
    ? identity.display_name
      ? `${identity.display_name}#${identity.pubkey_hex.slice(0, 8)}`
      : `#${identity.pubkey_hex.slice(0, 8)}`
    : '';

  // Minimal header for loading + error states: keeps the close ✕ visible so
  // the user can always dismiss the pane, even when the artifact data hasn't
  // arrived (or failed to load entirely).
  const minimalHeader = (
    <div className="flex h-16 flex-shrink-0 items-center justify-end border-b border-[#27272A] bg-[#18181B] px-4 py-3.5">
      <button
        data-testid="explore-detail-close"
        aria-label="Close"
        onClick={() => setSelectedId(null)}
        className="flex h-8 w-8 items-center justify-center rounded-md text-[#71717A] hover:bg-[#27272A]/50 hover:text-[#FAFAFA]"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );

  if (loading && !artifact) {
    return (
      <div data-testid="explore-detail" className="flex h-full flex-col overflow-hidden bg-[#141416]">
        {minimalHeader}
        <div data-testid="explore-detail-skeleton" className="flex flex-1 flex-col gap-3 p-4">
          <div className="h-16 animate-pulse rounded bg-[#27272A]" />
          <div className="h-32 animate-pulse rounded-md bg-[#27272A]" />
          <div className="h-4 w-3/4 animate-pulse rounded bg-[#27272A]" />
        </div>
      </div>
    );
  }

  if (!artifact) {
    return (
      <div data-testid="explore-detail" className="flex h-full flex-col overflow-hidden bg-[#141416]">
        {minimalHeader}
        <div className="flex flex-1 items-center justify-center p-6 text-center text-xs text-rose-400">
          Failed to load artifact details.
        </div>
      </div>
    );
  }

  // Base labels are tab-derived (`actionLabelsFor`). The Discover tab's middle
  // slot is "Install" by default, but if the user already has this artifact
  // in their local registry an Install click is a no-op — flip it to
  // "Uninstall" so the action panel reflects reality. Installed/My-Uploads
  // labels are unchanged. The destructive-variant decision below keys off
  // the resolved label, so styling follows automatically.
  const baseLabels = actionLabelsFor(tab);
  const labels =
    tab === 'discover' && installed && baseLabels.middle === 'Install'
      ? { ...baseLabels, middle: 'Uninstall' }
      : baseLabels;
  // The upstream artifact is gone (`/v1/artifact/:id` returned status:
  // 'tombstoned'). The user got here through the Installed tab — Discover
  // filters tombstones server-side, but the user can still click into one
  // they have installed. Surface a banner + disable any action that
  // requires the upstream (Install / Update / Open / Preview).
  const tombstoned = artifact.status === 'tombstoned';
  const isBundle = artifact.kind === 'bundle';
  const KindIcon = isBundle ? Layers : Palette;
  const kindIconColor = isBundle ? 'text-[#A855F7] bg-[#A855F7]/10' : 'text-[#00D9FF] bg-[#00D9FF]/10';
  const kindLabel = isBundle ? 'Bundle' : 'Theme';

  const name =
    typeof artifact.manifest.name === 'string' ? artifact.manifest.name : artifact.artifact_id;
  const description = manifestString(
    artifact.manifest as Record<string, unknown>,
    'description',
  );
  const version = manifestString(artifact.manifest as Record<string, unknown>, 'version') ?? '—';
  const license = manifestString(artifact.manifest as Record<string, unknown>, 'license') ?? '—';
  const updatedAt =
    typeof artifact.updated_at === 'number' && artifact.updated_at > 0
      ? new Date(artifact.updated_at * 1000).toLocaleDateString()
      : '—';
  const installs =
    typeof artifact.installs === 'number' ? artifact.installs.toLocaleString() : '—';
  const rawTags = artifact.manifest['tags'];
  const tags: string[] =
    Array.isArray(rawTags) && rawTags.every((t) => typeof t === 'string')
      ? (rawTags as string[])
      : [];

  const authorSlice = artifact.author_pubkey.slice(0, 8);
  const authorName = artifact.author_display_name?.trim();

  const handleInstall = async (trustNewPubkey = false) => {
    setInstallState({ kind: 'in-flight', phase: 'download', done: 0, total: 4 });
    try {
      const params: {
        artifact_id: string;
        target_workspace: string;
        trust_new_pubkey?: boolean;
      } = {
        artifact_id: artifact.artifact_id,
        target_workspace: installFolderPath(name, artifact.artifact_id),
      };
      if (trustNewPubkey) params.trust_new_pubkey = true;
      const result = (await send(
        'explorer.install',
        params as Parameters<typeof send<'explorer.install'>>[1],
      )) as unknown as {
        tofu?: 'ok' | 'mismatch';
        previously_seen?: TofuFingerprint;
        incoming?: TofuFingerprint;
      };
      if (result.tofu === 'mismatch' && result.previously_seen && result.incoming) {
        setTofuPair({ previously: result.previously_seen, incoming: result.incoming });
        setTofuOpen(true);
        setInstallState({ kind: 'idle' });
        return;
      }
      setInstallState({ kind: 'idle' });
      // Re-scan the workspace so the newly-installed overlay folder shows
      // up in the header dropdown without a full app reload. The grid
      // card's "Installed" badge is driven by the installed-id set
      // managed at the panel level (useInstalledArtifactIds); after a
      // detail-pane install, we fire a window event the panel listens
      // for so it refetches that set.
      await refreshOverlays();
      window.dispatchEvent(new CustomEvent('omni:artifact-installed'));
      // Pre-select the just-installed overlay and jump to the editor view
      // so the preview + editor are already pointed at it. Mirrors the
      // post-fork behavior in `handleFork` below. Does NOT setAsActive —
      // the in-game overlay only changes via the explicit Set Active action.
      const installedName = installFolderName(name, artifact.artifact_id);
      dispatch({ type: 'SELECT_OVERLAY', payload: installedName });
      dispatch({ type: 'SET_ACTIVE_PANEL', payload: 'components' });
      void ensureOverlayLoaded(installedName);
      toast.success(`Installed ${name}`);
    } catch (err) {
      setInstallState({ kind: 'error', message: mapErrorToUserMessage(err as OmniError).text });
    }
  };

  const handlePreview = async () => {
    try {
      const resp = await send('explorer.preview', { artifact_id: artifact.artifact_id });
      setPreview(resp.preview_token, {
        artifact_id: artifact.artifact_id,
        content_hash: artifact.content_hash,
        author_pubkey: artifact.author_pubkey,
        name,
        kind: isBundle ? 'bundle' : 'theme',
        tags,
        installs: artifact.installs ?? 0,
        r2_url: artifact.r2_url,
        thumbnail_url: artifact.thumbnail_url,
        author_fingerprint_hex: artifact.author_fingerprint_hex,
        created_at: artifact.created_at,
        updated_at: artifact.updated_at,
        author_display_name: artifact.author_display_name,
      });
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  // My-uploads "Update" CTA: resolve the artifact_id back to its local
  // workspace folder via the publish-index (host-side `publish.lookupWorkspace`),
  // then ask the panel to open <UploadDialog> pre-filled. Three terminal
  // statuses, each mapped to a user-facing toast:
  //   ok            → open dialog pre-filled with workspace_path
  //   missing_index → toast.info (artifact was published from a different
  //                   install / machine — author can't update it from here)
  //   missing_folder → toast.error (workspace folder was moved/deleted; user
  //                   needs to recreate it under the named path)
  const handleAuthorUpdate = async () => {
    try {
      const resp = await send('publish.lookupWorkspace', {
        artifact_id: artifact.artifact_id,
      });
      if (resp.status === 'ok' && resp.workspace_path) {
        onRequestUpload?.(resp.workspace_path);
      } else if (resp.status === 'missing_index') {
        toast.info(
          "This artifact has no publish record on this machine — you can't update it from here.",
        );
      } else if (resp.status === 'missing_folder') {
        toast.error(
          `Workspace folder was moved or deleted. Recreate it under ${resp.kind}s/${resp.name}/ to publish an update.`,
        );
      }
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  // My-uploads "Delete" CTA: removes the artifact from the share hub. The
  // host handler invalidates caches + tombstones the row server-side so
  // other surfaces refetch fresh. Local files are unaffected — only the
  // upstream listing goes away.
  const handleAuthorDelete = async () => {
    try {
      await send('upload.delete', { artifact_id: artifact.artifact_id });
      toast.success(`Deleted ${name} from the share hub.`);
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  const handleFork = async ({ target_name }: { target_name: string }) => {
    try {
      await send('explorer.fork', { artifact_id: artifact.artifact_id, target_name });
      setForkOpen(false);
      // Reload state.overlays from disk so SELECT_OVERLAY can find the new
      // fork; without this, getCurrentOverlay() returns undefined and the
      // editor lands in an empty "No overlay" state with the fork missing
      // from the header dropdown even though the files exist on disk.
      await refreshOverlays();
      dispatch({ type: 'SELECT_OVERLAY', payload: target_name });
      dispatch({ type: 'SET_ACTIVE_PANEL', payload: 'components' });
      toast.success(`Forked to overlays/${target_name} — ready to edit`);
    } catch (err) {
      setForkOpen(false);
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  const kebabLabels = kebabLabelsFor(tab);

  // Header pill + confirm-dialog wiring. Both require:
  //   - an entry for this artifact in updateStatus with available === true
  //   - the matching local registry row (for installed_version + author_pubkey
  //     fields that <UpdateConfirmDialog> renders)
  // Either being missing means we silently skip the pill. The corner pill on
  // the card has the same gating in <ArtifactCard>; the two surfaces stay
  // visually in sync because they read the same map.
  const status = updateStatus?.get(artifact.artifact_id);
  const installedRow = installedEntries?.find((e) => e.artifact_id === artifact.artifact_id);

  return (
    <>
      <div
        data-testid="explore-detail"
        className="flex h-full flex-col overflow-hidden bg-[#141416]"
      >
        {/* Sticky header (h-16) */}
        <div className="flex h-16 flex-shrink-0 items-center justify-between border-b border-[#27272A] bg-[#18181B] px-4 py-3.5">
          <div className="flex min-w-0 items-center gap-3">
            <div
              className={cn(
                'flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg',
                kindIconColor,
              )}
            >
              <KindIcon className="h-[18px] w-[18px]" aria-hidden />
            </div>
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="truncate text-base font-semibold leading-tight text-[#FAFAFA]">
                {name}
              </span>
              <span className="text-[13px] leading-tight text-[#71717A]">{kindLabel}</span>
            </div>
          </div>
          <div className="flex items-center gap-1">
            {/* Header-variant update pill. Surfaces only when there's an actual
                version delta against the worker manifest AND we have the
                installed registry row in hand (the confirm dialog needs both
                installed_version and author_pubkey from it). Click opens the
                <UpdateConfirmDialog> mounted at the bottom of this component. */}
            {status?.available && installedRow && (
              <UpdateAvailablePill
                status={status}
                variant="header"
                onClick={() => setConfirmOpen(true)}
              />
            )}
            {/* Kebab menu — currently empty (kebabLabelsFor returns [] for every
                tab after OWI-132 T5). Block stays in place so OWI-109 can
                repopulate Report / View-policy items without re-deriving the
                trigger styling. */}
            {kebabLabels.length > 0 && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    data-testid="explore-detail-kebab"
                    aria-label="More options"
                    className="flex h-8 w-8 items-center justify-center rounded-md text-[#71717A] hover:bg-[#27272A]/50 hover:text-[#FAFAFA]"
                  >
                    <MoreVertical className="h-4 w-4" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" />
              </DropdownMenu>
            )}
            <button
              data-testid="explore-detail-close"
              aria-label="Close"
              onClick={() => setSelectedId(null)}
              className="flex h-8 w-8 items-center justify-center rounded-md text-[#71717A] hover:bg-[#27272A]/50 hover:text-[#FAFAFA]"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Scrollable body */}
        <div className="flex-1 overflow-y-auto p-3.5">
          <div className="flex flex-col gap-3.5">
            {/* Hero */}
            <div className="aspect-video w-full overflow-hidden rounded-lg border border-[#27272A] bg-gradient-to-br from-[#1F1F23] to-[#101013]">
              {artifact.thumbnail_url && (
                <img
                  src={artifact.thumbnail_url}
                  alt={name}
                  className="h-full w-full object-cover"
                />
              )}
            </div>

            {/* Tombstoned banner: upstream artifact has been removed by the
                author (or moderation). The local copy still works, but the
                user can't re-download or update — surface the state and
                offer Fork as the durable-keep path. */}
            {tombstoned && (
              <div
                role="alert"
                data-testid="explore-detail-tombstoned-banner"
                className="flex items-start gap-2.5 rounded-md border border-amber-500/30 bg-amber-500/[0.06] p-3 text-[12px] leading-relaxed text-amber-200"
              >
                <AlertTriangle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0 text-amber-400" />
                <div className="flex flex-col gap-2">
                  <p>
                    <strong className="text-amber-100">
                      The author removed this from the share explorer.
                    </strong>{' '}
                    Your installed copy still works, but you can&apos;t re-download or update
                    it. Fork now to keep a copy you own.
                  </p>
                  <div>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-amber-500/40 bg-amber-500/[0.08] text-amber-100 hover:bg-amber-500/[0.16] hover:text-amber-50"
                      onClick={() => setForkOpen(true)}
                    >
                      <ForkIcon />
                      Fork now
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {description && (
              <p className="whitespace-pre-wrap text-sm leading-relaxed text-[#A1A1AA]">
                {description}
              </p>
            )}

            {/* Author chip */}
            <div className="flex items-center gap-3 rounded-lg border border-[#27272A] bg-[#27272A]/40 p-3">
              <div className="h-9 w-9 flex-shrink-0 rounded-full bg-gradient-to-br from-[#00D9FF] to-[#A855F7]" />
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-sm font-medium text-[#FAFAFA]">
                  {authorName ?? 'Unknown'}
                </span>
                <span className="truncate font-mono text-[13px] text-[#71717A]">
                  #{authorSlice}
                </span>
              </div>
            </div>

            {/* Stats grid 2x2 */}
            <div className="grid grid-cols-2 gap-2">
              <Stat icon={Download} label="Installs" value={installs} />
              <Stat icon={CheckSquare} label="Version" value={version} mono />
              <Stat icon={Clock} label="Updated" value={updatedAt} />
              <Stat icon={Calendar} label="License" value={license} />
            </div>

            {/* Tags */}
            {tags.length > 0 && (
              <div>
                <h4 className="mb-2.5 text-[11px] font-semibold uppercase tracking-[0.10em] text-[#52525B]">
                  Tags
                </h4>
                <div className="flex flex-wrap gap-2">
                  {tags.map((tag) => (
                    <span
                      key={tag}
                      className="flex items-center gap-1.5 rounded-md border border-[#3F3F46] bg-[#27272A] px-3 py-1 text-[13px] text-[#D4D4D8]"
                    >
                      <TagIcon className="h-3 w-3" aria-hidden />
                      {tag}
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Sticky footer */}
        <div className="flex flex-shrink-0 items-center gap-2 border-t border-[#27272A] bg-[#141416]/80 p-3">
          {labels.left === 'Preview' ? (
            // Preview hits `/v1/download/:id` which 410s on tombstoned rows —
            // disable the button rather than letting the user click into a
            // cryptic "TOMBSTONED" toast. `Open` (Installed/My-Uploads tabs)
            // reads from disk, so it stays enabled.
            <Button
              variant="outline"
              size="sm"
              onClick={handlePreview}
              disabled={tombstoned}
              title={tombstoned ? 'Removed from the share explorer' : undefined}
            >
              {labels.left}
            </Button>
          ) : (
            // `Open` slot for Installed / My-Uploads — opens the on-disk
            // workspace in the editor. The full Open flow lands in sub-spec
            // #016 (OWI-15); for now we keep the slot visible with a toast so
            // the layout stays stable and the user gets clear feedback.
            <Button
              variant="outline"
              size="sm"
              onClick={() => toast.info('Open lands in sub-spec #016.')}
            >
              {labels.left}
            </Button>
          )}
          <div className="flex-1" data-testid="explore-detail-action-middle">
            {labels.middle === 'Install' ? (
              installState.kind === 'idle' ? (
                // Install would trip the worker's 410 TOMBSTONED on download —
                // disable when the upstream is gone. Discover normally won't
                // surface tombstoned rows (server filter), so this fires only
                // when the pane is opened with a stale `selectedId`.
                <Button
                  className="w-full"
                  size="sm"
                  onClick={() => void handleInstall(false)}
                  disabled={tombstoned}
                  title={tombstoned ? 'Removed from the share explorer' : undefined}
                >
                  Install
                </Button>
              ) : installState.kind === 'in-flight' ? (
                <InstallProgress
                  phase={installState.phase}
                  done={installState.done}
                  total={installState.total}
                />
              ) : (
                <InlineError
                  message={installState.message}
                  onRetry={() => void handleInstall(false)}
                />
              )
            ) : (
              // Both Uninstall and Delete remove the artifact — render them
              // with the destructive variant so the visual weight matches the
              // action's gravity (red, not the default cyan primary).
              // Uninstall opens the panel-level confirm dialog and runs
              // `explorer.uninstall`; Delete fires `upload.delete` on the
              // share hub (local files untouched).
              <Button
                className="w-full"
                variant={
                  labels.middle === 'Delete' || labels.middle === 'Uninstall'
                    ? 'destructive'
                    : 'default'
                }
                size="sm"
                onClick={
                  labels.middle === 'Uninstall'
                    ? () => onRequestUninstall?.(artifact.artifact_id, name)
                    : labels.middle === 'Delete'
                      ? () => void handleAuthorDelete()
                      : () => void handleInstall(false)
                }
              >
                {labels.middle}
              </Button>
            )}
          </div>
          {labels.right === 'Fork' ? (
            <Button
              variant="outline"
              size="icon"
              aria-label="Fork"
              onClick={() => setForkOpen(true)}
            >
              <ForkIcon />
            </Button>
          ) : (
            // My-Uploads "Update" slot — resolves the artifact's workspace via
            // `publish.lookupWorkspace` and opens <UploadDialog> pre-filled.
            // Disabled when the upstream is tombstoned because PATCH-style
            // updates would 404 on the worker (`is_removed = 1`). My-Uploads
            // filters tombstones server-side; this is defensive belt+suspenders.
            <Button
              variant="outline"
              size="sm"
              onClick={() => void handleAuthorUpdate()}
              disabled={tombstoned && labels.right === 'Update'}
              title={
                tombstoned && labels.right === 'Update'
                  ? 'Removed from the share explorer'
                  : undefined
              }
            >
              {labels.right}
            </Button>
          )}
        </div>
      </div>

      {tofuPair && (
        <TofuMismatchDialog
          open={tofuOpen}
          onOpenChange={setTofuOpen}
          artifactName={name}
          previously={tofuPair.previously}
          incoming={tofuPair.incoming}
          onCancel={() => setTofuOpen(false)}
          onTrustNew={() => {
            setTofuOpen(false);
            void handleInstall(true);
          }}
        />
      )}
      <ForkDialog
        open={forkOpen}
        onOpenChange={setForkOpen}
        sourceKind={tab === 'discover' ? 'remote' : 'local'}
        origin={{ name, author_handle: authorName ? `${authorName}#${authorSlice}` : `#${authorSlice}` }}
        defaultName={`${name}-fork`}
        selfHandle={selfHandle}
        existingNames={existingNames}
        onCancel={() => setForkOpen(false)}
        onFork={({ target_name }) => void handleFork({ target_name })}
      />
      {/* Update-apply confirm dialog. Mounted conditionally because both
          UpdateStatus and the matching installedRow must be in scope for the
          dialog to render the version delta + pubkey-rotation warning. The
          post-install `omni:artifact-installed` window event is fired by the
          install pipeline itself (see explore-detail.tsx:handleInstall and
          explore-panel.tsx:handleHoverInstall), so registry refresh +
          indicator clear happen automatically. */}
      {status?.available && installedRow && (
        <UpdateConfirmDialog
          open={confirmOpen}
          onOpenChange={setConfirmOpen}
          artifact={artifact}
          installed={installedRow}
          onApplied={() => {
            // No-op — the install pipeline dispatches
            // `omni:artifact-installed` which every useInstalledArtifacts
            // consumer listens for. Registry, grid badges, header pill, and
            // detail pane all refresh from a single event.
          }}
        />
      )}
    </>
  );
}

function Stat({
  icon: Icon,
  label,
  value,
  mono = false,
}: {
  icon: typeof Download;
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex flex-col gap-1.5 rounded-lg border border-[#27272A] bg-[#27272A]/40 p-3">
      <div className="flex items-center gap-1.5 text-[11px] uppercase tracking-[0.06em] text-[#71717A]">
        <Icon className="h-3 w-3" aria-hidden />
        {label}
      </div>
      <div className={cn('text-base font-semibold text-[#FAFAFA]', mono && 'font-mono')}>
        {value}
      </div>
    </div>
  );
}

function ForkIcon() {
  // Inline lucide-style fork icon (matches mockup spec §3.4 footer right slot)
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
      <circle cx="12" cy="18" r="3" />
      <circle cx="6" cy="6" r="3" />
      <circle cx="18" cy="6" r="3" />
      <path d="M6 9v3a3 3 0 0 0 3 3h6a3 3 0 0 0 3-3V9" />
      <path d="M12 12v3" />
    </svg>
  );
}
