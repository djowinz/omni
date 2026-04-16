import { Hono } from "hono";
import type { AppEnv } from "../types";
import { errorFromKind } from "../lib/errors";
import { verifyJws, AuthError } from "../lib/auth";
import { isModerator } from "../lib/moderator";
import { _resetConfigCaches } from "./config";
import { hexEncode } from "../lib/hex";

/**
 * Moderator-only admin endpoints. Spec §9a/9b, contract §4.11/4.12.
 *
 * Every request:
 *   1. JWS-verified via `verifyJws` (shared body-buffering convention).
 *   2. `kid` (authed pubkey) must appear in the `OMNI_ADMIN_PUBKEYS` allowlist.
 *   3. Body validated against the contract vocabulary; on success, KV mutated,
 *      `version` bumped, module-level config caches invalidated so the next
 *      public read observes the change.
 *
 * Mutation ordering note: we read-modify-write KV without a CAS since admin
 * edits are low-frequency and a single moderator is the common case
 * (contract §4.10 note on moderator list). Last-writer-wins is acceptable.
 */

const TAG_RE = /^[a-z][a-z0-9-]{1,19}$/;

interface VocabShape {
  tags: string[];
  version: number;
}

interface LimitsShape {
  max_bundle_compressed: number;
  max_bundle_uncompressed: number;
  max_entries: number;
  version: number;
  updated_at: number;
}

/** Buffer the body + run JWS + moderator check. Returns the pubkey hex on
 *  success; throws a `Response` (as a bare reject value) on any gate failure
 *  so the caller can `try { ... } catch (r) { return r; }`. */
async function requireModerator(
  req: Request,
  env: AppEnv["Bindings"],
  body: ArrayBuffer,
): Promise<string> {
  let pubkey: Uint8Array;
  try {
    const auth = await verifyJws(req, env, body);
    pubkey = auth.pubkey;
  } catch (e) {
    if (e instanceof AuthError) {
      throw errorFromKind("Auth", e.detail, e.message);
    }
    throw errorFromKind(
      "Auth",
      "MalformedEnvelope",
      e instanceof Error ? e.message : String(e),
    );
  }
  const pubkeyHex = hexEncode(pubkey);
  if (!isModerator(pubkeyHex, env)) {
    throw errorFromKind(
      "Admin",
      "NotModerator",
      "pubkey is not on the moderator allowlist",
    );
  }
  return pubkeyHex;
}

const app = new Hono<AppEnv>();

// ---------------------------------------------------------------------------
// PATCH /v1/admin/vocab — contract §4.11, spec §9a
// ---------------------------------------------------------------------------
app.patch("/vocab", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }

  let parsed: unknown;
  try {
    parsed = body.byteLength === 0 ? {} : JSON.parse(new TextDecoder().decode(body));
  } catch {
    return errorFromKind("Malformed", "BadRequest", "body is not valid JSON");
  }
  if (!parsed || typeof parsed !== "object") {
    return errorFromKind("Malformed", "BadRequest", "body must be a JSON object");
  }
  const { add, remove } = parsed as { add?: unknown; remove?: unknown };
  const addList = add === undefined ? [] : add;
  const removeList = remove === undefined ? [] : remove;
  if (!Array.isArray(addList) || !Array.isArray(removeList)) {
    return errorFromKind("Admin", "BadTag", "add/remove must be string arrays");
  }
  for (const t of addList) {
    if (typeof t !== "string" || !TAG_RE.test(t)) {
      return errorFromKind(
        "Admin",
        "BadTag",
        `invalid tag in 'add': ${JSON.stringify(t)}`,
      );
    }
  }
  for (const t of removeList) {
    if (typeof t !== "string" || !TAG_RE.test(t)) {
      return errorFromKind(
        "Admin",
        "BadTag",
        `invalid tag in 'remove': ${JSON.stringify(t)}`,
      );
    }
  }
  if (addList.length === 0 && removeList.length === 0) {
    return errorFromKind(
      "Admin",
      "NoOp",
      "at least one of 'add' or 'remove' must be non-empty",
    );
  }

  const current = (await c.env.STATE.get("config:vocab", "json")) as
    | VocabShape
    | null;
  if (current === null) {
    return errorFromKind("Io", undefined, "config:vocab not seeded");
  }

  const tagSet = new Set(current.tags);
  for (const t of addList as string[]) tagSet.add(t);
  for (const t of removeList as string[]) tagSet.delete(t);

  const next: VocabShape = {
    tags: [...tagSet].sort(),
    version: (current.version ?? 0) + 1,
  };
  await c.env.STATE.put("config:vocab", JSON.stringify(next));
  _resetConfigCaches();

  return new Response(JSON.stringify(next), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

// ---------------------------------------------------------------------------
// PATCH /v1/admin/limits — contract §4.12, spec §9b
// ---------------------------------------------------------------------------
app.patch("/limits", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }

  let parsed: unknown;
  try {
    parsed = body.byteLength === 0 ? {} : JSON.parse(new TextDecoder().decode(body));
  } catch {
    return errorFromKind("Malformed", "BadRequest", "body is not valid JSON");
  }
  if (!parsed || typeof parsed !== "object") {
    return errorFromKind("Malformed", "BadRequest", "body must be a JSON object");
  }

  const patch = parsed as Partial<Record<keyof LimitsShape, unknown>>;

  const current = (await c.env.STATE.get("config:limits", "json")) as
    | LimitsShape
    | null;
  if (current === null) {
    return errorFromKind("Io", undefined, "config:limits not seeded");
  }

  const next: LimitsShape = { ...current };

  function takeNum(field: "max_bundle_compressed" | "max_bundle_uncompressed" | "max_entries"): Response | null {
    if (patch[field] === undefined) return null;
    const v = patch[field];
    if (typeof v !== "number" || !Number.isFinite(v) || !Number.isInteger(v)) {
      return errorFromKind("Admin", "BadValue", `${field} must be an integer`);
    }
    if (v <= 0) {
      return errorFromKind("Admin", "BadValue", `${field} must be > 0`);
    }
    next[field] = v;
    return null;
  }

  const errs =
    takeNum("max_bundle_compressed") ??
    takeNum("max_bundle_uncompressed") ??
    takeNum("max_entries");
  if (errs !== null) return errs;

  if (next.max_bundle_compressed > next.max_bundle_uncompressed) {
    return errorFromKind(
      "Admin",
      "BadValue",
      "max_bundle_compressed must be ≤ max_bundle_uncompressed",
    );
  }
  if (next.max_entries < 1) {
    return errorFromKind("Admin", "BadValue", "max_entries must be ≥ 1");
  }

  // Orphan check — only if we're lowering max_bundle_compressed.
  const force = c.req.header("X-Omni-Admin-Force") === "true";
  if (!force && next.max_bundle_compressed < current.max_bundle_compressed) {
    const largest = await largestLiveArtifactSize(c.env);
    if (largest !== null && largest > next.max_bundle_compressed) {
      return errorFromKind(
        "Admin",
        "WouldOrphanArtifacts",
        `lowering max_bundle_compressed to ${next.max_bundle_compressed} would orphan existing artifact(s) (largest=${largest}); set X-Omni-Admin-Force: true to override`,
      );
    }
  }

  next.version = (current.version ?? 0) + 1;
  next.updated_at = Math.floor(Date.now() / 1000);

  await c.env.STATE.put("config:limits", JSON.stringify(next));
  _resetConfigCaches();

  return new Response(JSON.stringify(next), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

/**
 * Scan D1 for non-removed artifacts' content_hashes and query R2 head() for
 * each to find the largest blob size. Admin edits are low-frequency; O(n)
 * head lookups are acceptable and simpler than denormalizing size into D1
 * (which would ripple through upload + PATCH paths in sibling tasks).
 */
async function largestLiveArtifactSize(
  env: AppEnv["Bindings"],
): Promise<number | null> {
  const rows = await env.META.prepare(
    "SELECT DISTINCT content_hash FROM artifacts WHERE is_removed = 0",
  ).all<{ content_hash: string }>();
  let max: number | null = null;
  for (const row of rows.results ?? []) {
    const head = await env.BLOBS.head(row.content_hash);
    if (head && typeof head.size === "number") {
      if (max === null || head.size > max) max = head.size;
    }
  }
  return max;
}

// ---------------------------------------------------------------------------
// Reports queue (T6) — contract §4.13–§4.15.
// ---------------------------------------------------------------------------

type ReportStatus = "pending" | "reviewed" | "actioned";
type ReportAction = "no_action" | "removed" | "banned_author";

interface ReportRecord {
  id: string;
  received_at: number;
  reporter_pubkey: string;
  reporter_df: string;
  artifact_id: string;
  category: string;
  note: string | null;
  status: ReportStatus;
  actioned_by: string | null;
  action: ReportAction | null;
  action_notes?: string;
}

const VALID_STATUSES: ReadonlySet<ReportStatus> = new Set([
  "pending",
  "reviewed",
  "actioned",
]);
const VALID_ACTIONS: ReadonlySet<ReportAction> = new Set([
  "no_action",
  "removed",
  "banned_author",
]);

// GET /v1/admin/reports?status=&cursor=&limit=
app.get("/reports", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }

  const statusParam = (c.req.query("status") ?? "pending") as ReportStatus;
  if (!VALID_STATUSES.has(statusParam)) {
    return errorFromKind(
      "Malformed",
      "BadRequest",
      `invalid status: ${statusParam}`,
    );
  }
  const cursor = c.req.query("cursor") ?? undefined;
  const limitRaw = c.req.query("limit");
  let limit = 50;
  if (limitRaw !== undefined) {
    const parsed = Number.parseInt(limitRaw, 10);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return errorFromKind("Malformed", "BadRequest", "limit must be > 0");
    }
    limit = Math.min(parsed, 100);
  }

  const list = await c.env.STATE.list({
    prefix: `reports-by-status:${statusParam}:`,
    limit,
    cursor,
  });

  const items: ReportRecord[] = [];
  for (const key of list.keys) {
    // Value is the id; but derive from the record itself for durability.
    const id = key.name.split(":").pop()!;
    const rec = (await c.env.STATE.get(`reports:${id}`, "json")) as
      | ReportRecord
      | null;
    if (rec !== null) items.push(rec);
  }

  const resp: { items: ReportRecord[]; next_cursor?: string } = { items };
  if (!list.list_complete && list.cursor) resp.next_cursor = list.cursor;
  return new Response(JSON.stringify(resp), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

// GET /v1/admin/report/:id
app.get("/report/:id", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }
  const id = c.req.param("id");
  const report = (await c.env.STATE.get(`reports:${id}`, "json")) as
    | ReportRecord
    | null;
  if (report === null) {
    return errorFromKind("Malformed", "NotFound", `report ${id} not found`);
  }
  const artifact = await c.env.META.prepare(
    "SELECT * FROM artifacts WHERE id = ? LIMIT 1",
  )
    .bind(report.artifact_id)
    .first();
  return new Response(
    JSON.stringify({ report, artifact: artifact ?? null }),
    {
      status: 200,
      headers: { "content-type": "application/json; charset=utf-8" },
    },
  );
});

// POST /v1/admin/report/:id/action
app.post("/report/:id/action", async (c) => {
  const body = await c.req.arrayBuffer();
  let pubkeyHex: string;
  try {
    pubkeyHex = await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }

  let parsed: unknown;
  try {
    parsed = body.byteLength === 0 ? {} : JSON.parse(new TextDecoder().decode(body));
  } catch {
    return errorFromKind("Malformed", "BadRequest", "body is not valid JSON");
  }
  if (!parsed || typeof parsed !== "object") {
    return errorFromKind("Malformed", "BadRequest", "body must be a JSON object");
  }
  const { action, notes } = parsed as { action?: unknown; notes?: unknown };
  if (typeof action !== "string" || !VALID_ACTIONS.has(action as ReportAction)) {
    return errorFromKind(
      "Malformed",
      "BadRequest",
      `invalid action: ${JSON.stringify(action)}`,
    );
  }
  if (notes !== undefined && typeof notes !== "string") {
    return errorFromKind("Malformed", "BadRequest", "notes must be a string");
  }

  const id = c.req.param("id");
  const existing = (await c.env.STATE.get(`reports:${id}`, "json")) as
    | ReportRecord
    | null;
  if (existing === null) {
    return errorFromKind("Malformed", "NotFound", `report ${id} not found`);
  }
  if (existing.status !== "pending") {
    return errorFromKind("Admin", "NoOp", "report already actioned");
  }

  const newStatus: ReportStatus =
    action === "no_action" ? "reviewed" : "actioned";
  const updated: ReportRecord = {
    ...existing,
    status: newStatus,
    actioned_by: pubkeyHex,
    action: action as ReportAction,
  };
  if (typeof notes === "string") updated.action_notes = notes;

  // Swap secondary-index key. Best-effort: delete old before writing new so
  // a crash between the two leaves the queue clean of the pending entry.
  await c.env.STATE.delete(
    `reports-by-status:${existing.status}:${existing.received_at}:${existing.id}`,
  );
  await c.env.STATE.put(
    `reports-by-status:${newStatus}:${existing.received_at}:${existing.id}`,
    existing.id,
  );
  await c.env.STATE.put(`reports:${id}`, JSON.stringify(updated));

  return new Response(JSON.stringify({ status: "ok", report: updated }), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

export default app;
