import type { Env } from "../env";
import { errorFromKind } from "../lib/errors";
import type { SanitizeReport } from "../lib/sanitize";
import { loadWasm } from "../lib/wasm";

/**
 * Single sanitize entry point for every bundle upload that needs per-isolate
 * memory isolation (fonts, images, anything larger than the inline theme path
 * can safely process). The DO is keyed per device-fingerprint by the caller
 * (`env.BUNDLE_PROCESSOR.idFromName(df_hex)`) so concurrent uploads from one
 * device serialize without blocking others — invariant #10.
 *
 * Pipeline per spec §5 / plan W3T9:
 *   1. Load WASM singletons.
 *   2. `identity.unpackSignedBundle(bytes)` — verifies the embedded JWS and
 *      canonical-hash equivalence. Any failure here surfaces as `Integrity.*`.
 *   3. For each (path, bytes) streamed from the handle:
 *        a. `sanitize.rejectExecutableMagic` (invariant #19c) — 422 on hit.
 *        b. Accumulate into a map for per-kind dispatch.
 *      Peak memory is bounded by the largest single file (invariant #19b).
 *   4. `sanitize.sanitizeBundle(manifest, files)` dispatches per
 *      `manifest.resource_kinds`. Returns `{sanitized, report}`.
 *   5. `bundle.pack(manifest, sanitized, undefined)` repacks UNSIGNED.
 *      The worker holds no author priv-key (invariant #1); re-signing happens
 *      on the consumer host at install time.
 *   6. `bundle.canonicalHash(manifest)` — post-sanitize manifest canonical hash.
 *
 * Response envelope (JSON) matches the shape `sanitizeViaDO` decodes in
 * `src/lib/sanitize.ts`:
 *   { sanitized_bundle: base64url(bytes), sanitize_report, canonical_hash: hex }
 *
 * Error envelope matches `src/lib/errors.ts` — same `{error,kind,detail}`
 * shape the rest of the Worker returns, so the upload route can pass through
 * the DO's response verbatim without re-wrapping.
 */
export class BundleProcessor {
  constructor(
    private readonly state: DurableObjectState,
    private readonly env: Env,
  ) {}

  async fetch(req: Request): Promise<Response> {
    void this.state;
    void this.env;

    if (req.method !== "POST") {
      return errorFromKind("Malformed", "BadRequest", "method must be POST");
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

    // Step 2: unpack + signature-verify. Any throw here is an Integrity or
    // Malformed problem — the tampered-fixture note in W1T3 calls out that
    // bogus zip layout trips ZipBomb before the JWS check, so we don't pin
    // on a specific sub-kind; we classify by message substring.
    let handle: {
      manifest: () => unknown;
      nextFile: () => { path: string; bytes: Uint8Array } | null;
      free?: () => void;
    };
    let manifest: unknown;
    try {
      handle = identity.unpackSignedBundle(bundleBytes, undefined) as typeof handle;
      manifest = handle.manifest();
    } catch (e) {
      return classifyUnpackError(e);
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
      return classifyUnpackError(e);
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
      return classifySanitizeError(e);
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
      repacked = bundle.pack(sanitizedManifest, sanitized, undefined);
    } catch (e) {
      return errorFromKind(
        "Io",
        undefined,
        `repack failed: ${errMessage(e)}`,
      );
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
      sanitized_bundle: base64UrlEncode(repacked),
      sanitize_report: report,
      canonical_hash: hexEncode(hash),
    };
    return new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: { "content-type": "application/json; charset=utf-8" },
    });
  }
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
 * Classify a thrown value from `unpackSignedBundle` / `nextFile` into the
 * structured error envelope. WASM bindings surface `JsValue::from_str(...)`
 * messages; we key off substrings emitted by `omni_bundle::BundleError`,
 * `omni_identity`, and the WASM glue in crates/omni-(bundle|identity|sanitize)/src/wasm.rs.
 */
function classifyUnpackError(e: unknown): Response {
  const msg = errMessage(e);
  const lower = msg.toLowerCase();
  // Executable-magic (belt-and-braces — the DO also runs the explicit check).
  if (lower.includes("rejected executable magic")) {
    return errorFromKind("Unsafe", "RejectedExecutableMagic", msg);
  }
  // Size caps from BundleLimits.
  if (lower.includes("size exceeded") || lower.includes("sizeexceeded")) {
    return errorFromKind("Malformed", "SizeExceeded", msg);
  }
  // Signature / canonical-hash binding failures.
  if (
    lower.includes("signature") ||
    lower.includes("canonical_hash mismatch") ||
    lower.includes("jws") ||
    lower.includes("missing signature")
  ) {
    return errorFromKind("Integrity", "SignatureInvalid", msg);
  }
  // Zip-bomb guard also fires on tampered payloads that perturb the DEFLATE
  // stream — surface as Integrity so upload callers treat it as a rejection
  // of the artifact, not a transient IO fault.
  if (lower.includes("zipbomb") || lower.includes("zip bomb")) {
    return errorFromKind("Integrity", "ZipBomb", msg);
  }
  // Structural problems fall through to Malformed (400).
  return errorFromKind("Malformed", "ManifestInvalid", msg);
}

function classifySanitizeError(e: unknown): Response {
  const msg = errMessage(e);
  const lower = msg.toLowerCase();
  if (lower.includes("rejected executable magic")) {
    return errorFromKind("Unsafe", "RejectedExecutableMagic", msg);
  }
  if (lower.includes("size exceeded")) {
    return errorFromKind("Malformed", "SizeExceeded", msg);
  }
  if (lower.includes("unknown resource kind")) {
    return errorFromKind("Malformed", "ManifestInvalid", msg);
  }
  // Handler failures (OTS reject, CSS parse, image-rs reject, …) are
  // semantic-safety rejections → Unsafe.
  return errorFromKind("Unsafe", "HandlerRejected", msg);
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

// ---- encoding helpers -------------------------------------------------------

function base64UrlEncode(bytes: Uint8Array): string {
  // Workers runtime has btoa(). Chunk to keep string-build cheap for multi-MB
  // bundles; 0x8000 window matches the idiom used by the noble libs.
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  const b64 = btoa(binary);
  return b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function hexEncode(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) {
    s += bytes[i].toString(16).padStart(2, "0");
  }
  return s;
}

