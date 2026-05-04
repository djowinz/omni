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

import { Download, Layers, Palette } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { CachedArtifactDetail, ArtifactDetail } from '../../lib/share-types';
import { CardHoverOverlay } from './card-hover-overlay';

export interface ArtifactCardProps {
  artifact: CachedArtifactDetail | ArtifactDetail;
  installed?: boolean;
  onClick?: () => void;
  onPreview?: () => void;
  onInstall?: () => void;
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
  const { artifact, installed, onClick, onPreview, onInstall, className } = props;
  const name = artifactName(artifact);
  const author = authorDisplay(artifact);
  const detail = artifact as ArtifactDetail;
  const installCount = typeof detail.installs === 'number' ? detail.installs : null;
  const tags = manifestTags(artifact).slice(0, 3);

  const isBundle = artifact.kind === 'bundle';
  const KindIcon = isBundle ? Layers : Palette;
  const kindLabel = installed ? 'Installed' : isBundle ? 'Bundle' : 'Theme';
  const kindColor = installed
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
            'absolute right-2 top-2 flex items-center gap-1 rounded-md border bg-black/60 px-2 py-0.5 text-[10px] text-[#D4D4D8]',
            kindColor,
          )}
        >
          <KindIcon className="h-2.5 w-2.5" aria-hidden />
          {kindLabel}
        </span>
        {/* Hover overlay (only when handlers are provided) */}
        {(onPreview || onInstall) && (
          <CardHoverOverlay
            onPreview={onPreview ?? (() => {})}
            onInstall={onInstall ?? (() => {})}
          />
        )}
      </div>

      {/* Metadata footer */}
      <div className="flex flex-col gap-1.5 px-3 py-2.5">
        <div className="flex items-center justify-between">
          <p className="truncate text-[13px] font-medium text-[#FAFAFA]">{name}</p>
          {installCount !== null && (
            <span className="flex items-center gap-1 text-[11px] text-[#71717A]">
              <Download className="h-2.5 w-2.5" aria-hidden />
              {installCount.toLocaleString()}
            </span>
          )}
        </div>
        <p className="text-[11px] text-[#71717A]">
          by {author.split('#')[0] || ''}
          <span className="text-[#52525B]">#{author.split('#')[1] || ''}</span>
        </p>
        {tags.length > 0 && (
          <div className="mt-0.5 flex flex-wrap gap-1">
            {tags.map((tag) => (
              <span
                key={tag}
                className="rounded-md border border-[#3F3F46] bg-[#27272A] px-2 py-0.5 text-[10px] text-[#A1A1AA]"
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
