# Preview Pipeline Unification

## Problem

The Electron editor preview and the game overlay render `.omni` files through two completely separate implementations:

- **Editor preview**: TypeScript builds HTML client-side via `buildPreviewStructure()`, updates DOM in-place via `updatePreviewDOM()` with client-side condition evaluation and metric formatting.
- **Game overlay**: Rust host builds HTML via `build_initial_html()`, sends per-frame JS updates via `build_update_js()` to Ultralight, which renders to a BGRA bitmap.

This dual-path architecture means every rendering feature must be implemented twice (TypeScript + Rust), and the preview may not match what the overlay actually renders. It also blocks features like chart/graph rendering that require host-side state (time-series buffers) — the preview has no way to show host-generated content.

## Decision Log

| Question | Decision | Rationale |
|----------|----------|-----------|
| How should host deliver content to preview? | Initial HTML + structured JSON diffs over WebSocket | Reuses existing `build_update_js` infrastructure, keeps preview DOM-based, preserves CSS transitions |
| What happens when host is disconnected? | Preview goes blank | Host cannot be running and not reporting stats. No fallback rendering path. |
| How are per-frame updates delivered? | Structured JSON payload (not raw JS string) | Separates data from serialization. No eval in renderer. Same data drives both Ultralight JS and preview JSON. |
| Single spec or split (pipeline + charts)? | Two specs, sequential | Pipeline unification is a standalone prerequisite. Charts come after. |
| Do structural edits go through the host? | Yes, all HTML comes from host | Single source of truth. Localhost WebSocket round-trip is fast enough for 400ms debounced edits. |

## Architecture

### Render Core Refactor

Split `build_update_js()` into two stages:

1. **`compute_update_diff()`** — Pure function. Takes the current widget tree + sensor snapshot, returns `HashMap<String, ElementUpdate>` where each entry maps an `omni-id` to its new class string and/or text content. No serialization.

2. **Two serializers consume the diff:**
   - `format_as_js(diff)` — Wraps as `omniUpdate({...})` JS string for Ultralight (existing behavior).
   - Raw diff serializes as JSON for WebSocket preview subscribers.

`build_initial_html()` is refactored to return a struct with separated fields: `html` (widget markup with `data-omni-id` attributes, no wrapping `<html>`/`<body>`), `css` (combined widget styles + theme CSS), and `full_document` (the complete HTML document with embedded styles for Ultralight). Ultralight consumes `full_document`. The `preview.html` WebSocket message sends `html` and `css` separately so the Electron preview can inject them independently into the DOM.

### Host Render Loop

Current:
```
loop {
    poll sensors
    if overlay changed → build_initial_html() → load into Ultralight
    build_update_js() → evaluate in Ultralight
    render → extract pixels → write to shared memory
}
```

Unified:
```
loop {
    poll sensors
    if overlay changed → build_initial_html() → load into Ultralight
                       → send preview.html to WebSocket subscribers
    compute_update_diff() → structured diff
        → format_as_js() → evaluate in Ultralight
        → serialize as JSON → send preview.update to subscribers
    render → extract pixels → write to shared memory
}
```

- Diff computed once per tick, serialized twice (JS for Ultralight, JSON for WebSocket).
- If no clients subscribed, JSON serialization is skipped entirely.
- `preview.update` sends at render loop cadence — true WYSIWYG.
- `WsSharedState` gets `preview_subscribers: Vec<SenderHandle>` to track subscribed connections.

### WebSocket Protocol

Three new message types:

**`preview.subscribe`** (client → host)
```json
{ "type": "preview.subscribe" }
```
Subscribes the client to preview updates. Host responds with `preview.subscribed` and immediately sends `preview.html` if an overlay is active.

**`preview.html`** (host → client)
```json
{
  "type": "preview.html",
  "html": "<div data-omni-id='omni-0'>...</div>",
  "css": "/* combined widget + theme styles */"
}
```
Sent on: initial subscribe (if overlay active), after `widget.apply`, and on overlay switch. This is the full structural HTML — preview injects it into the DOM once. CSS animations restart on structural changes (expected, since the user just changed markup).

**`preview.update`** (host → client)
```json
{
  "type": "preview.update",
  "diff": {
    "omni-0": { "c": "active cold", "t": "42%" },
    "omni-5": { "c": "hot" }
  }
}
```
Sent every render tick. `c` = className, `t` = textContent. Only changed elements included. DOM nodes stay alive — CSS transitions preserved.

### Subscriber Management

- Client sends `preview.subscribe` → added to subscriber list.
- Client disconnects → removed from subscriber list.
- `widget.apply` triggers `build_initial_html` + pushes `preview.html` to all subscribers.

## Frontend Changes

### Files Removed or Gutted

**`renderer/lib/omni-parser.ts`** — Remove `buildPreviewStructure()`, `updatePreviewDOM()`, `evaluateCondition()`, `formatMetricValue()`. Keep `parseOmniContent()` (still needed for widget panel to extract widget metadata for sidebar UI).

**`renderer/lib/sensor-mapping.ts`** — Remove entirely. Preview no longer processes raw sensor data.

**`renderer/hooks/use-sensor-data.ts`** — Remove entirely. No client-side sensor consumption for preview.

**MetricSimulator component** — Remove entirely. No simulate mode.

### Files Modified

**`renderer/components/omni/preview-panel.tsx`:**
- Remove all `buildPreviewStructure` / `updatePreviewDOM` logic.
- Remove live vs. simulate mode switching.
- On connected + mount: call `backend.subscribePreview()`.
- Listen for `onPreviewHtml` → set container `innerHTML` (structural reload).
- Listen for `onPreviewUpdate` → call `applyPreviewDiff(container, diff)` (incremental).
- On disconnected: render blank.
- Cleanup: unsubscribe on unmount.

**`renderer/lib/backend-api.ts`:**
- Add `subscribePreview()` → sends `{ type: 'preview.subscribe' }`.
- Add response type mapping: `preview.subscribe` → `preview.subscribed`.

**`main/host-manager.ts`:**
- Forward `preview.html` and `preview.update` messages to renderer via IPC (same pattern as existing `sensors.data` forwarding).

**`main/preload.ts`:**
- Add `window.omni.onPreviewHtml(callback)` — IPC listener for `preview-html` channel.
- Add `window.omni.onPreviewUpdate(callback)` — IPC listener for `preview-update` channel.

### New File

**`renderer/lib/preview-updater.ts`:**
- `applyPreviewDiff(container: HTMLElement, diff: Record<string, { c?: string; t?: string }>)` — Iterates diff entries, finds elements by `[data-omni-id="omni-N"]`, sets `className` and `textContent`. ~20 lines.

### State Management (`use-omni-state.tsx` / `app-reducer.ts`)

- Remove `previewMetrics` from `AppState`.
- Remove `UPDATE_PREVIEW_METRICS` action.
- Remove simulate mode related state/actions.

### Widget Panel (`widget-panel.tsx`)

- Unchanged. Still reads widget metadata from `parseOmniContent()` for sidebar toggle list. Widget enable/disable sends updated source via `widget.apply`.

## Error Handling & Edge Cases

**Host disconnects mid-session:**
Preview goes blank immediately. On reconnect, Electron re-sends `preview.subscribe`. Host responds with current `preview.html` and resumes `preview.update` streaming.

**Syntax errors in `.omni` edits:**
`widget.apply` returns diagnostics as before — Monaco shows error markers. If parse fails completely, host does not push `preview.html` — preview keeps showing last valid state. If parse succeeds with warnings, host applies and pushes updated `preview.html`.

**Multiple Electron windows (future):**
Each window sends its own `preview.subscribe`. Host tracks multiple subscribers and broadcasts to all.

**Overlay switches:**
Host rebuilds HTML for new overlay, pushes `preview.html` to subscribers. Same as structural edit.

**No overlay active:**
`preview.subscribed` response includes: `{ "type": "preview.subscribed", "active": false }`. Preview shows blank. When overlay is later activated, host pushes `preview.html`.

**Rapid edits:**
Electron debounces `widget.apply` at 400ms. Host processes sequentially. Each successful apply triggers `preview.html` push. CSS animations restart on structural changes — acceptable since user is actively editing.

## Out of Scope

- Chart/graph rendering (separate spec, depends on this work).
- Simulate mode replacement (removed, not replaced).
- Adaptive color / luminance sampling.
- Background images.
