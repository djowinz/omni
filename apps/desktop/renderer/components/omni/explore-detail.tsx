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
import { Calendar, CheckSquare, Clock, Download, Layers, MoreVertical, Palette, Tag as TagIcon, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { InstallProgress, type InstallPhase } from './install-progress';
import { TofuMismatchDialog, type TofuFingerprint } from './tofu-mismatch-dialog';
import { ForkDialog } from './fork-dialog';
import { InlineError } from './inline-error';
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
  kebabLabelsFor,
  type ExploreTab,
} from '../../lib/artifact-actions';

export interface ExploreDetailProps {
  selectedId: string;
  tab: ExploreTab;
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

export function ExploreDetail({ selectedId, tab }: ExploreDetailProps) {
  const { artifact, loading } = useExploreDetail(selectedId);
  const { setSelectedId } = useExploreFilters();
  const { setPreview } = usePreview();
  const { send } = useShareWs();
  const { state: omniState, dispatch } = useOmniState();
  const { identity } = useIdentity();

  const [installState, setInstallState] = useState<InstallState>({ kind: 'idle' });
  const [tofuOpen, setTofuOpen] = useState(false);
  const [tofuPair, setTofuPair] = useState<{
    previously: TofuFingerprint;
    incoming: TofuFingerprint;
  } | null>(null);
  const [forkOpen, setForkOpen] = useState(false);

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

  const labels = actionLabelsFor(tab);
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
      const params: { artifact_id: string; trust_new_pubkey?: boolean } = {
        artifact_id: artifact.artifact_id,
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

  const stubSubSpec = (which: '#015' | '#016') => () => {
    toast.info(`That action lands in sub-spec ${which}.`);
  };

  const handleFork = async ({ target_name }: { target_name: string }) => {
    try {
      await send('explorer.fork', { artifact_id: artifact.artifact_id, target_name });
      setForkOpen(false);
      dispatch({ type: 'SELECT_OVERLAY', payload: target_name });
      dispatch({ type: 'SET_ACTIVE_PANEL', payload: 'components' });
      toast.success(`Forked to overlays/${target_name} — ready to edit`);
    } catch (err) {
      setForkOpen(false);
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  const kebabLabels = kebabLabelsFor(tab);

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
                <DropdownMenuContent align="end">
                  {kebabLabels.includes('Check for update') && (
                    <DropdownMenuItem onSelect={stubSubSpec('#016')}>
                      Check for update
                    </DropdownMenuItem>
                  )}
                </DropdownMenuContent>
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
            <Button variant="outline" size="sm" onClick={handlePreview}>
              {labels.left}
            </Button>
          ) : (
            <Button variant="outline" size="sm" onClick={stubSubSpec('#016')}>
              {labels.left}
            </Button>
          )}
          <div className="flex-1" data-testid="explore-detail-action-middle">
            {labels.middle === 'Install' ? (
              installState.kind === 'idle' ? (
                <Button className="w-full" size="sm" onClick={() => void handleInstall(false)}>
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
              <Button
                className="w-full"
                variant={labels.middle === 'Delete' ? 'destructive' : 'default'}
                size="sm"
                onClick={stubSubSpec(labels.middle === 'Delete' ? '#015' : '#016')}
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
            <Button
              variant="outline"
              size="sm"
              onClick={stubSubSpec(labels.right === 'Update' ? '#015' : '#016')}
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
