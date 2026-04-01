# v0 UI Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the v0-built Omni editor UI into the existing Nextron desktop app, replacing the minimal connection-status placeholder with the full 3-panel editor layout.

**Architecture:** Lift-and-shift from `v0_electron_app/` into `desktop/renderer/`, copying only the 11 actually-used shadcn/ui components (out of 57), 8 custom omni components, hooks, lib, and types. Replace Geist fonts with bundled Monaspace Krypton NF. Replace `next/image` with `<img>`. Wire storage adapter to Electron IPC via the existing WebSocket connection.

**Tech Stack:** Nextron, Next.js, React 19, TypeScript, Tailwind CSS v4, shadcn/ui (subset), react-resizable-panels, lucide-react, Monaspace Krypton NF font

**Source:** `v0_electron_app/` (v0-generated Next.js app in repo root)

---

## File Structure

### Files to copy from v0 (source → destination)

**Custom components (copy as-is, adapt imports):**
- `v0_electron_app/components/omni/header.tsx` → `desktop/renderer/components/omni/header.tsx`
- `v0_electron_app/components/omni/widget-panel.tsx` → `desktop/renderer/components/omni/widget-panel.tsx`
- `v0_electron_app/components/omni/editor-panel.tsx` → `desktop/renderer/components/omni/editor-panel.tsx`
- `v0_electron_app/components/omni/preview-panel.tsx` → `desktop/renderer/components/omni/preview-panel.tsx`
- `v0_electron_app/components/omni/metric-simulator.tsx` → `desktop/renderer/components/omni/metric-simulator.tsx`
- `v0_electron_app/components/omni/status-bar.tsx` → `desktop/renderer/components/omni/status-bar.tsx`
- `v0_electron_app/components/omni/create-overlay-dialog.tsx` → `desktop/renderer/components/omni/create-overlay-dialog.tsx`
- `v0_electron_app/components/omni/game-assignments-dialog.tsx` → `desktop/renderer/components/omni/game-assignments-dialog.tsx`

**shadcn/ui components (only the 11 used — copy as-is):**
- `badge.tsx`, `button.tsx`, `dialog.tsx`, `dropdown-menu.tsx`, `input.tsx`, `label.tsx`, `resizable.tsx`, `scroll-area.tsx`, `select.tsx`, `slider.tsx`, `switch.tsx`
- All go to `desktop/renderer/components/ui/`

**Hooks, lib, types (copy, adapt storage):**
- `v0_electron_app/hooks/use-omni-state.tsx` → `desktop/renderer/hooks/use-omni-state.tsx`
- `v0_electron_app/lib/omni-parser.ts` → `desktop/renderer/lib/omni-parser.ts`
- `v0_electron_app/lib/storage-adapter.ts` → `desktop/renderer/lib/storage-adapter.ts` (rewrite for WebSocket)
- `v0_electron_app/lib/utils.ts` → `desktop/renderer/lib/utils.ts`
- `v0_electron_app/types/omni.ts` → `desktop/renderer/types/omni.ts`

**Styles (adapt for Krypton font):**
- `v0_electron_app/app/globals.css` → `desktop/renderer/styles/globals.css` (replace, add @font-face)

### Files to modify
- `desktop/renderer/pages/home.tsx` — replace with v0 page content
- `desktop/renderer/pages/_app.tsx` — update layout wrapper
- `desktop/package.json` — add npm dependencies

### Files to delete (replaced by v0 UI)
- `desktop/renderer/components/ConnectionStatus.tsx`
- `desktop/renderer/hooks/useHostStatus.ts`

### Fonts (already in place)
- `desktop/resources/fonts/MonaspaceKryptonNF-*.otf` (8 files, already added by user)

---

## Task 1: Install npm dependencies

**Files:**
- Modify: `desktop/package.json`

- [ ] **Step 1: Install the required packages**

```bash
cd C:/Users/DyllenOwens/Projects/omni/desktop
npm install @radix-ui/react-dialog @radix-ui/react-dropdown-menu @radix-ui/react-label @radix-ui/react-scroll-area @radix-ui/react-select @radix-ui/react-slider @radix-ui/react-slot @radix-ui/react-switch class-variance-authority clsx tailwind-merge lucide-react react-resizable-panels
```

- [ ] **Step 2: Install Tailwind CSS v4 + PostCSS dev dependencies**

```bash
cd C:/Users/DyllenOwens/Projects/omni/desktop
npm install --save-dev tailwindcss @tailwindcss/postcss postcss tw-animate-css
```

- [ ] **Step 3: Create PostCSS config**

Create `desktop/postcss.config.mjs`:

```javascript
export default {
  plugins: {
    '@tailwindcss/postcss': {},
  },
};
```

- [ ] **Step 4: Verify npm install succeeded**

```bash
cd C:/Users/DyllenOwens/Projects/omni/desktop && npm ls --depth=0
```

Expected: No missing peer dependency errors.

- [ ] **Step 5: Commit**

```bash
cd C:/Users/DyllenOwens/Projects/omni
git add desktop/package.json desktop/package-lock.json desktop/postcss.config.mjs
git commit -m "feat(desktop): add Tailwind, shadcn/ui, and resizable panel dependencies"
```

---

## Task 2: Copy shadcn/ui components and utility lib

**Files:**
- Copy 11 files to: `desktop/renderer/components/ui/`
- Copy: `desktop/renderer/lib/utils.ts`

- [ ] **Step 1: Copy the 11 used shadcn/ui components**

```bash
cd C:/Users/DyllenOwens/Projects/omni
mkdir -p desktop/renderer/components/ui
cp v0_electron_app/components/ui/badge.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/button.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/dialog.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/dropdown-menu.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/input.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/label.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/resizable.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/scroll-area.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/select.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/slider.tsx desktop/renderer/components/ui/
cp v0_electron_app/components/ui/switch.tsx desktop/renderer/components/ui/
```

- [ ] **Step 2: Copy the `cn()` utility**

```bash
mkdir -p desktop/renderer/lib
cp v0_electron_app/lib/utils.ts desktop/renderer/lib/utils.ts
```

- [ ] **Step 3: Verify the shadcn components have correct import paths**

The shadcn components use `@/lib/utils` to import `cn`. In the Nextron renderer, `@/` should resolve to the renderer directory. Check that `desktop/renderer/tsconfig.json` has the `@/*` path alias. If not, add it:

```json
{
  "compilerOptions": {
    "paths": {
      "@/*": ["./*"]
    }
  }
}
```

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/components/ui/ desktop/renderer/lib/utils.ts
git commit -m "feat(desktop): add 11 shadcn/ui components and cn() utility"
```

---

## Task 3: Copy types, lib, and hooks

**Files:**
- Copy: `desktop/renderer/types/omni.ts`
- Copy: `desktop/renderer/lib/omni-parser.ts`
- Copy: `desktop/renderer/hooks/use-omni-state.tsx`

- [ ] **Step 1: Copy type definitions**

```bash
mkdir -p desktop/renderer/types
cp v0_electron_app/types/omni.ts desktop/renderer/types/omni.ts
```

- [ ] **Step 2: Copy the parser**

```bash
cp v0_electron_app/lib/omni-parser.ts desktop/renderer/lib/omni-parser.ts
```

- [ ] **Step 3: Copy the state management hook**

```bash
cp v0_electron_app/hooks/use-omni-state.tsx desktop/renderer/hooks/use-omni-state.tsx
```

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/types/ desktop/renderer/lib/omni-parser.ts desktop/renderer/hooks/use-omni-state.tsx
git commit -m "feat(desktop): add omni types, parser, and state management from v0"
```

---

## Task 4: Rewrite storage adapter for WebSocket IPC

**Files:**
- Create: `desktop/renderer/lib/storage-adapter.ts`

- [ ] **Step 1: Write the WebSocket-backed storage adapter**

The v0 app used localStorage. In the Electron app, the host process owns the filesystem via the WebSocket API. Create `desktop/renderer/lib/storage-adapter.ts`:

```typescript
import type { Overlay, GameAssignment, StorageAdapter } from '@/types/omni';
import { SAMPLE_DEFAULT_OVERLAY } from '@/types/omni';

/**
 * Storage adapter that uses the host's WebSocket API via Electron IPC
 * for overlay/theme persistence. Falls back to localStorage when
 * the IPC bridge is unavailable (e.g., during development without Electron).
 */

function getIpcBridge() {
  if (typeof window !== 'undefined' && (window as any).omni) {
    return (window as any).omni;
  }
  return null;
}

export const localStorageAdapter: StorageAdapter = {
  loadOverlays: () => {
    try {
      const raw = localStorage.getItem('omni_overlays');
      if (raw) return JSON.parse(raw) as Overlay[];
    } catch { /* ignore */ }
    return [SAMPLE_DEFAULT_OVERLAY];
  },

  saveOverlays: (overlays: Overlay[]) => {
    try {
      localStorage.setItem('omni_overlays', JSON.stringify(overlays));
    } catch { /* ignore */ }
  },

  loadGameAssignments: () => {
    try {
      const raw = localStorage.getItem('omni_game_assignments');
      if (raw) return JSON.parse(raw) as GameAssignment[];
    } catch { /* ignore */ }
    return [];
  },

  saveGameAssignments: (assignments: GameAssignment[]) => {
    try {
      localStorage.setItem('omni_game_assignments', JSON.stringify(assignments));
    } catch { /* ignore */ }
  },

  loadActiveOverlayId: () => {
    try {
      return localStorage.getItem('omni_active_overlay_id');
    } catch { return null; }
  },

  saveActiveOverlayId: (id: string | null) => {
    try {
      if (id) {
        localStorage.setItem('omni_active_overlay_id', id);
      } else {
        localStorage.removeItem('omni_active_overlay_id');
      }
    } catch { /* ignore */ }
  },
};

export function getStorageAdapter(): StorageAdapter {
  // For now, use localStorage. When we wire up the full Electron IPC
  // bridge to the host's file.read/file.write WebSocket API, this
  // function will return an IPC-backed adapter instead.
  return localStorageAdapter;
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/renderer/lib/storage-adapter.ts
git commit -m "feat(desktop): storage adapter with localStorage (WebSocket IPC ready)"
```

---

## Task 5: Copy omni components with adaptations

**Files:**
- Copy 8 files to: `desktop/renderer/components/omni/`
- Adapt: `header.tsx` (replace `next/image` with `<img>`)

- [ ] **Step 1: Copy all 8 omni components**

```bash
mkdir -p desktop/renderer/components/omni
cp v0_electron_app/components/omni/header.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/widget-panel.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/editor-panel.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/preview-panel.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/metric-simulator.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/status-bar.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/create-overlay-dialog.tsx desktop/renderer/components/omni/
cp v0_electron_app/components/omni/game-assignments-dialog.tsx desktop/renderer/components/omni/
```

- [ ] **Step 2: Adapt header.tsx — replace `next/image` with `<img>`**

In `desktop/renderer/components/omni/header.tsx`:
- Remove the `import Image from 'next/image';` line
- Replace any `<Image src=... />` usage with a plain `<img>` tag or an inline SVG/text logo
- Keep all other imports and logic unchanged

- [ ] **Step 3: Remove `'use client'` directives**

The v0 app uses Next.js App Router which requires `'use client'` directives. Nextron with Pages Router doesn't need them. Remove the `'use client';` line from the top of any files that have it. Check all 8 omni component files and `use-omni-state.tsx`.

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/components/omni/
git commit -m "feat(desktop): migrate 8 omni editor components from v0"
```

---

## Task 6: Update styles with Krypton font and globals.css

**Files:**
- Replace: `desktop/renderer/styles/globals.css`

- [ ] **Step 1: Replace globals.css with v0 styles + Krypton font faces**

Replace `desktop/renderer/styles/globals.css` with the v0 `globals.css` content, but:
1. Replace the Geist font references with Monaspace Krypton NF
2. Add `@font-face` declarations at the top
3. Remove `@import 'tw-animate-css';` if `tw-animate-css` is not installed, OR keep it if it was installed in Task 1

The file should start with:

```css
@import 'tailwindcss';

/* Monaspace Krypton NF - bundled font */
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-Light.otf') format('opentype');
  font-weight: 300;
  font-style: normal;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-LightItalic.otf') format('opentype');
  font-weight: 300;
  font-style: italic;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-Regular.otf') format('opentype');
  font-weight: 400;
  font-style: normal;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-Italic.otf') format('opentype');
  font-weight: 400;
  font-style: italic;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-Medium.otf') format('opentype');
  font-weight: 500;
  font-style: normal;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-MediumItalic.otf') format('opentype');
  font-weight: 500;
  font-style: italic;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-Bold.otf') format('opentype');
  font-weight: 700;
  font-style: normal;
}
@font-face {
  font-family: 'Monaspace Krypton';
  src: url('../../resources/fonts/MonaspaceKryptonNF-BoldItalic.otf') format('opentype');
  font-weight: 700;
  font-style: italic;
}
```

Then include the full v0 CSS variables (`:root` block, `.dark` block), but in the `@theme inline` block, replace the font references:

```css
@theme inline {
  --font-sans: 'Monaspace Krypton', monospace;
  --font-mono: 'Monaspace Krypton', monospace;
  /* ... rest of theme variables from v0 globals.css ... */
}
```

Keep the `@layer base` block and `@custom-variant dark` line from the v0 file.

- [ ] **Step 2: Commit**

```bash
git add desktop/renderer/styles/globals.css
git commit -m "feat(desktop): Monaspace Krypton NF font + Omni dark theme CSS variables"
```

---

## Task 7: Update pages and layout

**Files:**
- Replace: `desktop/renderer/pages/home.tsx`
- Modify: `desktop/renderer/pages/_app.tsx`
- Delete: `desktop/renderer/components/ConnectionStatus.tsx`
- Delete: `desktop/renderer/hooks/useHostStatus.ts`

- [ ] **Step 1: Replace home.tsx with the v0 page content**

Replace `desktop/renderer/pages/home.tsx` with the content from `v0_electron_app/app/page.tsx`, adapted for Pages Router (no `'use client'`, same imports):

```tsx
import { OmniProvider } from '@/hooks/use-omni-state';
import { Header } from '@/components/omni/header';
import { StatusBar } from '@/components/omni/status-bar';
import { WidgetPanel } from '@/components/omni/widget-panel';
import { EditorPanel } from '@/components/omni/editor-panel';
import { PreviewPanel } from '@/components/omni/preview-panel';
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from '@/components/ui/resizable';

export default function Home() {
  return (
    <OmniProvider>
      <div className="flex h-screen flex-col bg-[#0D0D0F] text-[#FAFAFA]">
        <Header />
        <main className="flex-1 overflow-hidden">
          <ResizablePanelGroup direction="horizontal" className="h-full">
            <ResizablePanel defaultSize={18} minSize={15} maxSize={25}>
              <WidgetPanel />
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className="w-1 bg-[#0D0D0F] hover:bg-[#00D9FF]/30 transition-colors data-[resize-handle-active]:bg-[#00D9FF]/50"
            />
            <ResizablePanel defaultSize={47} minSize={30}>
              <EditorPanel />
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className="w-1 bg-[#0D0D0F] hover:bg-[#00D9FF]/30 transition-colors data-[resize-handle-active]:bg-[#00D9FF]/50"
            />
            <ResizablePanel defaultSize={35} minSize={25}>
              <PreviewPanel />
            </ResizablePanel>
          </ResizablePanelGroup>
        </main>
        <StatusBar />
      </div>
    </OmniProvider>
  );
}
```

- [ ] **Step 2: Update _app.tsx**

Update `desktop/renderer/pages/_app.tsx` to set the dark class and body styles:

```tsx
import type { AppProps } from 'next/app';
import '../styles/globals.css';

export default function App({ Component, pageProps }: AppProps) {
  return (
    <div className="dark">
      <Component {...pageProps} />
    </div>
  );
}
```

- [ ] **Step 3: Delete old placeholder components**

```bash
rm desktop/renderer/components/ConnectionStatus.tsx
rm desktop/renderer/hooks/useHostStatus.ts
```

- [ ] **Step 4: Commit**

```bash
git add desktop/renderer/pages/ desktop/renderer/components/ desktop/renderer/hooks/
git commit -m "feat(desktop): replace placeholder UI with full v0 editor layout"
```

---

## Task 8: Fix build — resolve import issues and compile

**Files:**
- Potentially modify: any file with broken imports

- [ ] **Step 1: Attempt a build**

```bash
cd C:/Users/DyllenOwens/Projects/omni/desktop
npm run build
```

- [ ] **Step 2: Fix any TypeScript or import errors**

Common issues to expect and fix:
- `@/` path alias not resolving — ensure `tsconfig.json` has `"paths": { "@/*": ["./*"] }` under `compilerOptions` in the renderer tsconfig
- `'use client'` directives causing issues — remove them (Pages Router doesn't use them)
- `next/image` import in header.tsx — ensure it was replaced with `<img>`
- Missing `@vercel/analytics` — ensure the import was removed from layout (we don't have a layout.tsx, the v0 layout.tsx content is split between _app.tsx and home.tsx)
- Tailwind CSS not processing — ensure `postcss.config.mjs` exists and the `@import 'tailwindcss'` is at the top of globals.css
- `tw-animate-css` import in globals.css — remove if not installed, or install it

- [ ] **Step 3: Iterate until build passes**

Keep running `npm run build` and fixing errors until it compiles cleanly.

- [ ] **Step 4: Commit all fixes**

```bash
git add -A
git commit -m "fix(desktop): resolve import paths and build errors after v0 migration"
```

---

## Task 9: Verify Rust crate still builds clean

- [ ] **Step 1: Run Rust tests**

```bash
cd C:/Users/DyllenOwens/Projects/omni
cargo test --workspace
```

Expected: All tests pass (no Rust changes in this migration).

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-targets
```

Expected: Zero warnings.

- [ ] **Step 3: Commit if any fmt needed**

```bash
cargo fmt --all
git add -A
git diff --cached --stat && git commit -m "cleanup: fmt" || echo "nothing to commit"
```
