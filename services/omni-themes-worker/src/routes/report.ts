import { Hono } from "hono";
import type { AppEnv } from "../types";
import { errorResponse } from "../lib/errors";
import { AuthError, verifyJws } from "../lib/auth";
import { checkAndIncrement } from "../lib/rate_limit";

/**
 * `POST /v1/report` — abuse-report intake (plan #008 W3T13, contract §4.7).
 *
 * Body: `{ artifact_id, category ∈ {illegal,malware,impersonation,nsfw,other},
 *          note?: string /* <= 500 chars *\/ }`.
 *
 * Flow: verify JWS → validate body → per-DF report quota (20/day via
 * `rate_limit` action="report") → confirm artifact exists → write
 * `reports:<uuid>` KV entry → bump `artifacts.report_count`. Returns
 * `{report_id, status:"received"}`. Admin review surface is deferred to #012.
 */
const app = new Hono<AppEnv>();

const CATEGORIES = new Set([
  "illegal",
  "malware",
  "impersonation",
  "nsfw",
  "other",
]);

const MAX_NOTE_LEN = 500;

interface ReportBody {
  artifact_id: string;
  category: string;
  note?: string;
}

function hexEncode(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) {
    s += bytes[i]!.toString(16).padStart(2, "0");
  }
  return s;
}

app.post("/", async (c) => {
  // Buffer body exactly once — workerd streams are single-read and verifyJws
  // needs the bytes for the body_sha256 claim check.
  const bodyBuf = await c.req.arrayBuffer();

  // --- Step 1: JWS auth. ---
  let auth;
  try {
    auth = await verifyJws(c.req.raw, c.env, bodyBuf);
  } catch (e) {
    if (e instanceof AuthError) {
      return errorResponse(401, e.code, e.message, {
        kind: "Auth",
        detail: e.detail,
      });
    }
    throw e;
  }

  // --- Step 2: parse + validate body. ---
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(bodyBuf));
  } catch {
    return errorResponse(400, "BAD_REQUEST", "body is not valid JSON", {
      kind: "Malformed",
      detail: "BadRequest",
    });
  }
  if (!parsed || typeof parsed !== "object") {
    return errorResponse(400, "BAD_REQUEST", "body must be a JSON object", {
      kind: "Malformed",
      detail: "BadRequest",
    });
  }
  const body = parsed as Partial<ReportBody>;

  if (typeof body.artifact_id !== "string" || body.artifact_id.length === 0) {
    return errorResponse(400, "BAD_REQUEST", "artifact_id is required", {
      kind: "Malformed",
      detail: "BadRequest",
    });
  }
  if (typeof body.category !== "string" || !CATEGORIES.has(body.category)) {
    return errorResponse(400, "BAD_REQUEST", "category is not in the allowed set", {
      kind: "Malformed",
      detail: "BadRequest",
    });
  }
  if (body.note !== undefined) {
    if (typeof body.note !== "string") {
      return errorResponse(400, "BAD_REQUEST", "note must be a string", {
        kind: "Malformed",
        detail: "BadRequest",
      });
    }
    // Contract §4.7 says "≤ 500 chars"; we interpret that as UTF-8 code-unit
    // length (String#length in ECMAScript, same as byte-count for ASCII plus
    // surrogate-pair count for BMP chars — sufficient to bound storage).
    if (body.note.length > MAX_NOTE_LEN) {
      return errorResponse(
        400,
        "BAD_REQUEST",
        `note exceeds ${MAX_NOTE_LEN}-character limit`,
        { kind: "Malformed", detail: "BadRequest" },
      );
    }
  }

  const pubkeyHex = hexEncode(auth.pubkey);
  const dfHex = hexEncode(auth.device_fp);

  // --- Step 3: per-DF report quota (20/day, scale-respecting). ---
  const rl = await checkAndIncrement(c.env, dfHex, pubkeyHex, "report");
  if (!rl.allowed) {
    if (rl.turnstile === true) {
      return errorResponse(428, "TURNSTILE_REQUIRED", "turnstile challenge required", {
        kind: "Quota",
        detail: "TurnstileRequired",
      });
    }
    return errorResponse(429, "RATE_LIMITED", "report quota exhausted", {
      kind: "Quota",
      detail: "RateLimited",
      retryAfter: rl.retry_after,
    });
  }

  // --- Step 4: confirm artifact exists. `artifacts.id` is the PK per
  // migrations/0001_initial_schema.sql; the contract names the field
  // `artifact_id` on the wire. ---
  const row = await c.env.META.prepare(
    "SELECT 1 AS present FROM artifacts WHERE id = ? LIMIT 1",
  )
    .bind(body.artifact_id)
    .first<{ present: number }>();
  if (!row) {
    return errorResponse(404, "NOT_FOUND", "artifact not found", {
      kind: "Malformed",
      detail: "NotFound",
    });
  }

  // --- Step 5: store report in KV. crypto.randomUUID is available in
  // workerd runtimes. ---
  const reportId = crypto.randomUUID();
  const record = {
    received_at: Math.floor(Date.now() / 1000),
    reporter_pubkey: pubkeyHex,
    reporter_df: dfHex,
    artifact_id: body.artifact_id,
    category: body.category,
    note: body.note ?? null,
  };
  await c.env.STATE.put(`reports:${reportId}`, JSON.stringify(record));

  // --- Step 6: bump artifacts.report_count. Read-then-write race accepted
  // per invariant #0; the counter is for ranking + moderator dashboards, not
  // a security gate. ---
  await c.env.META.prepare(
    "UPDATE artifacts SET report_count = report_count + 1 WHERE id = ?",
  )
    .bind(body.artifact_id)
    .run();

  return new Response(
    JSON.stringify({ report_id: reportId, status: "received" }),
    {
      status: 200,
      headers: { "content-type": "application/json; charset=utf-8" },
    },
  );
});

export default app;
