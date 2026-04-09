import { useState, useMemo, useRef, useEffect, useCallback } from 'react';
import { useOmniState } from '@/hooks/use-omni-state';
import { buildPreviewStructure, updatePreviewDOM, parseThemeImports } from '@/lib/omni-parser';
import { useBackend } from '@/hooks/use-backend';
import { MetricSimulator } from './metric-simulator';
import { SensorReadout } from './sensor-readout';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Monitor } from 'lucide-react';
import { useSensorData } from '@/hooks/use-sensor-data';
import { sensorSnapshotToMetrics } from '@/lib/sensor-mapping';
import { DEFAULT_METRICS } from '@/types/omni';
import type { MetricValues } from '@/types/omni';
import { cn } from '@/lib/utils';

export function PreviewPanel() {
  const { state, getCurrentOverlay } = useOmniState();
  const currentOverlay = getCurrentOverlay();

  const [mode, setMode] = useState<'live' | 'simulate'>('live');
  const sensorData = useSensorData();
  const isConnected = sensorData !== null;

  // Force simulate when disconnected
  const effectiveMode = isConnected ? mode : 'simulate';

  // Determine which metrics to use for preview
  const activeMetrics: MetricValues = useMemo(() => {
    if (effectiveMode === 'live' && sensorData) {
      return {
        ...DEFAULT_METRICS,
        ...sensorSnapshotToMetrics(sensorData.snapshot, sensorData.hwinfo),
      };
    }
    return state.previewMetrics;
  }, [effectiveMode, sensorData, state.previewMetrics]);

  const overlayRef = useRef<HTMLDivElement>(null);
  const backend = useBackend();
  const [themeCss, setThemeCss] = useState('');

  // Load theme CSS when overlay content changes
  useEffect(() => {
    if (!currentOverlay?.content) {
      setThemeCss('');
      return;
    }
    const themes = parseThemeImports(currentOverlay.content);
    if (themes.length === 0) {
      setThemeCss('');
      return;
    }
    // Load all theme files and concatenate their CSS
    Promise.all(
      themes.map(async (t) => {
        const path = t.src.startsWith('themes/') ? t.src : `themes/${t.src}`;
        try {
          return await backend.readFile(path);
        } catch {
          return '';
        }
      }),
    ).then((results) => setThemeCss(results.join('\n')));
  }, [currentOverlay?.content, backend]);

  // Build the overlay structure once when content changes (preserves DOM for transitions)
  const structure = useMemo(
    () => (currentOverlay?.content ? buildPreviewStructure(currentOverlay.content) : null),
    [currentOverlay?.content],
  );

  // Set the static HTML structure when it changes
  useEffect(() => {
    if (!overlayRef.current || !structure) return;
    const scopedCss = structure.css.replace(/position:\s*fixed/gi, 'position: absolute');
    const scopedHtml = structure.html.replace(/position:\s*fixed/gi, 'position: absolute');
    overlayRef.current.innerHTML = `<style>${themeCss}\n${scopedCss}</style>${scopedHtml}`;
  }, [structure, themeCss]);

  // Update metrics in place (class toggling + text replacement) — no DOM recreation
  useEffect(() => {
    if (!overlayRef.current || !structure) return;
    updatePreviewDOM(overlayRef.current, activeMetrics);
  }, [activeMetrics, structure]);

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Panel Header */}
      <div className="flex h-10 items-center justify-between border-b border-[#27272A] px-3 bg-[#18181B]">
        <div className="flex items-center gap-2">
          <Monitor className="h-4 w-4 text-[#00D9FF]" />
          <h2 className="text-sm font-medium text-[#FAFAFA]">Preview</h2>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-[10px] text-[#52525B]">Click to toggle</span>
          <button
            onClick={() => setMode((m) => (m === 'live' ? 'simulate' : 'live'))}
            disabled={!isConnected}
            className={cn(
              'text-xs px-2 py-0.5 rounded transition-colors',
              effectiveMode === 'live'
                ? 'bg-[#22C55E]/20 text-[#22C55E]'
                : 'bg-[#A855F7]/20 text-[#A855F7]',
              !isConnected && 'opacity-50 cursor-not-allowed',
            )}
          >
            {effectiveMode === 'live' ? (
              <>
                <span className="animate-pulse">●</span> Live
              </>
            ) : (
              '◆ Simulate'
            )}
          </button>
        </div>
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
                    border: '1px solid #ffffff10',
                  }}
                />
                <span className="text-xs text-[#71717A] uppercase tracking-widest">
                  Game Preview
                </span>
              </div>
            </div>

            {/* Preview mode indicator */}
            <div className="absolute bottom-3 right-3 flex items-center gap-1.5">
              <span className="text-[10px] text-[#71717A] uppercase tracking-wider">
                Preview Mode
              </span>
            </div>
          </div>

          {/* Rendered overlay — DOM is created once and updated in place for CSS transitions.
              SECURITY: innerHTML renders user-authored overlay templates.
              Safe in this context: contextIsolation is enabled, nodeIntegration is off,
              and content comes from the user's own files. Before supporting shared/imported
              overlays, add HTML sanitization to strip <script> and event handlers. */}
          <div ref={overlayRef} className="absolute inset-0 pointer-events-none overflow-hidden" />
        </div>

        {/* Bottom panel: live sensor readout or metric simulator */}
        <div className="border-t border-[#27272A] bg-[#18181B]">
          {effectiveMode === 'live' && sensorData ? (
            <SensorReadout snapshot={sensorData.snapshot} hwinfo={sensorData.hwinfo} />
          ) : (
            <ScrollArea className="h-[180px]">
              <MetricSimulator />
            </ScrollArea>
          )}
        </div>
      </div>
    </div>
  );
}
