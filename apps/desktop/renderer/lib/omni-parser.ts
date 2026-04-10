import type { ParsedWidget, ThemeImport } from '@/types/omni';

/**
 * Parse .omni file content and extract widget metadata
 */
export function parseOmniContent(content: string): ParsedWidget[] {
  const widgets: ParsedWidget[] = [];
  const lines = content.split('\n');

  // Regex to match widget opening tags
  const widgetOpenRegex = /<widget\s+([^>]*)>/gi;
  const widgetCloseRegex = /<\/widget>/gi;

  let match;

  // Find all widget tags and their line positions
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Check for widget opening tag
    widgetOpenRegex.lastIndex = 0;
    match = widgetOpenRegex.exec(line);

    if (match) {
      const attrs = match[1];
      const id = extractAttribute(attrs, 'id') || `widget-${widgets.length}`;
      const name = extractAttribute(attrs, 'name') || id;
      const enabled = extractAttribute(attrs, 'enabled') !== 'false';

      // Find the closing tag
      let endLine = i;
      for (let j = i; j < lines.length; j++) {
        widgetCloseRegex.lastIndex = 0;
        if (widgetCloseRegex.test(lines[j])) {
          endLine = j;
          break;
        }
      }

      widgets.push({
        id,
        name,
        enabled,
        startLine: i + 1, // 1-indexed for editor
        endLine: endLine + 1,
      });
    }
  }

  return widgets;
}

/**
 * Extract an attribute value from an attribute string
 */
function extractAttribute(attrs: string, name: string): string | null {
  // Match both single and double quoted values
  const regex = new RegExp(`${name}=["']([^"']*)["']`, 'i');
  const match = regex.exec(attrs);
  return match ? match[1] : null;
}

/**
 * Toggle a widget's enabled state in .omni content
 */
export function toggleWidgetEnabled(content: string, widgetId: string, enabled: boolean): string {
  const lines = content.split('\n');
  const result: string[] = [];

  for (const line of lines) {
    // Check if this line contains the widget with the given id
    const widgetRegex = new RegExp(`<widget\\s+([^>]*id=["']${widgetId}["'][^>]*)>`, 'i');
    const match = widgetRegex.exec(line);

    if (match) {
      const attrs = match[1];
      let newAttrs = attrs;

      // Check if enabled attribute exists
      if (/enabled=["'][^"']*["']/i.test(attrs)) {
        // Replace existing enabled attribute
        newAttrs = attrs.replace(/enabled=["'][^"']*["']/i, `enabled="${enabled}"`);
      } else {
        // Add enabled attribute
        newAttrs = `${attrs} enabled="${enabled}"`;
      }

      result.push(line.replace(match[1], newAttrs));
    } else {
      result.push(line);
    }
  }

  return result.join('\n');
}

/**
 * Parse theme imports from .omni file content
 * Themes are imported via <theme src="path/to/theme.css" /> or <theme src="..." />
 */
export function parseThemeImports(content: string): ThemeImport[] {
  const themes: ThemeImport[] = [];
  const lines = content.split('\n');

  // Match <theme src="..." /> patterns
  const themeRegex = /<theme\s+src=["']([^"']+)["']\s*\/?>/gi;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let match;

    themeRegex.lastIndex = 0;
    while ((match = themeRegex.exec(line)) !== null) {
      const src = match[1];
      // Extract name from path (e.g., "themes/neon.css" -> "neon")
      const name =
        src
          .split('/')
          .pop()
          ?.replace(/\.css$/i, '') || src;

      themes.push({
        src,
        name,
        line: i + 1, // 1-indexed
      });
    }
  }

  return themes;
}
