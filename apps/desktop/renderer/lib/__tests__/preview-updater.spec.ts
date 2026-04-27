// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { applyPreviewDiff, applyPreviewValues } from '../preview-updater';

function createContainer(html: string): HTMLElement {
  const div = document.createElement('div');
  div.innerHTML = html;
  return div;
}

describe('applyPreviewDiff', () => {
  it('updates className on element by omni-id', () => {
    const container = createContainer('<div data-omni-id="omni-0" class="old">text</div>');
    applyPreviewDiff(container, {
      'omni-0': { c: 'new active' },
    });
    expect(container.querySelector('[data-omni-id="omni-0"]')!.getAttribute('class')).toBe(
      'new active',
    );
  });

  it('updates textContent on element by omni-id', () => {
    const container = createContainer('<span data-omni-id="omni-3">old text</span>');
    applyPreviewDiff(container, {
      'omni-3': { t: '72°C' },
    });
    expect(container.querySelector('[data-omni-id="omni-3"]')!.textContent).toBe('72°C');
  });

  it('updates both className and textContent', () => {
    const container = createContainer('<div data-omni-id="omni-1" class="cool">50%</div>');
    applyPreviewDiff(container, {
      'omni-1': { c: 'hot warning', t: '95%' },
    });
    const el = container.querySelector('[data-omni-id="omni-1"]')!;
    expect(el.getAttribute('class')).toBe('hot warning');
    expect(el.textContent).toBe('95%');
  });

  it('skips missing elements without error', () => {
    const container = createContainer('<div data-omni-id="omni-0">ok</div>');
    expect(() => {
      applyPreviewDiff(container, {
        'omni-99': { t: 'ghost' },
      });
    }).not.toThrow();
    expect(container.querySelector('[data-omni-id="omni-0"]')!.textContent).toBe('ok');
  });

  it('handles empty diff', () => {
    const container = createContainer('<div data-omni-id="omni-0">ok</div>');
    applyPreviewDiff(container, {});
    expect(container.querySelector('[data-omni-id="omni-0"]')!.textContent).toBe('ok');
  });

  it('preserves child elements when updating text', () => {
    const container = createContainer(
      '<div data-omni-id="omni-0">CPU: <span data-omni-id="omni-1">0%</span></div>',
    );
    applyPreviewDiff(container, {
      'omni-0': { t: 'GPU: ' },
    });
    const el = container.querySelector('[data-omni-id="omni-0"]')!;
    // Text node updated but child span preserved
    expect(el.querySelector('[data-omni-id="omni-1"]')).not.toBeNull();
    expect(el.childNodes[0].textContent).toBe('GPU: ');
  });

  it('applies attribute updates via setAttribute', () => {
    const container = createContainer('<svg><polyline data-omni-id="omni-0" points="0,0"/></svg>');
    applyPreviewDiff(container, {
      'omni-0': { a: { points: '0,50 10,40 20,30' } },
    });
    expect(container.querySelector('[data-omni-id="omni-0"]')!.getAttribute('points')).toBe(
      '0,50 10,40 20,30',
    );
  });

  it('applies multiple attributes in one update', () => {
    const container = createContainer(
      '<svg><rect data-omni-id="omni-0" x="0" y="0" width="10" height="10"/></svg>',
    );
    applyPreviewDiff(container, {
      'omni-0': { a: { height: '42', y: '8' } },
    });
    const el = container.querySelector('[data-omni-id="omni-0"]')!;
    expect(el.getAttribute('height')).toBe('42');
    expect(el.getAttribute('y')).toBe('8');
  });

  it('combines className, textContent, and attributes in one update', () => {
    const container = createContainer(
      '<svg><circle data-omni-id="omni-0" r="10" class="old">label</circle></svg>',
    );
    applyPreviewDiff(container, {
      'omni-0': {
        c: 'hot',
        t: 'label2',
        a: { r: '20' },
      },
    });
    const el = container.querySelector('[data-omni-id="omni-0"]')!;
    expect(el.getAttribute('class')).toBe('hot');
    expect(el.textContent).toBe('label2');
    expect(el.getAttribute('r')).toBe('20');
  });

  it('updates multiple elements in one diff', () => {
    const container = createContainer(
      '<div data-omni-id="omni-0">a</div><div data-omni-id="omni-1">b</div>',
    );
    applyPreviewDiff(container, {
      'omni-0': { t: 'x' },
      'omni-1': { t: 'y' },
    });
    expect(container.querySelector('[data-omni-id="omni-0"]')!.textContent).toBe('x');
    expect(container.querySelector('[data-omni-id="omni-1"]')!.textContent).toBe('y');
  });
});

// ── applyPreviewValues — pins the editor-preview-stale-sensors fix ──────────
//
// Bug: the iframe's data-sensor spans never updated because the IPC bridge
// stripped `values` from the preview.update wire payload and the renderer
// only applied `diff`. Result: live-stats panel showed CPU 9%, iframe
// preview showed CPU 27% (stuck at the value baked into the initial
// preview.html). These tests pin the format/precision/threshold logic so it
// stays bytewise-equivalent to crates/host/src/omni/bootstrap.js.

describe('applyPreviewValues — sensor span updates', () => {
  it('formats percent with default precision (0) and updates textContent', () => {
    const container = createContainer(
      '<span data-sensor="cpu.usage" data-sensor-format="percent">old</span>',
    );
    applyPreviewValues(container, { 'cpu.usage': 9.42 });
    expect(container.querySelector('[data-sensor="cpu.usage"]')!.textContent).toBe('9%');
  });

  it('scales 0..1 floats to 0..100 in percent format (matches bootstrap)', () => {
    const container = createContainer(
      '<span data-sensor="ram.percent" data-sensor-format="percent" data-sensor-precision="1">old</span>',
    );
    applyPreviewValues(container, { 'ram.percent': 0.44 });
    expect(container.querySelector('[data-sensor="ram.percent"]')!.textContent).toBe('44.0%');
  });

  it('passes through values >1 in percent format without rescaling', () => {
    const container = createContainer(
      '<span data-sensor="ram.percent" data-sensor-format="percent">old</span>',
    );
    applyPreviewValues(container, { 'ram.percent': 44 });
    expect(container.querySelector('[data-sensor="ram.percent"]')!.textContent).toBe('44%');
  });

  it('formats temperature with degree symbol', () => {
    const container = createContainer(
      '<span data-sensor="cpu.temp" data-sensor-format="temperature" data-sensor-precision="0">--</span>',
    );
    applyPreviewValues(container, { 'cpu.temp': 67.4 });
    expect(container.querySelector('[data-sensor="cpu.temp"]')!.textContent).toBe('67°C');
  });

  it('formats bytes with auto unit selection', () => {
    const container = createContainer(
      '<span data-sensor="ram.used" data-sensor-format="bytes" data-sensor-precision="2">--</span>',
    );
    applyPreviewValues(container, { 'ram.used': 1024 * 1024 * 1024 * 8 + 1024 * 1024 * 100 });
    // ~8.10 GB
    expect(container.querySelector('[data-sensor="ram.used"]')!.textContent).toBe('8.10 GB');
  });

  it('toggles sensor-warn class when value crosses warn threshold', () => {
    const container = createContainer(
      '<span data-sensor="cpu.usage" data-sensor-format="percent" data-sensor-threshold-warn="80" data-sensor-threshold-critical="95">--</span>',
    );
    applyPreviewValues(container, { 'cpu.usage': 85 });
    const el = container.querySelector('[data-sensor="cpu.usage"]')!;
    expect(el.classList.contains('sensor-warn')).toBe(true);
    expect(el.classList.contains('sensor-critical')).toBe(false);
  });

  it('toggles sensor-critical (and clears sensor-warn) above critical threshold', () => {
    const container = createContainer(
      '<span data-sensor="cpu.usage" data-sensor-format="percent" data-sensor-threshold-warn="80" data-sensor-threshold-critical="95" class="sensor-warn">--</span>',
    );
    applyPreviewValues(container, { 'cpu.usage': 97 });
    const el = container.querySelector('[data-sensor="cpu.usage"]')!;
    expect(el.classList.contains('sensor-critical')).toBe(true);
    expect(el.classList.contains('sensor-warn')).toBe(false);
  });

  it('skips elements whose sensor path is absent from values (does not blank them)', () => {
    const container = createContainer(
      '<span data-sensor="cpu.usage" data-sensor-format="percent">prev value</span>',
    );
    applyPreviewValues(container, { 'gpu.usage': 50 });
    // cpu.usage wasn't in the values map → element untouched.
    expect(container.querySelector('[data-sensor="cpu.usage"]')!.textContent).toBe('prev value');
  });

  it('updates multiple sensor spans bound to the same path', () => {
    const container = createContainer(
      '<span data-sensor="cpu.usage" data-sensor-format="percent">a</span>' +
        '<span data-sensor="cpu.usage" data-sensor-format="percent" data-sensor-precision="2">b</span>',
    );
    applyPreviewValues(container, { 'cpu.usage': 42.5 });
    const spans = container.querySelectorAll('[data-sensor="cpu.usage"]');
    expect(spans[0].textContent).toBe('43%'); // precision 0 (default)
    expect(spans[1].textContent).toBe('42.50%'); // precision 2
  });

  it('writes to attribute when target=attr:<name>', () => {
    const container = createContainer(
      '<polyline data-sensor="cpu.usage" data-sensor-target="attr:stroke-width" data-sensor-format="raw">--</polyline>',
    );
    applyPreviewValues(container, { 'cpu.usage': 3 });
    expect(container.querySelector('[data-sensor="cpu.usage"]')!.getAttribute('stroke-width')).toBe(
      '3',
    );
  });

  it('writes to CSS variable when target=style-var:<name>', () => {
    const container = createContainer(
      '<div data-sensor="cpu.usage" data-sensor-target="style-var:fill" data-sensor-format="raw">--</div>',
    );
    applyPreviewValues(container, { 'cpu.usage': 7 });
    const el = container.querySelector('[data-sensor="cpu.usage"]') as HTMLElement;
    expect(el.style.getPropertyValue('--fill')).toBe('7');
  });
});
