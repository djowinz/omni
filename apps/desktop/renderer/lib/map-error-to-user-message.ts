// Maps a host-supplied OmniError envelope into the renderer-facing shape
// consumed by toasts, banners, and dialog error surfaces.
//
// D-004-J rationale (retro-004): the editor renders only the server-supplied
// `message`, logs `detail` opaquely, and never parses `detail` to derive
// behavior or copy. The host owns wording; the renderer is presentation only.
// `opaquePayload` exists strictly so a "Report this" affordance can copy a
// stable JSON blob to the clipboard for support tickets without the renderer
// ever inspecting the envelope's contents.

export interface OmniError {
  code: string;
  kind: 'Malformed' | 'Unsafe' | 'Integrity' | 'Io' | 'Auth' | 'Quota' | 'Admin' | 'HostLocal';
  detail?: string;
  message: string;
}

export interface UserFacingError {
  severity: 'info' | 'warning' | 'error';
  icon: 'warn' | 'block' | 'retry' | 'info';
  /** ALWAYS `error.message` — never `error.detail`. */
  text: string;
  /** `JSON.stringify({ code, kind, detail })` for "Report this" clipboard. */
  opaquePayload: string;
}

export function mapErrorToUserMessage(error: OmniError): UserFacingError {
  let severity: UserFacingError['severity'];
  let icon: UserFacingError['icon'];

  // Exhaustive switch — adding a new OmniError kind without updating this
  // mapping must produce a TypeScript error at the `_exhaustive` line below.
  switch (error.kind) {
    case 'Auth':
    case 'Quota':
    case 'Admin':
      severity = 'error';
      icon = 'warn';
      break;
    case 'Integrity':
      severity = 'error';
      icon = 'block';
      break;
    case 'Io':
      severity = 'warning';
      icon = 'retry';
      break;
    case 'HostLocal':
      severity = 'warning';
      icon = 'warn';
      break;
    case 'Malformed':
    case 'Unsafe':
      severity = 'warning';
      icon = 'warn';
      break;
    default: {
      // Exhaustive check — adding a new OmniError kind without updating this
      // mapping must produce a TypeScript error at the `_exhaustive` line
      // below. At runtime we DON'T throw, though: throwing here masks the
      // primary error with a "Unhandled OmniError kind" surface that
      // doesn't tell the user (or us in logs) what actually went wrong.
      // Soft-fall-back to a Malformed-shaped severity so the original
      // message still surfaces in a toast.
      const _exhaustive: never = error.kind;
      void _exhaustive;
      console.warn(
        `[mapErrorToUserMessage] unknown OmniError kind=${String((error as { kind?: unknown }).kind)} — falling back to Malformed mapping`,
        error,
      );
      severity = 'warning';
      icon = 'warn';
      break;
    }
  }

  const opaquePayload = JSON.stringify({
    code: error.code,
    kind: error.kind,
    detail: error.detail ?? null,
  });

  return {
    severity,
    icon,
    text: error.message,
    opaquePayload,
  };
}
