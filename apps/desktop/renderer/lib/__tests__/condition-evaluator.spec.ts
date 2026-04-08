import { describe, it, expect } from 'vitest';
import { buildPreviewStructure } from '../omni-parser';
import type { MetricValues } from '@/types/omni';

// The condition evaluator and metric formatter are internal to omni-parser.
// We test the condition evaluator through buildPreviewStructure which encodes
// class bindings as data attributes. The actual evaluation happens at runtime
// via updatePreviewDOM, but the binding encoding is the testable contract.
//
// For the metric formatter, we test it indirectly by verifying the structure
// preserves placeholders correctly (formatting happens at DOM update time).

const baseMetrics: MetricValues = {
  fps: 144,
  'frame-time': 6.9,
  'frame-time.avg': 7.0,
  'frame-time.1pct': 8.5,
  'frame-time.01pct': 12.1,
  'cpu.usage': 45,
  'cpu.temp': 62,
  'gpu.usage': 78,
  'gpu.temp': 71,
  'gpu.clock': 1950,
  'gpu.mem-clock': 7800,
  'gpu.vram.used': 6144,
  'gpu.vram.total': 8192,
  'gpu.power': 220,
  'gpu.fan': 55,
  'ram.usage': 60,
  'ram.used': 16384,
  'ram.total': 32768,
};

function makeWidget(template: string, style = ''): string {
  return `<widget id="test" name="Test">
  <template>${template}</template>
  <style>${style}</style>
</widget>`;
}

describe('Class Binding Encoding', () => {
  describe('given a single class binding', () => {
    const content = makeWidget('<div class="panel" class:warning="{gpu.temp} > 80">GPU</div>');

    it('should encode the binding as a data attribute', () => {
      const { html } = buildPreviewStructure(content);
      expect(html).toContain('data-omni-bindings=');
      expect(html).toContain(encodeURIComponent('warning:{gpu.temp} > 80'));
    });

    it('should remove the class binding from attributes', () => {
      const { html } = buildPreviewStructure(content);
      expect(html).not.toContain('class:warning');
    });

    it('should preserve the existing class attribute', () => {
      const { html } = buildPreviewStructure(content);
      expect(html).toContain('class="panel"');
    });
  });

  describe('given multiple class bindings on one element', () => {
    const content = makeWidget(
      '<div class:warning="{gpu.temp} > 80" class:critical="{gpu.temp} > 95">GPU</div>',
    );

    it('should encode all bindings separated by pipe', () => {
      const { html } = buildPreviewStructure(content);
      const decoded = decodeURIComponent(html.match(/data-omni-bindings="([^"]+)"/)?.[1] || '');
      expect(decoded).toContain('warning:{gpu.temp} > 80');
      expect(decoded).toContain('critical:{gpu.temp} > 95');
      expect(decoded.split('|')).toHaveLength(2);
    });
  });

  describe('given a complex condition with AND', () => {
    const content = makeWidget(
      '<div class:alert="{gpu.temp} > 80 && {cpu.usage} > 90">Stats</div>',
    );

    it('should preserve the full condition expression', () => {
      const { html } = buildPreviewStructure(content);
      const decoded = decodeURIComponent(html.match(/data-omni-bindings="([^"]+)"/)?.[1] || '');
      expect(decoded).toBe('alert:{gpu.temp} > 80 && {cpu.usage} > 90');
    });
  });

  describe('given a condition with arithmetic', () => {
    const content = makeWidget(
      '<div class:full="{gpu.vram.used} / {gpu.vram.total} > 0.9">VRAM</div>',
    );

    it('should preserve arithmetic in the encoded condition', () => {
      const { html } = buildPreviewStructure(content);
      const decoded = decodeURIComponent(html.match(/data-omni-bindings="([^"]+)"/)?.[1] || '');
      expect(decoded).toBe('full:{gpu.vram.used} / {gpu.vram.total} > 0.9');
    });
  });
});

describe('Preview Structure', () => {
  describe('given metric placeholders in text', () => {
    const content = makeWidget('<span>{fps} FPS</span>');

    it('should preserve placeholders for runtime interpolation', () => {
      const { html } = buildPreviewStructure(content);
      expect(html).toContain('{fps} FPS');
    });
  });

  describe('given multiple widgets with mixed enabled state', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div>{fps}</div></template>
  <style>.fps { color: cyan; }</style>
</widget>
<widget id="gpu" name="GPU" enabled="false">
  <template><div>{gpu.temp}</div></template>
  <style>.gpu { color: red; }</style>
</widget>
<widget id="ram" name="RAM">
  <template><div>{ram.usage}%</div></template>
  <style>.ram { color: green; }</style>
</widget>`;

    it('should only include enabled widgets', () => {
      const { html, css } = buildPreviewStructure(content);
      expect(html).toContain('{fps}');
      expect(html).toContain('{ram.usage}');
      expect(html).not.toContain('{gpu.temp}');
      expect(css).toContain('.fps');
      expect(css).toContain('.ram');
      expect(css).not.toContain('.gpu');
    });
  });
});
