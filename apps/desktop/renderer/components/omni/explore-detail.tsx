/**
 * ExploreDetail — right 260px detail pane.
 *
 * Renders an ArtifactCard "detail" variant wired with per-tab action slots
 * and kebab menu items. In Wave 3b only Preview is fully functional;
 * Install/Uninstall/Fork/Delete/Update click handlers toast "coming in sub-spec
 * #015/#016" so the UI is visible but real flows defer to those owners.
 *
 * Kebab: Copy artifact ID and Copy share link are fully wired in all tabs.
 * Check for update (Installed tab only) is a stub until #016 wires the
 * local install registry.
 *
 * P15 (OWI-106): The Install middle slot is now a real { idle | in-flight |
 * error } state machine. On success, toasts and resets. On tofu='mismatch',
 * opens TofuMismatchDialog (re-dispatches with trust_new_pubkey=true on
 * confirm). On error, renders InlineError with retry.
 *
 * NOTE: The host's explorer.installResult schema carries a `tofu` discriminator
 * ('first_install' | 'matched' | 'mismatch') but does NOT yet emit
 * `previously_seen` or `incoming` fingerprint fields — those are a host-side
 * gap. The TofuMismatchDialog branch is wired but will not fire until the host
 * adds those fields. Tracked as a follow-up.
 *
 * NOTE: Live progress events (explorer.installProgress subscription) are
 * deferred to a follow-up. The InstallProgress bar animates statically at
 * phase='download' during the flight for now.
 *
 * P16 (OWI-107): Fork right slot now opens ForkDialog instead of stub toast.
 * On submit, dispatches explorer.fork (artifact_id + target_name — the shipped
 * ExplorerForkParams schema is flat; the spec's discriminated source union is
 * not in the Rust handler yet, tracked as a host-side follow-up). On success:
 * closes dialog + SELECT_OVERLAY → target_name + SET_ACTIVE_PANEL →
 * 'components' + toast. NOTE: SET_ACTIVE_OVERLAY does not exist in the reducer;
 * SELECT_OVERLAY is used instead.
 *
 * existingNames is derived from state.overlays (array) — useOmniState has no
 * workspaces map; the overlays array serves as the collision list.
 */

import { useMemo, useState } from 'react';
import { Button } from '@/components/ui/button';
import { DropdownMenuItem, DropdownMenuSeparator } from '@/components/ui/dropdown-menu';
import { ArtifactCard, type ArtifactCardActionSlots } from './artifact-card';
import { InstallProgress, type InstallPhase } from './install-progress';
import { TofuMismatchDialog, type TofuFingerprint } from './tofu-mismatch-dialog';
import { ForkDialog } from './fork-dialog';
import { InlineError } from './inline-error';
import { useExploreDetail } from '../../hooks/use-explore-detail';
import { usePreview } from '../../lib/preview-context';
import { useShareWs } from '../../hooks/use-share-ws';
import { useOmniState } from '../../hooks/use-omni-state';
import { useIdentity } from '../../lib/identity-context';
import { toast } from '../../lib/toast';
import { mapErrorToUserMessage, type OmniError } from '../../lib/map-error-to-user-message';
import {
  actionLabelsFor,
  kebabLabelsFor,
  buildShareLink,
  type ExploreTab,
} from '../../lib/artifact-actions';

export interface ExploreDetailProps {
  selectedId: string | null;
  tab: ExploreTab;
}

type InstallState =
  | { kind: 'idle' }
  | { kind: 'in-flight'; phase: InstallPhase; done: number; total: number }
  | { kind: 'error'; message: string };

export function ExploreDetail({ selectedId, tab }: ExploreDetailProps) {
  const { artifact, loading } = useExploreDetail(selectedId);
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

  if (selectedId === null) {
    return (
      <div
        data-testid="explore-detail-placeholder"
        className="flex h-full items-center justify-center p-6 text-center text-xs text-zinc-500"
      >
        Select an artifact to see details.
      </div>
    );
  }

  if (loading && !artifact) {
    return (
      <div data-testid="explore-detail-skeleton" className="flex flex-col gap-3 p-4">
        <div className="h-32 animate-pulse rounded-md bg-[#27272A]" />
        <div className="h-4 w-3/4 animate-pulse rounded bg-[#27272A]" />
        <div className="h-3 w-1/2 animate-pulse rounded bg-[#27272A]" />
      </div>
    );
  }

  if (!artifact) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-center text-xs text-rose-400">
        Failed to load artifact details.
      </div>
    );
  }

  const labels = actionLabelsFor(tab);

  const handleInstall = async (trustNewPubkey = false) => {
    setInstallState({ kind: 'in-flight', phase: 'download', done: 0, total: 4 });
    try {
      const params: { artifact_id: string; trust_new_pubkey?: boolean } = {
        artifact_id: artifact.artifact_id,
      };
      if (trustNewPubkey) params.trust_new_pubkey = true;
      const result = (await send('explorer.install', params as Parameters<typeof send<'explorer.install'>>[1])) as unknown as {
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
      toast.success(`Installed ${typeof artifact.manifest.name === 'string' ? artifact.manifest.name : artifact.artifact_id}`);
    } catch (err) {
      setInstallState({
        kind: 'error',
        message: mapErrorToUserMessage(err as OmniError).text,
      });
    }
  };

  const handlePreview = async () => {
    try {
      const resp = await send('explorer.preview', { artifact_id: artifact.artifact_id });
      setPreview(resp.preview_token, {
        artifact_id: artifact.artifact_id,
        content_hash: artifact.content_hash,
        author_pubkey: artifact.author_pubkey,
        name:
          typeof artifact.manifest.name === 'string'
            ? artifact.manifest.name
            : artifact.artifact_id,
        kind: artifact.kind === 'bundle' ? 'bundle' : 'theme',
        tags: Array.isArray(artifact.manifest.tags)
          ? (artifact.manifest.tags.filter((t): t is string => typeof t === 'string') as string[])
          : [],
        installs: artifact.installs ?? 0,
        r2_url: artifact.r2_url,
        thumbnail_url: artifact.thumbnail_url,
        author_fingerprint_hex: artifact.author_fingerprint_hex,
        created_at: artifact.created_at,
        updated_at: artifact.updated_at,
        // OWI-91: forward the author_display_name from the ArtifactDetail
        // into the cached preview entry so the preview-banner author chip
        // matches the detail-card handle (single source of truth — both
        // schemas now declare this field).
        author_display_name: artifact.author_display_name,
      });
    } catch (err) {
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  const stubSubSpec = (which: '#015' | '#016') => () => {
    toast.info(`That action lands in sub-spec ${which}.`);
  };

  const copyId = async () => {
    await navigator.clipboard.writeText(artifact.artifact_id);
    toast.success('Artifact ID copied.');
  };

  const copyShareLink = async () => {
    await navigator.clipboard.writeText(buildShareLink(artifact.artifact_id));
    toast.success('Share link copied.');
  };

  // Mirrors authorDisplay() from artifact-card.tsx (not exported).
  // `<display_name>#<8-hex>` when display name is present; `#<8-hex>` otherwise.
  const artifactAuthorDisplay = (() => {
    const slice = artifact.author_pubkey.slice(0, 8);
    const raw = artifact.author_display_name;
    const name = raw?.trim();
    return name ? `${name}#${slice}` : `#${slice}`;
  })();

  const artifactName =
    typeof artifact.manifest.name === 'string' ? artifact.manifest.name : artifact.artifact_id;

  const handleFork = async ({ target_name }: { target_name: string }) => {
    try {
      await send('explorer.fork', {
        artifact_id: artifact.artifact_id,
        target_name,
      });
      setForkOpen(false);
      dispatch({ type: 'SELECT_OVERLAY', payload: target_name });
      dispatch({ type: 'SET_ACTIVE_PANEL', payload: 'components' });
      toast.success(`Forked to overlays/${target_name} — ready to edit`);
    } catch (err) {
      setForkOpen(false);
      toast.error(err as Parameters<typeof toast.error>[0]);
    }
  };

  const actionSlots: ArtifactCardActionSlots = {
    left:
      labels.left === 'Preview' ? (
        <Button variant="outline" size="sm" onClick={handlePreview}>
          {labels.left}
        </Button>
      ) : (
        <Button variant="outline" size="sm" onClick={stubSubSpec('#016')}>
          {labels.left}
        </Button>
      ),
    middle:
      labels.middle === 'Install' ? (
        installState.kind === 'idle' ? (
          <Button variant="default" size="sm" onClick={() => void handleInstall(false)}>
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
          variant={labels.middle === 'Delete' ? 'destructive' : 'default'}
          size="sm"
          onClick={stubSubSpec(labels.middle === 'Delete' ? '#015' : '#016')}
        >
          {labels.middle}
        </Button>
      ),
    right:
      labels.right === 'Fork' ? (
        <Button variant="secondary" size="sm" onClick={() => setForkOpen(true)}>
          Fork
        </Button>
      ) : (
        <Button
          variant="secondary"
          size="sm"
          onClick={stubSubSpec(labels.right === 'Update' ? '#015' : '#016')}
        >
          {labels.right}
        </Button>
      ),
  };

  const kebabLabels = kebabLabelsFor(tab);
  const kebabItems = (
    <>
      <DropdownMenuItem onSelect={copyId}>Copy artifact ID</DropdownMenuItem>
      <DropdownMenuItem onSelect={copyShareLink}>Copy share link</DropdownMenuItem>
      {kebabLabels.includes('Check for update') ? (
        <>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={stubSubSpec('#016')}>Check for update</DropdownMenuItem>
        </>
      ) : null}
    </>
  );

  return (
    <>
      <ArtifactCard
        variant="detail"
        artifact={artifact}
        actionSlots={actionSlots}
        kebabMenuItems={kebabItems}
      />
      {tofuPair && (
        <TofuMismatchDialog
          open={tofuOpen}
          onOpenChange={setTofuOpen}
          artifactName={
            typeof artifact.manifest.name === 'string'
              ? artifact.manifest.name
              : artifact.artifact_id
          }
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
        origin={{
          name: artifactName,
          author_handle: artifactAuthorDisplay,
        }}
        defaultName={`${artifactName}-fork`}
        selfHandle={selfHandle}
        existingNames={existingNames}
        onCancel={() => setForkOpen(false)}
        onFork={({ target_name }) => void handleFork({ target_name })}
      />
    </>
  );
}
