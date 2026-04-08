import { describe, it, expect } from 'vitest';
import {
  parseOmniContent,
  toggleWidgetEnabled,
  buildPreviewStructure,
  parseThemeImports,
} from '../omni-parser';

describe('parseOmniContent', () => {
  describe('given a file with a single widget', () => {
    const content = `<widget id="fps" name="FPS Counter">
  <template><div>{fps}</div></template>
  <style>.fps { color: white; }</style>
</widget>`;

    it('should extract the widget metadata', () => {
      const widgets = parseOmniContent(content);
      expect(widgets).toHaveLength(1);
      expect(widgets[0]).toEqual({
        id: 'fps',
        name: 'FPS Counter',
        enabled: true,
        startLine: 1,
        endLine: 4,
      });
    });
  });

  describe('given a file with multiple widgets', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div>{fps}</div></template>
</widget>
<widget id="gpu" name="GPU Stats">
  <template><div>{gpu.temp}</div></template>
</widget>`;

    it('should extract all widgets', () => {
      const widgets = parseOmniContent(content);
      expect(widgets).toHaveLength(2);
      expect(widgets[0].id).toBe('fps');
      expect(widgets[1].id).toBe('gpu');
    });
  });

  describe('given a widget with enabled="false"', () => {
    const content = `<widget id="fps" name="FPS" enabled="false">
  <template><div>{fps}</div></template>
</widget>`;

    it('should mark the widget as disabled', () => {
      const widgets = parseOmniContent(content);
      expect(widgets[0].enabled).toBe(false);
    });
  });

  describe('given a widget without an id', () => {
    const content = `<widget name="Unnamed">
  <template><div>hello</div></template>
</widget>`;

    it('should generate a fallback id', () => {
      const widgets = parseOmniContent(content);
      expect(widgets[0].id).toBe('widget-0');
      expect(widgets[0].name).toBe('Unnamed');
    });
  });

  describe('given empty content', () => {
    it('should return an empty array', () => {
      expect(parseOmniContent('')).toEqual([]);
    });
  });
});

describe('toggleWidgetEnabled', () => {
  describe('given a widget with an existing enabled attribute', () => {
    const content = `<widget id="fps" name="FPS" enabled="true">
  <template><div>{fps}</div></template>
</widget>`;

    it('should replace the enabled value', () => {
      const result = toggleWidgetEnabled(content, 'fps', false);
      expect(result).toContain('enabled="false"');
      expect(result).not.toContain('enabled="true"');
    });
  });

  describe('given a widget without an enabled attribute', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div>{fps}</div></template>
</widget>`;

    it('should add the enabled attribute', () => {
      const result = toggleWidgetEnabled(content, 'fps', false);
      expect(result).toContain('enabled="false"');
    });
  });

  describe('given a widget id that does not exist', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div>{fps}</div></template>
</widget>`;

    it('should leave the content unchanged', () => {
      const result = toggleWidgetEnabled(content, 'nonexistent', false);
      expect(result).toBe(content);
    });
  });
});

describe('buildPreviewStructure', () => {
  describe('given a widget with template and style', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div class="fps">{fps} FPS</div></template>
  <style>.fps { color: cyan; }</style>
</widget>`;

    it('should extract html and css', () => {
      const result = buildPreviewStructure(content);
      expect(result.html).toContain('class="fps"');
      expect(result.html).toContain('{fps} FPS');
      expect(result.css).toContain('.fps { color: cyan; }');
    });
  });

  describe('given a disabled widget', () => {
    const content = `<widget id="fps" name="FPS" enabled="false">
  <template><div>{fps}</div></template>
  <style>.fps { color: cyan; }</style>
</widget>`;

    it('should exclude it from the output', () => {
      const result = buildPreviewStructure(content);
      expect(result.html).toBe('');
      expect(result.css).toBe('');
    });
  });

  describe('given a widget with class bindings', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div class="panel" class:warning="{gpu.temp} > 80">{fps}</div></template>
  <style>.panel { color: white; }</style>
</widget>`;

    it('should convert class bindings to data attributes', () => {
      const result = buildPreviewStructure(content);
      expect(result.html).toContain('data-omni-bindings=');
      expect(result.html).not.toContain('class:warning');
      expect(result.html).toContain('class="panel"');
    });

    it('should encode the binding condition', () => {
      const result = buildPreviewStructure(content);
      const encoded = encodeURIComponent('warning:{gpu.temp} > 80');
      expect(result.html).toContain(encoded);
    });
  });

  describe('given a widget with no template', () => {
    const content = `<widget id="empty" name="Empty">
  <style>.empty { color: red; }</style>
</widget>`;

    it('should skip the widget', () => {
      const result = buildPreviewStructure(content);
      expect(result.html).toBe('');
      expect(result.css).toBe('');
    });
  });
});

describe('parseThemeImports', () => {
  describe('given a file with theme imports', () => {
    const content = `<theme src="themes/neon.css" />
<theme src="themes/dark.css" />
<widget id="fps" name="FPS">
  <template><div>{fps}</div></template>
</widget>`;

    it('should extract all theme imports', () => {
      const themes = parseThemeImports(content);
      expect(themes).toHaveLength(2);
      expect(themes[0]).toEqual({ src: 'themes/neon.css', name: 'neon', line: 1 });
      expect(themes[1]).toEqual({ src: 'themes/dark.css', name: 'dark', line: 2 });
    });
  });

  describe('given a file with no themes', () => {
    const content = `<widget id="fps" name="FPS">
  <template><div>{fps}</div></template>
</widget>`;

    it('should return an empty array', () => {
      expect(parseThemeImports(content)).toEqual([]);
    });
  });

  describe('given a self-closing theme tag without trailing slash', () => {
    const content = `<theme src="themes/retro.css">`;

    it('should still extract the theme', () => {
      const themes = parseThemeImports(content);
      expect(themes).toHaveLength(1);
      expect(themes[0].name).toBe('retro');
    });
  });
});
