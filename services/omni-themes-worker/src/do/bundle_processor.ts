import { b64urlDecodeJson, b64urlEncode } from "../lib/base64url";
import { classifyWasmError, errorFromKind } from "../lib/errors";
import { hexEncode } from "../lib/hex";
import type { SanitizeReport } from "../lib/sanitize";
import { loadWasm } from "../lib/wasm";

/**
 * # DO fetch contract (for Gamma / route callers)
 *
 * - Method: `POST`
 * - Body: raw signed-bundle bytes (`application/octet-stream`), no envelope.
 * - Header `X-Omni-Bundle-Limits` (optional): `b64urlEncodeJson(limits)` where
 *   `limits` is the runtime `BundleLimits` shape the Worker reads from
 *   `config:limits` KV (cf. `routes/upload.ts::getLimits`). Passed through to
 *   both `identity.unpackSignedBundle` and `bundle.pack` so server policy is
 *   enforced at the WASM boundary.
 * - When the header is absent the DO falls back to `undefined` (crate default
 *   `BundleLimits`). This keeps existing callers + tests working until Gamma
 *   migrates `upload.ts` / `artifact.ts PATCH` to forward the header.
 *
 * One-liner for Gamma at each call site:
 *   `headers: { "X-Omni-Bundle-Limits": b64urlEncodeJson(limits) }`
 *
 * # Pipeline per spec §5 / plan W3T9
 *   1. Load WASM singletons.
 *   2. `identity.unpackSignedBundle(bytes, limits)` — verifies the embedded
 *      JWS and canonical-hash equivalence. Any failure surfaces through
 *      `classifyWasmError` (lib/errors.ts) — the single domain-carving table.
 *   3. For each (path, bytes) streamed from the handle:
 *        a. `sanitize.rejectExecutableMagic` (invariant #19c) — 422 on hit.
 *        b. Accumulate into a map for per-kind dispatch.
 *      Peak memory is bounded by the largest single file (invariant #19b).
 *   4. `sanitize.sanitizeBundle(manifest, files)` dispatches per
 *      `manifest.resource_kinds`. Returns `{sanitized, report}`.
 *   5. `bundle.pack(manifest, sanitized, limits)` repacks UNSIGNED.
 *      The worker holds no author priv-key (invariant #1); re-signing happens
 *      on the consumer host at install time.
 *   6. `bundle.canonicalHash(manifest)` — post-sanitize manifest canonical hash.
 *
 * Response envelope (JSON) matches the shape `sanitizeViaDO` decodes in
 * `src/lib/sanitize.ts`:
 *   { sanitized_bundle: base64url(bytes), sanitize_report, canonical_hash: hex }
 *
 * Error envelope matches `src/lib/errors.ts` so the upload route can pass the
 * DO's response through verbatim.
 *
 * The DO is keyed per device-fingerprint by the caller
 * (`env.BUNDLE_PROCESSOR.idFromName(df_hex)`) so concurrent uploads from one
 * device serialize without blocking others — invariant #10. The DO does not
 * use `state.storage` or `env` yet, so the constructor takes no parameters
 * (matches modern Workers `DurableObject` convention where both are optional).
 */
export class BundleProcessor {
  async fetch(req: Request): Promise<Response> {
    if (req.method !== "POST") {
      return errorFromKind("Malformed", "BadRequest", "method must be POST");
    }

    // Optional runtime limits passed via header (see contract above).
    let limits: unknown = undefined;
    const limitsHeader = req.headers.get("X-Omni-Bundle-Limits");
    if (limitsHeader !== null && limitsHeader.length > 0) {
      try {
        limits = b64urlDecodeJson(limitsHeader);
      } catch (e) {
        return errorFromKind(
          "Malformed",
          "BadRequest",
          `invalid X-Omni-Bundle-Limits header: ${errMessage(e)}`,
        );
      }
    }

    let bundleBytes: Uint8Array;
    try {
      const buf = await req.arrayBuffer();
      bundleBytes = new Uint8Array(buf);
    } catch (e) {
      return errorFromKind(
        "Malformed",
        "BadRequest",
        `failed to read request body: ${errMessage(e)}`,
      );
    }
    if (bundleBytes.length === 0) {
      return errorFromKind("Malformed", "BadRequest", "empty request body");
    }

    const { bundle, identity, sanitize } = await loadWasm();

    // Step 2: unpack + signature-verify. Any throw routes through the shared
    // classifier so the WASM substring table lives in exactly one place.
    let handle: {
      manifest: () => unknown;
      nextFile: () => { path: string; bytes: Uint8Array } | null;
      free?: () => void;
    };
    let manifest: unknown;
    try {
      handle = identity.unpackSignedBundle(bundleBytes, limits) as typeof handle;
      manifest = handle.manifest();
    } catch (e) {
      return classifyUnpackStage(e);
    }

    // Step 3: stream files, enforce magic-byte deny-list.
    const files: Record<string, Uint8Array> = {};
    try {
      for (;;) {
        const entry = handle.nextFile();
        if (entry === null) break;
        const magic = sanitize.rejectExecutableMagic(entry.bytes) as {
          ok: boolean;
          prefixHex?: string;
        };
        if (!magic.ok) {
          return errorFromKind(
            "Unsafe",
            "RejectedExecutableMagic",
            `rejected executable magic ${magic.prefixHex ?? "?"} at ${entry.path}`,
          );
        }
        files[entry.path] = entry.bytes;
      }
      handle.free?.();
    } catch (e) {
      return classifyUnpackStage(e);
    }

    // Step 4: dispatch sanitize per manifest.resource_kinds.
    let sanitized: Record<string, Uint8Array>;
    let report: SanitizeReport;
    try {
      const result = sanitize.sanitizeBundle(manifest, files) as {
        sanitized: Record<string, Uint8Array>;
        report: SanitizeReport;
      };
      sanitized = result.sanitized;
      report = result.report;
    } catch (e) {
      return classifySanitizeStage(e);
    }

    // Step 5: repack unsigned (worker never holds priv-keys — invariant #1).
    // Sanitize rewrites file bytes (theme CSS scrub, overlay template rewrite,
    // font subset-rebuild), so the manifest's FileEntry.sha256 values — which
    // were computed over the pre-sanitize bytes — no longer match. Rewrite
    // each entry's sha256 with the post-sanitize digest before packing;
    // `bundle.pack` verifies manifest vs file bytes and would otherwise
    // reject with Integrity.HashMismatch.
    let sanitizedManifest: {
      files: Array<{ path: string; sha256: string }>;
      [k: string]: unknown;
    };
    try {
      sanitizedManifest = await updateManifestHashes(
        manifest as typeof sanitizedManifest,
        sanitized,
      );
    } catch (e) {
      return errorFromKind(
        "Io",
        undefined,
        `rehash failed: ${errMessage(e)}`,
      );
    }
    let repacked: Uint8Array;
    try {
      repacked = bundle.pack(sanitizedManifest, sanitized, limits);
    } catch (e) {
      const c = classifyWasmError(e);
      return errorFromKind(c.kind, c.detail, `repack failed: ${c.message}`);
    }

    // Step 6: canonical hash over the post-sanitize manifest.
    let hash: Uint8Array;
    try {
      hash = bundle.canonicalHash(sanitizedManifest);
    } catch (e) {
      return errorFromKind(
        "Io",
        undefined,
        `canonical_hash failed: ${errMessage(e)}`,
      );
    }

    const responseBody = {
      sanitized_bundle: b64urlEncode(repacked),
      sanitize_report: report,
      canonical_hash: hexEncode(hash),
    };
    return new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: { "content-type": "application/json; charset=utf-8" },
    });
  }
}

/**
 * Stage-aware wrappers around `classifyWasmError`. The shared classifier is
 * domain-carved (invariant #19a) but cannot see *which* pipeline stage raised
 * the error — so some messages are ambiguous:
 *
 *   - A tampered zip payload trips the unpack `ZipBomb` guard; we want that
 *     surfaced as `Integrity.ZipBomb` (the artifact itself is rejected, not
 *     the content inside). `classifyWasmError` maps it to `Unsafe.ZipBomb`
 *     because the same substring can arise from inside content too.
 *   - A handler error at the sanitize stage whose message happens to contain
 *     "malformed" (e.g. ttf-parser "head table is malformed") should be an
 *     `Unsafe.HandlerRejected`, not `Malformed.ManifestInvalid`.
 *
 * Stage context disambiguates. These wrappers apply the stage-specific
 * overrides and delegate to `classifyWasmError` for everything else.
 */
function classifyUnpackStage(e: unknown): Response {
  const c = classifyWasmError(e);
  // Override: at unpack, ZipBomb is artifact-level tamper, not content-level.
  if (c.detail === "ZipBomb") {
    return errorFromKind("Integrity", "ZipBomb", c.message);
  }
  return errorFromKind(c.kind, c.detail, c.message);
}

function classifySanitizeStage(e: unknown): Response {
  const msg = errMessage(e);
  const lower = msg.toLowerCase();
  // Sanitize-stage errors from per-kind handlers (OTS reject, image-rs reject,
  // CSS parser reject) are semantic-safety rejections → Unsafe.HandlerRejected
  // regardless of whether the underlying parser's message contains words like
  // "malformed" that would otherwise trip the shared classifier's Malformed
  // branch.
  if (lower.includes("handler error")) {
    return errorFromKind("Unsafe", "HandlerRejected", msg);
  }
  const c = classifyWasmError(e);
  // Default sanitize failure → Unsafe (old local classifier behavior) unless
  // the shared classifier identified a more specific category (size exceeded,
  // executable magic, unknown resource kind).
  if (c.kind === "Io") {
    return errorFromKind("Unsafe", "HandlerRejected", c.message);
  }
  return errorFromKind(c.kind, c.detail, c.message);
}

function errMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  try {
    return String(e);
  } catch {
    return "<unrepresentable error>";
  }
}

/**
 * Rewrite `manifest.files[*].sha256` with the sha256 of the post-sanitize
 * bytes. Returns a shallow clone so the caller's original manifest value is
 * untouched (the wasm-bindgen manifest handle is an opaque JS object; we
 * treat the JSON shape as the contract and rebuild the entries array).
 */
async function updateManifestHashes(
  manifest: { files: Array<{ path: string; sha256: string }>; [k: string]: unknown },
  sanitized: Record<string, Uint8Array>,
): Promise<{ files: Array<{ path: string; sha256: string }>; [k: string]: unknown }> {
  const rewritten = await Promise.all(
    manifest.files.map(async (f) => {
      const bytes = sanitized[f.path];
      if (!bytes) {
        // Pack will surface this as Integrity.FileMissing; pass it through so
        // the classifier surfaces a structured error rather than a rehash one.
        return { ...f };
      }
      const digest = await crypto.subtle.digest("SHA-256", bytes);
      const arr = new Uint8Array(digest);
      return { ...f, sha256: hexEncode(arr) };
    }),
  );
  return { ...manifest, files: rewritten };
}
