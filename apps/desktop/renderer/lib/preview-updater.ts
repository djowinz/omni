export interface PreviewDiff {
  [omniId: string]: { c?: string; t?: string; a?: Record<string, string> };
}

/**
 * Raw sensor-path → numeric value map. Mirrors the `values` field of the
 * host's `preview.update` wire payload (see crates/host/src/omni/preview.rs).
 */
export type PreviewValues = Record<string, number>;

export function applyPreviewDiff(container: HTMLElement, diff: PreviewDiff): void {
  for (const [id, update] of Object.entries(diff)) {
    const el = container.querySelector(`[data-omni-id="${id}"]`);
    if (!el) continue;
    if (update.c !== undefined) {
      // Use setAttribute so this works for both HTML and SVG elements.
      // SVG elements have className as an SVGAnimatedString, not a writable string.
      el.setAttribute('class', update.c);
    }
    if (update.t !== undefined) {
      // Target only the first text node child — matches Ultralight's omniUpdate behavior.
      // Using el.textContent would destroy child elements in mixed-content nodes.
      for (const n of el.childNodes) {
        if (n.nodeType === Node.TEXT_NODE) {
          n.textContent = update.t;
          break;
        }
      }
    }
    if (update.a) {
      for (const [name, value] of Object.entries(update.a)) {
        el.setAttribute(name, value);
      }
    }
  }
}

// ── Sensor-value updater ────────────────────────────────────────────────────
//
// The host emits two streams in every `preview.update`:
//   - `diff` — handled above.
//   - `values` — raw sensor paths → numbers, applied here.
//
// Inside Ultralight (the actual overlay window) `bootstrap.js::__omni_update`
// handles `values` by walking `[data-sensor]` spans, formatting via the
// span's `data-sensor-format` + `data-sensor-precision` attributes, setting
// textContent (or other targets), and toggling threshold classes. The editor
// preview iframe doesn't run bootstrap.js, so this function reproduces the
// same behaviour. It MUST stay in sync with `crates/host/src/omni/bootstrap.js`
// — divergence would mean the editor preview displays different values than
// the actual overlay.

const CLASS_RE = /^[a-zA-Z0-9_\-\s]*$/;
const IDENT_RE = /^[a-zA-Z_][a-zA-Z0-9_\-]*$/;

function parsePrecision(raw: string | null): number {
  const n = parseInt(raw ?? '0', 10);
  if (!Number.isFinite(n) || n < 0 || n > 6) return 0;
  return n;
}

function parseThreshold(raw: string | null): number | null {
  if (raw === null || raw === undefined || raw === '') return null;
  const n = parseFloat(raw);
  return Number.isFinite(n) ? n : null;
}

function formatValue(raw: number, format: string, precision: number): string {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) return 'N/A';
  switch (format) {
    case 'percent': {
      // Match bootstrap exactly: 0..1 floats are scaled to 0..100; values >1
      // are passed through. Sensor sources mix both conventions.
      const v = raw <= 1 ? raw * 100 : raw;
      return v.toFixed(precision) + '%';
    }
    case 'bytes': {
      const units = ['B', 'KB', 'MB', 'GB', 'TB'];
      let v = raw;
      let i = 0;
      while (v >= 1024 && i < units.length - 1) {
        v /= 1024;
        i++;
      }
      return v.toFixed(precision) + ' ' + units[i];
    }
    case 'temperature':
      return raw.toFixed(precision) + '°C';
    case 'frequency': {
      if (raw >= 1_000_000_000) return (raw / 1_000_000_000).toFixed(precision) + ' GHz';
      if (raw >= 1_000_000) return (raw / 1_000_000).toFixed(precision) + ' MHz';
      if (raw >= 1_000) return (raw / 1_000).toFixed(precision) + ' kHz';
      return raw.toFixed(precision) + ' Hz';
    }
    case 'raw':
    default:
      return raw.toFixed(precision);
  }
}

function applyTarget(el: Element, target: string, formatted: string): void {
  if (target === 'text') {
    if (el.textContent !== formatted) el.textContent = formatted;
    return;
  }
  if (target === 'class') {
    if (!CLASS_RE.test(formatted)) return;
    if (el.getAttribute('class') !== formatted) el.setAttribute('class', formatted);
    return;
  }
  if (target.startsWith('attr:')) {
    const name = target.slice(5);
    if (!IDENT_RE.test(name)) return;
    if (el.getAttribute(name) !== formatted) el.setAttribute(name, formatted);
    return;
  }
  if (target.startsWith('style-var:')) {
    const name = target.slice(10);
    if (!IDENT_RE.test(name)) return;
    (el as HTMLElement).style.setProperty('--' + name, formatted);
    return;
  }
}

function applyThresholds(
  el: Element,
  raw: number,
  warn: number | null,
  crit: number | null,
): void {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) return;
  const critOn = crit !== null && raw >= crit;
  const warnOn = !critOn && warn !== null && raw >= warn;
  el.classList.toggle('sensor-critical', critOn);
  el.classList.toggle('sensor-warn', warnOn);
}

/**
 * Walk every `[data-sensor]` span under `container` and update its target
 * (textContent / attribute / style-var / class) with the formatted value
 * from `values`, plus apply threshold classes. Equivalent to bootstrap.js's
 * `__omni_update(values)` running inside Ultralight.
 *
 * Sensor paths absent from `values` are skipped (not zeroed) so a partial
 * snapshot doesn't blank out elements that just weren't updated this tick.
 */
export function applyPreviewValues(container: HTMLElement, values: PreviewValues): void {
  if (!values) return;
  const sensed = container.querySelectorAll('[data-sensor]');
  for (const el of Array.from(sensed)) {
    const path = el.getAttribute('data-sensor');
    if (!path) continue;
    const raw = values[path];
    if (raw === undefined || raw === null) continue;
    const target = el.getAttribute('data-sensor-target') || 'text';
    const format = el.getAttribute('data-sensor-format') || 'raw';
    const precision = parsePrecision(el.getAttribute('data-sensor-precision'));
    const warn = parseThreshold(el.getAttribute('data-sensor-threshold-warn'));
    const crit = parseThreshold(el.getAttribute('data-sensor-threshold-critical'));
    const formatted = formatValue(raw, format, precision);
    applyTarget(el, target, formatted);
    applyThresholds(el, raw, warn, crit);
  }
}
