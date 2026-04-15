// Typed toast wrapper — the sole import site for `sonner` in the renderer.
// Phase 3 specs import from `@/lib/toast`, never from `sonner` directly, so
// the toast library can be swapped in one file. `error()` always routes
// through `mapErrorToUserMessage` — no branching on `code` in the wrapper.

import { toast as sonnerToast } from "sonner";
import {
  mapErrorToUserMessage,
  type OmniError,
} from "./map-error-to-user-message";

export const toast = {
  success: (text: string) => sonnerToast.success(text),
  error: (error: OmniError) => {
    const mapped = mapErrorToUserMessage(error);
    sonnerToast.error(mapped.text, {
      action: {
        label: "Report this",
        onClick: () => navigator.clipboard.writeText(mapped.opaquePayload),
      },
    });
  },
  info: (text: string) => sonnerToast.info(text),
};
