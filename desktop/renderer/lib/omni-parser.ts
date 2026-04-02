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
 * Build the initial overlay HTML structure for preview.
 * Metric placeholders are left as data attributes for in-place updates.
 * Class bindings are stored as data attributes for in-place toggling.
 * Returns { html, css } where html has data-omni-* markers for dynamic updates.
 */
export function buildPreviewStructure(content: string): { html: string; css: string } {
  const widgets = parseOmniContent(content);
  const enabledWidgets = widgets.filter(w => w.enabled);

  let html = '';
  let css = '';

  for (const widget of enabledWidgets) {
    const widgetContent = extractWidgetContent(content, widget);
    if (!widgetContent) continue;

    // Process template: preserve class bindings as data attrs, mark text placeholders
    let template = widgetContent.template;

    // Convert class:name="condition" to data attributes and remove from attrs
    template = template.replace(
      /<([a-zA-Z][a-zA-Z0-9-]*)\b((?:[^>"']|"[^"]*"|'[^']*')*)>/g,
      (_, tagName, attrs) => {
        const bindings: Array<{ className: string; condition: string }> = [];
        const bindingRegex = /\s*class:([a-zA-Z0-9_-]+)=["']([^"']+)["']/g;
        let bMatch;
        while ((bMatch = bindingRegex.exec(attrs)) !== null) {
          bindings.push({ className: bMatch[1], condition: bMatch[2] });
        }

        let cleanAttrs = attrs.replace(/\s*class:[a-zA-Z0-9_-]+=["'][^"']+["']/g, '');

        if (bindings.length > 0) {
          const encoded = bindings.map(b => `${b.className}:${b.condition}`).join('|');
          cleanAttrs += ` data-omni-bindings="${encodeURIComponent(encoded)}"`;
        }

        return `<${tagName}${cleanAttrs}>`;
      }
    );

    html += template;
    css += widgetContent.style;
  }

  return { html, css };
}

/**
 * Update an existing preview DOM container in place:
 * - Toggle conditional classes based on current metric values
 * - Replace {metric} text content with current values
 * This avoids destroying/recreating DOM elements, preserving CSS transitions.
 */
export function updatePreviewDOM(container: HTMLElement, metrics: MetricValues): void {
  // 1. Update class bindings on elements with data-omni-bindings
  const boundElements = container.querySelectorAll('[data-omni-bindings]');
  boundElements.forEach(el => {
    const encoded = el.getAttribute('data-omni-bindings');
    if (!encoded) return;

    const bindings = decodeURIComponent(encoded).split('|').map(b => {
      const colonIdx = b.indexOf(':');
      return { className: b.slice(0, colonIdx), condition: b.slice(colonIdx + 1) };
    });

    for (const { className, condition } of bindings) {
      if (evaluateCondition(condition, metrics)) {
        el.classList.add(className);
      } else {
        el.classList.remove(className);
      }
    }
  });

  // 2. Update text content — walk text nodes and replace {metric} placeholders
  // Skip <style> and <script> elements — their text content is CSS/JS, not metric templates
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => {
      const tag = node.parentElement?.tagName?.toLowerCase();
      if (tag === 'style' || tag === 'script') return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const textNode = node as Text;
    const parent = textNode.parentElement;
    if (!parent) continue;

    // Retrieve or store the original template text
    const original = parent.getAttribute('data-omni-text') ?? textNode.textContent;
    if (!original || !original.includes('{')) continue;

    // Store the original template on first visit
    if (!parent.hasAttribute('data-omni-text')) {
      parent.setAttribute('data-omni-text', original);
    }

    const template = parent.getAttribute('data-omni-text') ?? original;
    const updated = template.replace(/\{([^}]+)\}/g, (_, metric) => {
      const path = metric.trim();
      const value = getMetricValue(path, metrics);
      return formatMetricValue(path, value);
    });

    if (textNode.textContent !== updated) {
      textNode.textContent = updated;
    }
  }
}

/**
 * Format a metric value for display, matching the Rust backend's formatting.
 * - Percentages (cpu.usage, gpu.usage, ram.usage): rounded integer
 * - Temperatures: rounded integer, "N/A" if NaN
 * - Power (gpu.power): rounded integer
 * - Frame times: one decimal place
 * - FPS: rounded integer
 * - Integer values (clocks, VRAM, fan): no decimals
 * - Unknown/unavailable: "N/A"
 */
function formatMetricValue(path: string, value: number | null): string {
  if (value === null || (typeof value === 'number' && isNaN(value))) return 'N/A';

  switch (path) {
    // Rounded integer (no decimals)
    case 'cpu.usage':
    case 'gpu.usage':
    case 'ram.usage':
    case 'gpu.power':
    case 'gpu.temp':
    case 'cpu.temp':
    case 'fps':
      return Math.round(value).toString();

    // One decimal place
    case 'frametime':
    case 'frame-time':
    case 'frame-time.avg':
    case 'frame-time.1pct':
    case 'frame-time.01pct':
    case 'frame.1pct':
      return value.toFixed(1);

    // Integer values
    case 'gpu.clock':
    case 'gpu.mem-clock':
    case 'gpu.fan':
    case 'gpu.vram.used':
    case 'gpu.vram.total':
    case 'ram.used':
    case 'ram.total':
      return Math.round(value).toString();

    default:
      // For any unrecognized path, use reasonable formatting
      return Number.isInteger(value) ? value.toString() : value.toFixed(1);
  }
}

/**
 * Replace metric placeholders with formatted values
 */
function replacePlaceholders(template: string, metrics: MetricValues): string {
  let result = template;

  // Replace all {metric} patterns
  const placeholderRegex = /\{([^}]+)\}/g;

  result = result.replace(placeholderRegex, (match, metric) => {
    const path = metric.trim();
    const value = getMetricValue(path, metrics);
    return formatMetricValue(path, value);
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
 * Evaluate class bindings and apply conditional classes per-element.
 * Each element's class:name="condition" bindings are evaluated independently
 * and added to that element's own class="" attribute.
 *
 * Example:
 *   <div class="panel" class:warning="{gpu.temp} > 80">
 * becomes (when gpu.temp is 90):
 *   <div class="panel warning">
 */
function evaluateClassBindings(template: string, metrics: MetricValues): string {
  // Match each opening tag — must handle > inside quoted attribute values.
  // This regex matches: < tagName (attributes including quoted values with >) >
  return template.replace(
    /<([a-zA-Z][a-zA-Z0-9-]*)\b((?:[^>"']|"[^"]*"|'[^']*')*)>/g,
    (fullMatch, tagName, attrs) => {
      // Find all class:name="condition" bindings on this element
      const bindingRegex = /\s*class:([a-zA-Z0-9_-]+)=["']([^"']+)["']/g;
      const classesToAdd: string[] = [];
      let bindingMatch;

      while ((bindingMatch = bindingRegex.exec(attrs)) !== null) {
        const className = bindingMatch[1];
        const condition = bindingMatch[2];
        if (evaluateCondition(condition, metrics)) {
          classesToAdd.push(className);
        }
      }

      // Remove class binding attributes from this element's attrs
      let cleanAttrs = attrs.replace(/\s*class:[a-zA-Z0-9_-]+=["'][^"']+["']/g, '');

      // Add conditional classes to the element's class attribute
      if (classesToAdd.length > 0) {
        const classMatch = /class="([^"]*)"/.exec(cleanAttrs);
        if (classMatch) {
          const existing = classMatch[1];
          const combined = `${existing} ${classesToAdd.join(' ')}`.trim();
          cleanAttrs = cleanAttrs.replace(`class="${existing}"`, `class="${combined}"`);
        } else {
          // No class attribute exists — add one
          cleanAttrs = ` class="${classesToAdd.join(' ')}"${cleanAttrs}`;
        }
      }

      return `<${tagName}${cleanAttrs}>`;
    }
  );
}

/**
 * Evaluate a condition expression against metric values.
 * Supports the same grammar as the Rust backend:
 *   - Comparisons: {metric} > value, {metric} <= value, etc.
 *   - Logical AND: {gpu.temp} > 80 && {cpu.usage} > 90
 *   - Logical OR: {gpu.temp} > 90 || {cpu.temp} > 85
 *   - Negation: !({fps} > 60)
 *   - Parentheses: ({gpu.temp} > 80) && ({cpu.usage} > 90)
 *   - Arithmetic: {gpu.vram.used} / {gpu.vram.total} > 0.9
 */
function evaluateCondition(condition: string, metrics: MetricValues): boolean {
  try {
    return evalOrExpr(condition.trim(), metrics).value;
  } catch {
    return false;
  }
}

interface EvalResult {
  value: boolean;
  rest: string;
}

interface NumResult {
  value: number;
  rest: string;
}

function evalOrExpr(input: string, metrics: MetricValues): EvalResult {
  let { value, rest } = evalAndExpr(input, metrics);
  while (rest.trimStart().startsWith('||')) {
    rest = rest.trimStart().slice(2);
    const right = evalAndExpr(rest, metrics);
    value = value || right.value;
    rest = right.rest;
  }
  return { value, rest };
}

function evalAndExpr(input: string, metrics: MetricValues): EvalResult {
  let { value, rest } = evalNotExpr(input, metrics);
  while (rest.trimStart().startsWith('&&')) {
    rest = rest.trimStart().slice(2);
    const right = evalNotExpr(rest, metrics);
    value = value && right.value;
    rest = right.rest;
  }
  return { value, rest };
}

function evalNotExpr(input: string, metrics: MetricValues): EvalResult {
  const trimmed = input.trimStart();
  if (trimmed.startsWith('!')) {
    const inner = evalNotExpr(trimmed.slice(1), metrics);
    return { value: !inner.value, rest: inner.rest };
  }
  return evalComparison(trimmed, metrics);
}

function evalComparison(input: string, metrics: MetricValues): EvalResult {
  const left = evalNumExpr(input, metrics);
  const rest = left.rest.trimStart();

  const ops = ['<=', '>=', '==', '!=', '<', '>'];
  for (const op of ops) {
    if (rest.startsWith(op)) {
      const right = evalNumExpr(rest.slice(op.length), metrics);
      let value = false;
      switch (op) {
        case '<': value = left.value < right.value; break;
        case '>': value = left.value > right.value; break;
        case '<=': value = left.value <= right.value; break;
        case '>=': value = left.value >= right.value; break;
        case '==': value = left.value === right.value; break;
        case '!=': value = left.value !== right.value; break;
      }
      return { value, rest: right.rest };
    }
  }

  // No comparison operator — treat nonzero as true
  return { value: left.value !== 0, rest: left.rest };
}

function evalNumExpr(input: string, metrics: MetricValues): NumResult {
  let { value, rest } = evalMulExpr(input, metrics);
  let trimmed = rest.trimStart();
  while (trimmed.startsWith('+') || (trimmed.startsWith('-') && !trimmed.startsWith('->'))) {
    const op = trimmed[0];
    const right = evalMulExpr(trimmed.slice(1), metrics);
    value = op === '+' ? value + right.value : value - right.value;
    rest = right.rest;
    trimmed = rest.trimStart();
  }
  return { value, rest };
}

function evalMulExpr(input: string, metrics: MetricValues): NumResult {
  let { value, rest } = evalPrimary(input, metrics);
  let trimmed = rest.trimStart();
  while (trimmed.startsWith('*') || trimmed.startsWith('/')) {
    const op = trimmed[0];
    const right = evalPrimary(trimmed.slice(1), metrics);
    value = op === '*' ? value * right.value : (right.value !== 0 ? value / right.value : 0);
    rest = right.rest;
    trimmed = rest.trimStart();
  }
  return { value, rest };
}

function evalPrimary(input: string, metrics: MetricValues): NumResult {
  const trimmed = input.trimStart();

  // Parenthesized expression
  if (trimmed.startsWith('(')) {
    const inner = evalNumExpr(trimmed.slice(1), metrics);
    const rest = inner.rest.trimStart();
    return { value: inner.value, rest: rest.startsWith(')') ? rest.slice(1) : rest };
  }

  // Metric variable: {metric.path}
  const metricMatch = /^\{([^}]+)\}/.exec(trimmed);
  if (metricMatch) {
    const val = getMetricValue(metricMatch[1].trim(), metrics);
    return { value: val ?? 0, rest: trimmed.slice(metricMatch[0].length) };
  }

  // Bare metric path (without braces): gpu.temp, fps, etc.
  const bareMetricMatch = /^([a-zA-Z][a-zA-Z0-9._-]*)/.exec(trimmed);
  if (bareMetricMatch) {
    const val = getMetricValue(bareMetricMatch[1], metrics);
    if (val !== null) {
      return { value: val, rest: trimmed.slice(bareMetricMatch[0].length) };
    }
  }

  // Number literal
  const numMatch = /^-?\d+(\.\d+)?/.exec(trimmed);
  if (numMatch) {
    return { value: parseFloat(numMatch[0]), rest: trimmed.slice(numMatch[0].length) };
  }

  // Can't parse — return 0
  return { value: 0, rest: trimmed };
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
