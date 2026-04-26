/**
 * Step 1 banners — sidecar recovery surfaces.
 *
 * `LinkedArtifactBanner` (INV-7.6.2): cyan-outlined banner shown at the top
 * of Step 1 when a `.omni-publish.json` sidecar matches the current identity
 * (silent restore from the local publish-index counts as a match — the
 * sidecar gets written back before Step 1 renders).
 *
 * `PubkeyMismatchBanner` (INV-7.6.4): amber warning shown when a sidecar is
 * present but `author_pubkey_hex` differs from the current identity. The
 * upload will proceed as a NEW artifact (not an update) — the banner makes
 * that consequence visible before the user commits.
 *
 * Mockup reference: `sidecar-deleted-recovery.html`.
 */

import { Check, AlertTriangle } from 'lucide-react';
import type { PublishSidecar } from '@omni/shared-types';

export interface BannerProps {
  sidecar: PublishSidecar;
}

export function LinkedArtifactBanner({ sidecar }: BannerProps) {
  const date = sidecar.last_published_at ? sidecar.last_published_at.slice(0, 10) : '';
  return (
    <div
      data-testid="linked-artifact-banner"
      className="px-3 py-2 mb-3 rounded-md border border-[#00D9FF]/40 bg-[#00D9FF]/5 flex gap-2 items-start"
    >
      <Check className="w-3.5 h-3.5 text-[#00D9FF] shrink-0 mt-0.5" strokeWidth={2} />
      <div className="text-xs leading-relaxed">
        <div className="font-semibold text-[#00D9FF] mb-0.5">Linked to existing artifact</div>
        <div className="text-[#a1a1aa]">
          <code className="text-[#d4d4d8]">{sidecar.artifact_id.slice(0, 12)}…</code>
          {' · '}last published v{sidecar.version} on {date}. This upload will be an update.
        </div>
      </div>
    </div>
  );
}

export function PubkeyMismatchBanner({ sidecar }: BannerProps) {
  return (
    <div
      data-testid="pubkey-mismatch-banner"
      className="px-3 py-2 mb-3 rounded-md border border-amber-700/50 bg-amber-900/10 flex gap-2 items-start"
    >
      <AlertTriangle className="w-3.5 h-3.5 text-amber-500 shrink-0 mt-0.5" strokeWidth={2} />
      <div className="text-xs leading-relaxed">
        <div className="font-semibold text-amber-500 mb-0.5">
          Originally published by a different identity
        </div>
        <div className="text-[#a1a1aa]">
          By <code className="text-[#d4d4d8]">{sidecar.author_pubkey_hex.slice(0, 12)}…</code>
          {' · '}This upload will be a new artifact.
        </div>
      </div>
    </div>
  );
}
