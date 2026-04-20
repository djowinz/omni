/**
 * debugLog — env-gated breadcrumb logger for the renderer.
 *
 * Enabled when EITHER:
 *   - build-time flag `NEXT_PUBLIC_OMNI_DEBUG=1` (Next.js public env var)
 *   - runtime toggle `localStorage.setItem('OMNI_DEBUG', '1')`
 *
 * When disabled, all calls are no-ops so zero breadcrumb noise reaches
 * the devtools console in packaged builds. See OWI-5.
 */

function envFlag(): boolean {
  try {
    return typeof process !== 'undefined' && process.env?.NEXT_PUBLIC_OMNI_DEBUG === '1';
  } catch {
    return false;
  }
}

function storageFlag(): boolean {
  try {
    return typeof window !== 'undefined' && window.localStorage?.getItem('OMNI_DEBUG') === '1';
  } catch {
    return false;
  }
}

const enabled = envFlag() || storageFlag();

export const debugLog: (...args: unknown[]) => void = enabled
  ? (...args) => console.log(...args)
  : () => {};
