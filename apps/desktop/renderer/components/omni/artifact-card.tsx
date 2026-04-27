import type { ReactNode } from 'react';
import { MoreVertical } from 'lucide-react';

import { cn } from '@/lib/utils';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import type { CachedArtifactDetail, ArtifactDetail } from '../../lib/share-types';

export interface ArtifactCardActionSlots {
  /** Passive/view action — e.g. Preview, Open. Neutral style. */
  left?: ReactNode;
  /** State-toggle action — e.g. Install, Uninstall, Delete. Position stable across tabs. */
  middle?: ReactNode;
  /** Derivative action — e.g. Fork, Update. Usually accent style. */
  right?: ReactNode;
}

export interface ArtifactCardProps {
  /** Grid variant renders the compact card; detail renders the right-panel full view. */
  variant?: 'grid' | 'detail';
  /** Artifact summary (grid) or full detail (detail). */
  artifact: CachedArtifactDetail | ArtifactDetail;
  /** Whether this artifact is installed in the user's workspace. */
  installed?: boolean;
  /** Consumer-provided action buttons for the three fixed slots (detail variant only). */
  actionSlots?: ArtifactCardActionSlots;
  /** Consumer-provided kebab menu items (detail variant only). */
  kebabMenuItems?: ReactNode;
  /** Click handler for the whole card (grid variant). */
  onClick?: () => void;
  className?: string;
  /** Selection marker forwarded onto the grid-variant root element. */
  'data-selected'?: 'true' | 'false';
}

/** Safely read a string field out of a manifest record. */
function manifestString(manifest: Record<string, unknown> | undefined, key: string): string | null {
  if (!manifest) return null;
  const v = manifest[key];
  return typeof v === 'string' && v.length > 0 ? v : null;
}

/** Derive the artifact display name from available fields. */
function artifactName(artifact: CachedArtifactDetail | ArtifactDetail): string {
  // CachedArtifactDetail has a top-level `name` field.
  const cached = artifact as CachedArtifactDetail;
  if (typeof cached.name === 'string' && cached.name.length > 0) return cached.name;
  // ArtifactDetail stores name inside the manifest.
  const detail = artifact as ArtifactDetail;
  if (detail.manifest) {
    const fromManifest = manifestString(detail.manifest as Record<string, unknown>, 'name');
    if (fromManifest) return fromManifest;
  }
  return artifact.artifact_id;
}

/** Derive a human-readable author label from available fields. */
function authorDisplay(artifact: CachedArtifactDetail | ArtifactDetail): string {
  // Per identity-completion-and-display-name spec (2026-04-26):
  // Presentation handle is `<display_name>#<8-hex>`. The 8-hex is the
  // canonical disambiguator (always shown — it's the trust anchor); the
  // name is the friendly prefix sourced from `author_display_name` on the
  // list/gallery/artifact response, or null when the worker has nothing
  // for the author.
  const slice = artifact.author_pubkey.slice(0, 8);
  const detail = artifact as ArtifactDetail & CachedArtifactDetail;
  const name = detail.author_display_name?.trim();
  return name ? `${name}#${slice}` : `#${slice}`;
}

function GridCard(props: ArtifactCardProps) {
  const { artifact, installed, onClick, className } = props;
  const name = artifactName(artifact);
  const author = authorDisplay(artifact);
  const detail = artifact as ArtifactDetail;
  const installCount = typeof detail.installs === 'number' ? detail.installs : null;

  const badgeLabel = installed ? 'Installed' : artifact.kind === 'bundle' ? 'Bundle' : 'Theme';
  const badgeClass = installed
    ? 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30'
    : 'bg-zinc-700/60 text-zinc-400 border-zinc-600/40';

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
        'relative flex flex-col rounded-lg border border-zinc-800 bg-zinc-900 overflow-hidden',
        onClick &&
          'cursor-pointer transition-colors hover:border-zinc-700 hover:bg-zinc-800/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400/50',
        className,
      )}
    >
      {/* Thumbnail */}
      <div className="relative aspect-video w-full overflow-hidden bg-zinc-800">
        {artifact.thumbnail_url ? (
          <img src={artifact.thumbnail_url} alt={name} className="h-full w-full object-cover" />
        ) : (
          <div className="h-full w-full bg-gradient-to-br from-zinc-700 to-zinc-800" />
        )}

        {/* Top-right badge */}
        <span
          className={cn(
            'absolute right-2 top-2 rounded-full border px-2 py-0.5 text-xs font-medium',
            badgeClass,
          )}
        >
          {badgeLabel}
        </span>
      </div>

      {/* Metadata footer */}
      <div className="flex flex-col gap-0.5 px-3 py-2">
        <p className="truncate text-sm font-medium text-zinc-100">{name}</p>
        <p className="text-xs text-zinc-500">
          by {author}
          {installCount !== null && (
            <span className="ml-1 text-zinc-600">· {installCount.toLocaleString()} installs</span>
          )}
        </p>
      </div>
    </div>
  );
}

function DetailCard({ artifact, actionSlots, kebabMenuItems, className }: ArtifactCardProps) {
  const name = artifactName(artifact);
  const detail = artifact as ArtifactDetail;
  const author = authorDisplay(artifact);

  // Fingerprint TOFU pill — show first 6 hex chars if available
  const fingerprintHex =
    typeof detail.author_fingerprint_hex === 'string' && detail.author_fingerprint_hex.length >= 6
      ? detail.author_fingerprint_hex.slice(0, 6)
      : null;

  // Stats from ArtifactDetail fields
  const installs = typeof detail.installs === 'number' ? detail.installs.toLocaleString() : '—';
  const updatedAt =
    typeof detail.updated_at === 'number' && detail.updated_at > 0
      ? new Date(detail.updated_at * 1000).toLocaleDateString()
      : typeof (artifact as CachedArtifactDetail).updated_at === 'number' &&
          (artifact as CachedArtifactDetail).updated_at > 0
        ? new Date((artifact as CachedArtifactDetail).updated_at * 1000).toLocaleDateString()
        : '—';

  // Manifest-derived fields
  const manifest =
    detail.manifest != null ? (detail.manifest as Record<string, unknown>) : undefined;
  const version = manifestString(manifest, 'version') ?? '—';
  const license = manifestString(manifest, 'license') ?? '—';
  const description = manifestString(manifest, 'description') ?? null;
  const rawTags = manifest?.['tags'];
  const tags: string[] =
    Array.isArray(rawTags) && rawTags.every((t) => typeof t === 'string')
      ? (rawTags as string[])
      : [];

  return (
    <div
      data-testid="artifact-card-detail"
      className={cn(
        'flex flex-col gap-4 rounded-lg border border-zinc-800 bg-zinc-900 p-4',
        className,
      )}
    >
      {/* Header: title + kebab */}
      <div className="flex items-start justify-between gap-2">
        <h2 className="text-lg font-semibold text-zinc-100 leading-snug">{name}</h2>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              data-testid="artifact-card-kebab"
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-zinc-700 bg-zinc-800 text-zinc-400 transition-colors hover:border-zinc-600 hover:text-zinc-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400/50"
              aria-label="More options"
            >
              <MoreVertical className="h-4 w-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">{kebabMenuItems ?? null}</DropdownMenuContent>
        </DropdownMenu>
      </div>

      {/* Author row */}
      <div className="flex items-center gap-2 text-sm text-zinc-500">
        <span>by {author}</span>
        {fingerprintHex && (
          <span className="rounded border border-zinc-700 bg-zinc-800 px-1.5 py-0.5 font-mono text-xs text-zinc-400">
            {fingerprintHex}
          </span>
        )}
      </div>

      {/* Hero thumbnail */}
      <div className="aspect-video w-full overflow-hidden rounded-md bg-zinc-800">
        {artifact.thumbnail_url ? (
          <img src={artifact.thumbnail_url} alt={name} className="h-full w-full object-cover" />
        ) : (
          <div className="h-full w-full bg-gradient-to-br from-zinc-700 to-zinc-800" />
        )}
      </div>

      {/* Description */}
      {description && (
        <p className="whitespace-pre-wrap text-sm text-zinc-400 leading-relaxed">{description}</p>
      )}

      {/* Stats grid */}
      <div className="grid grid-cols-4 gap-2">
        {(
          [
            { label: 'Installs', value: installs },
            { label: 'Updated', value: updatedAt },
            { label: 'Version', value: version },
            { label: 'License', value: license },
          ] as const
        ).map(({ label, value }) => (
          <div
            key={label}
            className="flex flex-col items-center rounded-md border border-zinc-800 bg-zinc-800/40 px-2 py-2 text-center"
          >
            <span className="text-xs text-zinc-500">{label}</span>
            <span className="mt-0.5 text-sm font-medium text-zinc-200">{value}</span>
          </div>
        ))}
      </div>

      {/* Tag chips */}
      {tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {tags.map((tag) => (
            <span
              key={tag}
              className="rounded-full border border-zinc-700 bg-zinc-800/60 px-2 py-0.5 text-xs text-zinc-400"
            >
              {tag}
            </span>
          ))}
        </div>
      )}

      {/* Action row — three fixed slots */}
      <div className="flex items-center gap-2">
        <div data-testid="artifact-card-action-slot-left">{actionSlots?.left ?? null}</div>
        <div data-testid="artifact-card-action-slot-middle" className="flex-1">
          {actionSlots?.middle ?? null}
        </div>
        <div data-testid="artifact-card-action-slot-right">{actionSlots?.right ?? null}</div>
      </div>
    </div>
  );
}

export function ArtifactCard(props: ArtifactCardProps) {
  const { variant = 'grid' } = props;
  if (variant === 'detail') {
    return <DetailCard {...props} />;
  }
  return <GridCard {...props} />;
}
