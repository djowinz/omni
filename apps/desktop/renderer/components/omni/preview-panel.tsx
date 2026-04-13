import { useEffect, useRef, useCallback } from 'react';
import { useOmniState } from '@/hooks/use-omni-state';
import { useBackend } from '@/hooks/use-backend';
import { applyPreviewDiff, type PreviewDiff } from '@/lib/preview-updater';
import { useSensorData } from '@/hooks/use-sensor-data';
import { SensorReadout } from './sensor-readout';
import { Monitor } from 'lucide-react';

export function PreviewPanel() {
  const { state } = useOmniState();
  const backend = useBackend();
  const sensorData = useSensorData();
  const iframeRef = useRef<HTMLIFrameElement>(null);

  // Subscribe to preview when connected
  useEffect(() => {
    if (!state.connected) return;
    backend.subscribePreview();
  }, [state.connected, backend]);

  // Handle preview.html — write full document into iframe
  const handlePreviewHtml = useCallback((data: { html: string; css: string }) => {
    const doc = iframeRef.current?.contentDocument;
    if (!doc) return;

    // Write a complete HTML document. position:fixed inside the iframe
    // resolves relative to the iframe's viewport, not the outer window.
    doc.open();
    doc.write(`<!DOCTYPE html>
<html><head><style>
*{margin:0;padding:0;box-sizing:border-box}
html,body{width:100%;height:100%;background:transparent;overflow:hidden}
${data.css}
</style></head><body>${data.html}</body></html>`);
    doc.close();
  }, []);

  // Handle preview.update — incremental diff applied to iframe document
  const handlePreviewUpdate = useCallback((data: { diff: PreviewDiff }) => {
    const body = iframeRef.current?.contentDocument?.body;
    if (!body) return;
    applyPreviewDiff(body, data.diff);
  }, []);

  // Register IPC listeners
  useEffect(() => {
    const cleanupHtml = window.omni?.onPreviewHtml?.(handlePreviewHtml);
    const cleanupUpdate = window.omni?.onPreviewUpdate?.(handlePreviewUpdate);

    return () => {
      cleanupHtml?.();
      cleanupUpdate?.();
    };
  }, [handlePreviewHtml, handlePreviewUpdate]);

  // Blank on disconnect
  useEffect(() => {
    if (!state.connected) {
      const doc = iframeRef.current?.contentDocument;
      if (doc) {
        doc.open();
        doc.write('<!DOCTYPE html><html><head></head><body></body></html>');
        doc.close();
      }
    }
  }, [state.connected]);

  return (
    <div className="flex h-full flex-col bg-[#0D0D0F]">
      {/* Panel Header */}
      <div className="flex h-10 items-center justify-between border-b border-[#27272A] px-3 bg-[#18181B]">
        <div className="flex items-center gap-2">
          <Monitor className="h-4 w-4 text-[#00D9FF]" />
          <h2 className="text-sm font-medium text-[#FAFAFA]">Preview</h2>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] text-[#71717A] uppercase tracking-wider">
            {state.connected ? 'Host-driven' : 'Disconnected'}
          </span>
        </div>
      </div>

      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Preview area */}
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

          {/* Rendered overlay inside an iframe. The iframe creates its own viewport
              so position:fixed resolves relative to the preview area, not the outer
              window. CSS is fully isolated — no Tailwind bleed, no :root issues.
              SECURITY: innerHTML renders user-authored overlay templates.
              Safe in this context: contextIsolation is enabled, nodeIntegration is off,
              and content comes from the user's own files. Before supporting shared/imported
              overlays, add HTML sanitization to strip <script> and event handlers. */}
          <iframe
            ref={iframeRef}
            className="absolute inset-0 w-full h-full pointer-events-none"
            style={{ border: 'none', background: 'transparent' }}
          />
        </div>

        {/* Bottom panel: live sensor readout */}
        {sensorData && (
          <div className="border-t border-[#27272A] bg-[#18181B]">
            <SensorReadout snapshot={sensorData.snapshot} hwinfo={sensorData.hwinfo} />
          </div>
        )}
      </div>
    </div>
  );
}
