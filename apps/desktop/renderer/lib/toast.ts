// Typed toast wrapper — the sole import site for `sonner` in the renderer.
// Phase 3 specs import from `@/lib/toast`, never from `sonner` directly, so
// the toast library can be swapped in one file. `error()` always routes
// through `mapErrorToUserMessage` — no branching on `code` in the wrapper.

import { toast as sonnerToast, type ExternalToast } from 'sonner';
import { mapErrorToUserMessage, type OmniError } from './map-error-to-user-message';

/**
 * Returns true if `e` already conforms to the OmniError shape (host-emitted
 * D-004-J error envelope). Defensive predicate so `toast.error` can accept
 * raw catch values from anywhere without crashing — the renderer has many
 * code paths that catch JS `Error` or unstructured throws (Next.js page
 * navigation, React error boundaries, third-party libs) and would
 * otherwise hit `mapErrorToUserMessage`'s exhaustive switch and throw a
 * SECONDARY "Unhandled OmniError kind: undefined" that masks the real
 * cause.
 */
function isOmniError(e: unknown): e is OmniError {
  if (typeof e !== 'object' || e === null) return false;
  const r = e as Record<string, unknown>;
  return (
    typeof r.code === 'string' &&
    typeof r.kind === 'string' &&
    typeof r.message === 'string'
  );
}

/**
 * Coerce any thrown value into an OmniError. Real OmniError envelopes
 * pass through unchanged; raw `Error` instances and primitive strings
 * become a synthetic `HostLocal` envelope so the user still sees the
 * underlying message. This is the safety net that prevents render-cascade
 * crashes when a non-share-WS code path accidentally lands at toast.error.
 */
function coerceToOmniError(e: unknown): OmniError {
  if (isOmniError(e)) return e;
  return {
    code: 'INTERNAL',
    kind: 'HostLocal',
    message: e instanceof Error ? e.message : String(e),
  };
}

export const toast = {
  success: (text: string) => sonnerToast.success(text),
  error: (error: OmniError | unknown) => {
    const mapped = mapErrorToUserMessage(coerceToOmniError(error));
    sonnerToast.error(mapped.text, {
      action: {
        label: 'Report this',
        onClick: () => navigator.clipboard.writeText(mapped.opaquePayload),
      },
    });
  },
  info: (text: string) => sonnerToast.info(text),
  warning: (text: string, options?: ExternalToast) => sonnerToast.warning(text, options),
};
