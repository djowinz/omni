export interface PreviewDiff {
  [omniId: string]: { c?: string; t?: string };
}

export function applyPreviewDiff(container: HTMLElement, diff: PreviewDiff): void {
  for (const [id, update] of Object.entries(diff)) {
    const el = container.querySelector(`[data-omni-id="${id}"]`);
    if (!el) continue;
    if (update.c !== undefined) {
      el.className = update.c;
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
  }
}
