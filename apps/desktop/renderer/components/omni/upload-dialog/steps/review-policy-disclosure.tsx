/**
 * ReviewPolicyDisclosure — Step 2 footer block reminding the publisher of
 * Omni's content policy. Renders inline copy + a single text link to the
 * full policy doc. Spec INV-7.2.7.
 *
 * Link behaviour:
 *   - Prefers `window.omni.openExternal(url)` so Electron routes the URL
 *     through `shell.openExternal` and opens it in the user's default
 *     browser instead of inside the Electron `BrowserWindow`.
 *   - Falls back to the plain `target="_blank"` anchor when the bridge is
 *     absent (current state — see TODO below; also keeps the component
 *     useful inside `next dev` running in a non-Electron browser tab).
 *
 * TODO(OWI): the preload bridge does NOT currently expose `openExternal`.
 * Add `openExternal: (url: string) => ipcRenderer.send('open-external', url)`
 * to `apps/desktop/main/preload.ts`, register the matching `ipcMain.on`
 * handler that calls `shell.openExternal(url)` (with an allow-list
 * restricted to https:// origins), and extend `OmniIpcBridge` in
 * `apps/desktop/renderer/types/electron.d.ts`. Until that ships, the
 * `target="_blank"` fallback runs and the link opens in a new in-renderer
 * window — acceptable for a placeholder URL, but flagged for follow-up.
 */

/**
 * Sentinel policy URL.
 *
 * The real Omni content-policy doc location has not been chosen by
 * product/legal yet. We keep an `omni.example` sentinel so:
 *   - tests asserting `href` matches `^https?://` still pass
 *   - clicking the link doesn't accidentally take a user to a third-party
 *     site that we'd then have to reclaim (e.g. github.com/<org>/<repo>
 *     where the org doesn't exist yet)
 *   - the eventual swap is a single-string edit gated on a tiny PR
 *
 * Replace this with the canonical URL once product/legal sign off on the
 * final hosting location (likely `https://omni.app/policy` or a bundled
 * Electron route — both are live options).
 */
export const POLICY_URL = 'https://omni.example/policy';

export interface ReviewPolicyDisclosureProps {
  /** Optional override — primarily for tests. Defaults to POLICY_URL. */
  policyUrl?: string;
}

export function ReviewPolicyDisclosure({ policyUrl = POLICY_URL }: ReviewPolicyDisclosureProps) {
  const handleClick = (event: React.MouseEvent<HTMLAnchorElement>) => {
    // Route through the Electron preload bridge when available so the URL
    // opens in the user's default browser (shell.openExternal) instead of
    // navigating the Electron BrowserWindow.
    //
    // The structural cast keeps this file self-contained — `OmniIpcBridge`
    // in `types/electron.d.ts` doesn't declare `openExternal` yet (see TODO
    // above). When the preload + main-process plumbing lands, drop the cast
    // and consume `window.omni.openExternal` directly.
    const bridge = typeof window !== 'undefined' ? window.omni : undefined;
    const openExternal = (bridge as { openExternal?: (url: string) => unknown } | undefined)
      ?.openExternal;
    if (typeof openExternal === 'function') {
      event.preventDefault();
      void openExternal(policyUrl);
    }
    // Otherwise fall through to the anchor's default behaviour
    // (target="_blank") — see TODO at the top of this file.
  };

  return (
    <div data-testid="review-policy-disclosure" className="text-xs leading-relaxed text-zinc-400">
      By publishing, you confirm this content follows Omni&apos;s content policy (no illegal
      content, no harassment, no explicit material).{' '}
      <a
        data-testid="review-policy-disclosure-link"
        href={policyUrl}
        onClick={handleClick}
        target="_blank"
        rel="noreferrer"
        className="text-[#00D9FF] hover:underline"
      >
        Read the full policy
      </a>
    </div>
  );
}
