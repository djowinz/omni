import { describe, it, expect } from "vitest";
import {
  mapErrorToUserMessage,
  type OmniError,
  type UserFacingError,
} from "../map-error-to-user-message";

type Kind = OmniError["kind"];

const ALL_KINDS: readonly Kind[] = [
  "Malformed",
  "Unsafe",
  "Integrity",
  "Io",
  "Auth",
  "Quota",
  "Admin",
  "HostLocal",
] as const;

interface Expected {
  severity: UserFacingError["severity"];
  icon: UserFacingError["icon"];
}

const EXPECTED: Record<Kind, Expected> = {
  Auth: { severity: "error", icon: "warn" },
  Quota: { severity: "error", icon: "warn" },
  Admin: { severity: "error", icon: "warn" },
  Integrity: { severity: "error", icon: "block" },
  Io: { severity: "warning", icon: "retry" },
  HostLocal: { severity: "warning", icon: "warn" },
  Malformed: { severity: "warning", icon: "warn" },
  Unsafe: { severity: "warning", icon: "warn" },
};

function makeError(kind: Kind, overrides: Partial<OmniError> = {}): OmniError {
  return {
    code: `E_${kind.toUpperCase()}`,
    kind,
    detail: `detail for ${kind}`,
    message: `human-readable message for ${kind}`,
    ...overrides,
  };
}

describe("mapErrorToUserMessage", () => {
  describe("severity + icon mapping per kind", () => {
    for (const kind of ALL_KINDS) {
      it(`maps ${kind} to severity=${EXPECTED[kind].severity}, icon=${EXPECTED[kind].icon}`, () => {
        const mapped = mapErrorToUserMessage(makeError(kind));
        expect(mapped.severity).toBe(EXPECTED[kind].severity);
        expect(mapped.icon).toBe(EXPECTED[kind].icon);
      });
    }
  });

  describe("text invariant", () => {
    it.each(ALL_KINDS)(
      "text === error.message for kind=%s",
      (kind) => {
        const error = makeError(kind);
        const mapped = mapErrorToUserMessage(error);
        expect(mapped.text).toBe(error.message);
      },
    );

    it("text is error.message even when detail differs wildly", () => {
      const error: OmniError = {
        code: "E_X",
        kind: "Integrity",
        detail: "internal stack trace the renderer must never surface",
        message: "Bundle failed integrity check.",
      };
      const mapped = mapErrorToUserMessage(error);
      expect(mapped.text).toBe("Bundle failed integrity check.");
      expect(mapped.text).not.toContain("stack trace");
    });
  });

  describe("opaquePayload round-trip", () => {
    it.each(ALL_KINDS)(
      "round-trips { code, kind, detail } for kind=%s",
      (kind) => {
        const error = makeError(kind);
        const mapped = mapErrorToUserMessage(error);
        const parsed = JSON.parse(mapped.opaquePayload);
        expect(parsed).toEqual({
          code: error.code,
          kind: error.kind,
          detail: error.detail,
        });
      },
    );

    it("preserves code verbatim", () => {
      const error = makeError("Auth", { code: "E_AUTH_REQUEST_EXPIRED" });
      const parsed = JSON.parse(
        mapErrorToUserMessage(error).opaquePayload,
      );
      expect(parsed.code).toBe("E_AUTH_REQUEST_EXPIRED");
    });
  });

  describe("missing detail handling", () => {
    it("produces valid JSON with detail: null when detail is undefined", () => {
      const error: OmniError = {
        code: "E_QUOTA",
        kind: "Quota",
        message: "Rate limit exceeded.",
      };
      const mapped = mapErrorToUserMessage(error);
      const parsed = JSON.parse(mapped.opaquePayload);
      expect(parsed).toEqual({
        code: "E_QUOTA",
        kind: "Quota",
        detail: null,
      });
    });

    it.each(ALL_KINDS)(
      "detail becomes null (not undefined, not missing) for kind=%s when omitted",
      (kind) => {
        const error: OmniError = {
          code: `E_${kind}`,
          kind,
          message: `msg for ${kind}`,
        };
        const mapped = mapErrorToUserMessage(error);
        const parsed = JSON.parse(mapped.opaquePayload) as {
          detail: unknown;
        };
        expect(parsed.detail).toBeNull();
        // Property present (not merely undefined-stripped).
        expect(Object.prototype.hasOwnProperty.call(parsed, "detail")).toBe(
          true,
        );
      },
    );
  });
});
