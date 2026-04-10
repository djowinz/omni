// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { applyPreviewDiff } from '../preview-updater';

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
    expect(container.querySelector('[data-omni-id="omni-0"]')!.className).toBe('new active');
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
    expect(el.className).toBe('hot warning');
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
