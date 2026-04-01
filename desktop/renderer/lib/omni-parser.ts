import type { ParsedWidget, MetricValues, ThemeImport } from '@/types/omni';

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
 * Render .omni content to HTML for preview
 * Replaces metric placeholders with actual values
 */
export function renderOmniPreview(content: string, metrics: MetricValues): string {
  const widgets = parseOmniContent(content);
  const enabledWidgets = widgets.filter(w => w.enabled);
  
  let html = '';
  let css = '';
  
  for (const widget of enabledWidgets) {
    const widgetContent = extractWidgetContent(content, widget);
    if (widgetContent) {
      html += renderWidgetTemplate(widgetContent.template, metrics);
      css += widgetContent.style;
    }
  }
  
  return `<style>${css}</style>${html}`;
}

/**
 * Extract template and style content from a widget
 */
function extractWidgetContent(content: string, widget: ParsedWidget): { template: string; style: string } | null {
  const lines = content.split('\n');
  const widgetLines = lines.slice(widget.startLine - 1, widget.endLine);
  const widgetContent = widgetLines.join('\n');
  
  // Extract template content
  const templateMatch = /<template>([\s\S]*?)<\/template>/i.exec(widgetContent);
  const styleMatch = /<style>([\s\S]*?)<\/style>/i.exec(widgetContent);
  
  if (!templateMatch) return null;
  
  return {
    template: templateMatch[1].trim(),
    style: styleMatch ? styleMatch[1].trim() : '',
  };
}

/**
 * Render a widget template with metric values
 * Also evaluates class bindings
 */
function renderWidgetTemplate(template: string, metrics: MetricValues): string {
  let rendered = template;
  
  // First, evaluate class bindings like class:warning="{fps} < 60"
  rendered = evaluateClassBindings(rendered, metrics);
  
  // Then replace metric placeholders
  rendered = replacePlaceholders(rendered, metrics);
  
  return rendered;
}

/**
 * Replace metric placeholders with actual values
 */
function replacePlaceholders(template: string, metrics: MetricValues): string {
  let result = template;
  
  // Replace all {metric} patterns
  const placeholderRegex = /\{([^}]+)\}/g;
  
  result = result.replace(placeholderRegex, (match, metric) => {
    const value = getMetricValue(metric.trim(), metrics);
    return value !== null ? String(value) : match;
  });
  
  return result;
}

/**
 * Get a metric value by its dot notation path
 */
function getMetricValue(path: string, metrics: MetricValues): number | null {
  // Handle cpu.core.N pattern
  const coreMatch = /^cpu\.core\.(\d+)$/.exec(path);
  if (coreMatch) {
    const index = parseInt(coreMatch[1], 10);
    return metrics['cpu.core'][index] ?? null;
  }
  
  // Direct property access
  const value = (metrics as unknown as Record<string, unknown>)[path];
  if (typeof value === 'number') return value;
  
  return null;
}

/**
 * Evaluate class bindings and apply conditional classes
 * Supports: class:className="{metric} operator value"
 */
function evaluateClassBindings(template: string, metrics: MetricValues): string {
  const bindingRegex = /class:([a-zA-Z0-9_-]+)=["']([^"']+)["']/g;

  // Collect classes whose conditions evaluate to true
  const classesToAdd: string[] = [];
  let match;
  while ((match = bindingRegex.exec(template)) !== null) {
    const className = match[1];
    const condition = match[2];
    if (evaluateCondition(condition, metrics)) {
      classesToAdd.push(className);
    }
  }

  // Remove all class binding attributes first (before position-dependent work)
  let result = template.replace(/\s*class:[a-zA-Z0-9_-]+=["'][^"']+["']/g, '');

  // Add collected classes to the first class="" attribute in the cleaned result
  if (classesToAdd.length > 0) {
    const classAttrMatch = result.match(/class="([^"]*)"/);
    if (classAttrMatch) {
      const existingClasses = classAttrMatch[1];
      const newClasses = `${existingClasses} ${classesToAdd.join(' ')}`.trim();
      result = result.replace(`class="${existingClasses}"`, `class="${newClasses}"`);
    }
  }

  return result;
}

/**
 * Evaluate a condition expression
 * Supports: {metric} < value, {metric} > value, etc.
 */
function evaluateCondition(condition: string, metrics: MetricValues): boolean {
  // Parse condition: {metric} operator value
  const conditionRegex = /\{([^}]+)\}\s*(<=|>=|<|>|==|!=)\s*(\d+(?:\.\d+)?)/;
  const match = conditionRegex.exec(condition);
  
  if (!match) return false;
  
  const metricPath = match[1].trim();
  const operator = match[2];
  const threshold = parseFloat(match[3]);
  
  const value = getMetricValue(metricPath, metrics);
  if (value === null) return false;
  
  switch (operator) {
    case '<': return value < threshold;
    case '>': return value > threshold;
    case '<=': return value <= threshold;
    case '>=': return value >= threshold;
    case '==': return value === threshold;
    case '!=': return value !== threshold;
    default: return false;
  }
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
      const name = src.split('/').pop()?.replace(/\.css$/i, '') || src;
      
      themes.push({
        src,
        name,
        line: i + 1, // 1-indexed
      });
    }
  }
  
  return themes;
}
