'use client';

import { toast } from '../../lib/toast';
import { usePreview } from '../../lib/preview-context';
import { useShareWs } from '../../hooks/use-share-ws';

export function PreviewBanner() {
  const { activeToken, activeArtifact, clearPreview } = usePreview();
  const { send } = useShareWs();

  if (activeToken === null || activeArtifact === null) return null;

  const handleRevert = async () => {
    try {
      await send('explorer.cancelPreview', { preview_token: activeToken });
    } catch (err) {
      // Swallow — toast the user-facing message. Host drop/disconnect also auto-reverts, so this is defensive.
      toast.error(
        (err as { message?: string; code?: string; kind?: string })?.message != null
          ? (err as Parameters<typeof toast.error>[0])
          : { code: 'REVERT_FAILED', kind: 'HostLocal', message: 'Failed to revert preview' },
      );
    } finally {
      clearPreview();
    }
  };

  const handleInstallNow = () => {
    // Wave 3a stub — real install-inline UX is #016 Wave 3b.
    toast.info('Install flow coming in Wave 3b — preview cleared');
    clearPreview();
  };

  return (
    <div
      data-testid="preview-banner"
      className="flex h-7 items-center justify-between bg-gradient-to-r from-cyan-900/40 via-cyan-700/30 to-cyan-900/40 px-4 text-xs text-cyan-100 shadow-sm"
    >
      <div className="flex items-center gap-2">
        <span
          className="inline-block h-2 w-2 animate-pulse rounded-full bg-cyan-300"
          aria-hidden="true"
        />
        <span>
          Previewing <span className="font-semibold">{activeArtifact.name}</span> · not installed
        </span>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          data-testid="preview-banner-revert"
          onClick={handleRevert}
          className="rounded border border-cyan-400/50 px-2 py-0.5 text-cyan-100 hover:bg-cyan-400/10"
        >
          Revert
        </button>
        <button
          type="button"
          data-testid="preview-banner-install-now"
          onClick={handleInstallNow}
          className="rounded bg-cyan-400 px-2 py-0.5 text-zinc-950 hover:bg-cyan-300"
        >
          ⬇ Install Now
        </button>
      </div>
    </div>
  );
}
