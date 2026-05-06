/**
 * ArtifactCard — grid-variant card for the Explore panel.
 *
 * Per share-explorer-redesign spec §3.5:
 * - The 'detail' variant is retired (the new <ExploreDetail> builds its own
 *   header/body/footer layout).
 * - Card has Tailwind `group` so <CardHoverOverlay> can reveal on hover.
 * - Kind badge uses rounded-md (was rounded-full).
 * - Tag chips use rounded-md (was rounded-full).
 * - data-selected="true" adds a 1.5px cyan border + 3px ring.
 */

import { AlertTriangle, Download, Layers, Palette } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { CachedArtifactDetail, ArtifactDetail } from '../../lib/share-types';
import { CardHoverOverlay } from './card-hover-overlay';

export interface ArtifactCardProps {
  artifact: CachedArtifactDetail | ArtifactDetail;
  installed?: boolean;
  /** True when the upstream artifact row has `is_removed = 1` — the
   *  installed copy still works locally, but the card surfaces an amber
   *  "Removed upstream" pill so the user knows the author pulled it
   *  from the share explorer. Only meaningful on the Installed tab; the
   *  Discover list filters tombstoned rows out server-side. */
  tombstoned?: boolean;
  onClick?: () => void;
  onPreview?: () => void;
  onInstall?: () => void;
  onUninstall?: () => void;
  className?: string;
  'data-selected'?: 'true' | 'false';
}

function manifestString(manifest: Record<string, unknown> | undefined, key: string): string | null {
  if (!manifest) return null;
  const v = manifest[key];
  return typeof v === 'string' && v.length > 0 ? v : null;
}

function artifactName(artifact: CachedArtifactDetail | ArtifactDetail): string {
  const cached = artifact as CachedArtifactDetail;
  if (typeof cached.name === 'string' && cached.name.length > 0) return cached.name;
  const detail = artifact as ArtifactDetail;
  if (detail.manifest) {
    const fromManifest = manifestString(detail.manifest as Record<string, unknown>, 'name');
    if (fromManifest) return fromManifest;
  }
  return artifact.artifact_id;
}

function authorDisplay(artifact: CachedArtifactDetail | ArtifactDetail): string {
  const slice = artifact.author_pubkey.slice(0, 8);
  const name = artifact.author_display_name?.trim();
  return name ? `${name}#${slice}` : `#${slice}`;
}

function manifestTags(artifact: CachedArtifactDetail | ArtifactDetail): string[] {
  const cached = artifact as CachedArtifactDetail;
  if (Array.isArray(cached.tags)) return cached.tags;
  const detail = artifact as ArtifactDetail;
  if (detail.manifest && Array.isArray((detail.manifest as Record<string, unknown>)['tags'])) {
    return ((detail.manifest as Record<string, unknown>)['tags'] as unknown[]).filter(
      (t): t is string => typeof t === 'string',
    );
  }
  return [];
}

export function ArtifactCard(props: ArtifactCardProps) {
  const { artifact, installed, tombstoned, onClick, onPreview, onInstall, onUninstall, className } = props;
  const name = artifactName(artifact);
  const author = authorDisplay(artifact);
  const detail = artifact as ArtifactDetail;
  const installCount = typeof detail.installs === 'number' ? detail.installs : null;
  const tags = manifestTags(artifact).slice(0, 3);

  const isBundle = artifact.kind === 'bundle';
  // Tombstoned takes priority: the upstream artifact is gone, so the user
  // shouldn't see the green "Installed" affordance — it implies the artifact
  // is still good to redistribute, which it isn't. Amber pill flags the
  // "still works locally, but pulled from the share network" state.
  const KindIcon = tombstoned ? AlertTriangle : isBundle ? Layers : Palette;
  const kindLabel = tombstoned
    ? 'Removed upstream'
    : installed
      ? 'Installed'
      : isBundle
        ? 'Bundle'
        : 'Theme';
  const kindColor = tombstoned
    ? 'text-amber-400 border-amber-500/30'
    : installed
      ? 'text-emerald-400 border-emerald-500/30'
      : isBundle
        ? 'text-[#A855F7] border-[#A855F7]/40'
        : 'text-[#00D9FF] border-[#3F3F46]';

  return (
    <div
      data-testid="artifact-card-grid"
      data-selected={props['data-selected']}
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick}
      onKeyDown={
        onClick
          ? (e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                onClick();
              }
            }
          : undefined
      }
      className={cn(
        'group relative flex flex-col overflow-hidden rounded-lg border border-[#27272A] bg-[#141416] transition-colors',
        onClick && 'cursor-pointer hover:border-[#3F3F46] hover:bg-[#1A1A1D] focus-visible:outline-none',
        props['data-selected'] === 'true' &&
          'border-[1.5px] border-[#00D9FF] shadow-[0_0_0_3px_rgba(0,217,255,0.10)]',
        className,
      )}
    >
      {/* Thumbnail */}
      <div className="relative aspect-video w-full overflow-hidden bg-[#0F0F12]">
        {artifact.thumbnail_url ? (
          <img src={artifact.thumbnail_url} alt={name} className="h-full w-full object-cover" />
        ) : (
          <div className="h-full w-full bg-gradient-to-br from-[#1F1F23] to-[#101013]" />
        )}
        {/* Kind badge */}
        <span
          className={cn(
            'absolute right-2 top-2 flex items-center gap-1.5 rounded-md border bg-black/60 px-2 py-1 text-xs text-[#D4D4D8]',
            kindColor,
          )}
        >
          <KindIcon className="h-3 w-3" aria-hidden />
          {kindLabel}
        </span>
        {/* Hover overlay (only when at least one handler is provided) */}
        {(onPreview || onInstall || onUninstall) && (
          <CardHoverOverlay
            onPreview={onPreview}
            onInstall={onInstall}
            onUninstall={onUninstall}
          />
        )}
      </div>

      {/* Metadata footer */}
      <div className="flex flex-col gap-2 px-3.5 py-3">
        <div className="flex items-center justify-between gap-2">
          <p className="truncate text-sm font-medium text-[#FAFAFA]">{name}</p>
          {installCount !== null && (
            <span className="flex flex-shrink-0 items-center gap-1 text-[13px] text-[#71717A]">
              <Download className="h-3.5 w-3.5" aria-hidden />
              {installCount.toLocaleString()}
            </span>
          )}
        </div>
        <p className="text-[13px] text-[#71717A]">
          by {author.split('#')[0] || ''}
          <span className="text-[#52525B]">#{author.split('#')[1] || ''}</span>
        </p>
        {tags.length > 0 && (
          <div className="mt-0.5 flex flex-wrap gap-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="rounded-md border border-[#3F3F46] bg-[#27272A] px-2 py-0.5 text-xs text-[#A1A1AA]"
              >
                {tag}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
