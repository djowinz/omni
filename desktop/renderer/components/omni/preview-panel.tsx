

import { useMemo } from 'react';
import { useOmniState } from '@/hooks/use-omni-state';
import { renderOmniPreview } from '@/lib/omni-parser';
import { MetricSimulator } from './metric-simulator';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Monitor } from 'lucide-react';

export function PreviewPanel() {
  const { state, getCurrentOverlay } = useOmniState();
  const currentOverlay = getCurrentOverlay();

  const previewHtml = useMemo(
    () => currentOverlay?.content
      ? renderOmniPreview(currentOverlay.content, state.previewMetrics)
      : '',
    [currentOverlay?.content, state.previewMetrics]
  );

  // Transform CSS to work within the preview container (convert fixed to absolute)
  const scopedPreviewHtml = previewHtml.replace(
    /position:\s*fixed/gi,
    'position: absolute'
  );

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Panel Header */}
      <div className="flex h-10 items-center justify-between border-b border-[#27272A] px-3 bg-[#18181B]">
        <div className="flex items-center gap-2">
          <Monitor className="h-4 w-4 text-[#00D9FF]" />
          <h2 className="text-sm font-medium text-[#FAFAFA]">Preview</h2>
        </div>
        <span className="text-xs text-[#71717A]">Live</span>
      </div>

      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Preview area - simulates in-game overlay */}
        <div className="relative flex-1 overflow-hidden m-2 rounded-lg border border-[#27272A]">
          {/* Game simulation background */}
          <div
            className="absolute inset-0"
            style={{
              background: `
                radial-gradient(ellipse at 30% 20%, rgba(0, 217, 255, 0.03) 0%, transparent 50%),
                radial-gradient(ellipse at 70% 80%, rgba(168, 85, 247, 0.03) 0%, transparent 50%),
                linear-gradient(180deg, #0a0a0c 0%, #111114 50%, #0a0a0c 100%)
              `,
            }}
          >
            {/* Subtle grid pattern to simulate game content */}
            <div
              className="absolute inset-0 opacity-[0.03]"
              style={{
                backgroundImage: `
                  linear-gradient(rgba(0, 217, 255, 0.5) 1px, transparent 1px),
                  linear-gradient(90deg, rgba(0, 217, 255, 0.5) 1px, transparent 1px)
                `,
                backgroundSize: '40px 40px',
              }}
            />

            {/* Fake game scene elements */}
            <div className="absolute inset-0 flex items-center justify-center">
              <div className="text-center opacity-20">
                <div 
                  className="w-24 h-24 mx-auto mb-3 rounded-lg"
                  style={{
                    background: 'linear-gradient(135deg, #00D9FF20 0%, #A855F720 100%)',
                    border: '1px solid #ffffff10'
                  }}
                />
                <span className="text-xs text-[#71717A] uppercase tracking-widest">Game Preview</span>
              </div>
            </div>

            {/* Preview mode indicator */}
            <div className="absolute bottom-3 right-3 flex items-center gap-1.5">
              <div className="h-1.5 w-1.5 rounded-full bg-[#22C55E] animate-pulse" />
              <span className="text-[10px] text-[#71717A] uppercase tracking-wider">Preview Mode</span>
            </div>
          </div>

          {/* Rendered overlay - container for position:absolute elements */}
          {/* SECURITY: dangerouslySetInnerHTML renders user-authored overlay templates.
              Safe in this context: contextIsolation is enabled, nodeIntegration is off,
              and content comes from the user's own files. Before supporting shared/imported
              overlays, add HTML sanitization to strip <script> and event handlers. */}
          <div
            className="absolute inset-0 pointer-events-none overflow-hidden"
            dangerouslySetInnerHTML={{ __html: scopedPreviewHtml }}
          />
        </div>

        {/* Metric simulator */}
        <div className="border-t border-[#27272A] bg-[#18181B]">
          <ScrollArea className="h-[180px]">
            <MetricSimulator />
          </ScrollArea>
        </div>
      </div>
    </div>
  );
}
