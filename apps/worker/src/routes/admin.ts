import { Hono } from "hono";
import type { AppEnv } from "../types";
import { errorFromKind } from "../lib/errors";
import { verifyJws, AuthError } from "../lib/auth";
import { isModerator } from "../lib/moderator";
import { _resetConfigCaches } from "./config";
import { hexEncode, hexDecode } from "../lib/hex";

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
const STATUS_ORDER: readonly ReportStatus[] = ["pending", "reviewed", "actioned"];
const VALID_ACTIONS: ReadonlySet<ReportAction> = new Set([
  "no_action",
  "removed",
  "banned_author",
]);

/**
 * Admin-facing report view — contract §4.13 field names.
 *
 * Stored record uses `category`/`note` (matching §4.7 intake body fields from
 * sub-spec #008). Contract §4.13 names the admin-visible fields `reason` and
 * `notes`. We translate on read only; the stored shape is unchanged so #008's
 * report intake tests keep passing. `reporter_pubkey` is dropped — not in the
 * contract item shape. `action_notes` is propagated as optional.
 */
interface AdminReportView {
  id: string;
  artifact_id: string;
  reason: string;
  notes: string | null;
  reporter_df: string;
  received_at: number;
  status: ReportStatus;
  actioned_by: string | null;
  action: ReportAction | null;
  action_notes?: string;
}

function adminReportView(row: ReportRecord): AdminReportView {
  const view: AdminReportView = {
    id: row.id,
    artifact_id: row.artifact_id,
    reason: row.category,
    notes: row.note,
    reporter_df: row.reporter_df,
    received_at: row.received_at,
    status: row.status,
    actioned_by: row.actioned_by,
    action: row.action,
  };
  if (row.action_notes !== undefined) view.action_notes = row.action_notes;
  return view;
}

/** Composite cursor for the no-status-filter case.
 *
 *  When `status` is absent, we iterate the three per-status prefixes in order
 *  (pending → reviewed → actioned) and concatenate items up to `limit`. The
 *  cursor carries the current status + KV cursor so a follow-up page resumes
 *  mid-prefix or at the next prefix. JSON+base64url — small shapes, low volume
 *  (contract §4.13 is a moderator dashboard endpoint, not a hot path).
 */
interface ReportsCursor {
  status: ReportStatus;
  kv?: string;
}

function encodeReportsCursor(c: ReportsCursor): string {
  return btoa(JSON.stringify(c)).replace(/=+$/, "");
}
function decodeReportsCursor(s: string): ReportsCursor | null {
  try {
    const obj = JSON.parse(atob(s)) as ReportsCursor;
    if (!VALID_STATUSES.has(obj.status)) return null;
    return obj;
  } catch {
    return null;
  }
}

async function listReportsForStatus(
  env: AppEnv["Bindings"],
  status: ReportStatus,
  limit: number,
  kvCursor: string | undefined,
): Promise<{ items: AdminReportView[]; nextKvCursor: string | undefined }> {
  const list = await env.STATE.list({
    prefix: `reports-by-status:${status}:`,
    limit,
    cursor: kvCursor,
  });
  const items: AdminReportView[] = [];
  for (const key of list.keys) {
    const id = key.name.split(":").pop()!;
    const rec = (await env.STATE.get(`reports:${id}`, "json")) as
      | ReportRecord
      | null;
    if (rec !== null) items.push(adminReportView(rec));
  }
  const nextKvCursor = list.list_complete ? undefined : list.cursor;
  return { items, nextKvCursor };
}

// GET /v1/admin/reports?status=&cursor=&limit=
app.get("/reports", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }

  const statusQuery = c.req.query("status");
  const statusParam =
    statusQuery === undefined || statusQuery === ""
      ? undefined
      : (statusQuery as ReportStatus);
  if (statusParam !== undefined && !VALID_STATUSES.has(statusParam)) {
    return errorFromKind(
      "Malformed",
      "BadRequest",
      `invalid status: ${statusParam}`,
    );
  }
  const cursorParam = c.req.query("cursor") ?? undefined;
  const limitRaw = c.req.query("limit");
  let limit = 25; // contract §4.13 default
  if (limitRaw !== undefined) {
    const parsed = Number.parseInt(limitRaw, 10);
    if (!Number.isFinite(parsed) || parsed <= 0 || parsed > 100) {
      return errorFromKind(
        "Malformed",
        "BadRequest",
        "limit must be in [1, 100]",
      );
    }
    limit = parsed;
  }

  // Single-status path — plain KV cursor passthrough (back-compat with earlier
  // callers that don't know about the composite cursor shape).
  if (statusParam !== undefined) {
    const { items, nextKvCursor } = await listReportsForStatus(
      c.env,
      statusParam,
      limit,
      cursorParam,
    );
    const resp: { items: AdminReportView[]; next_cursor?: string } = { items };
    if (nextKvCursor) resp.next_cursor = nextKvCursor;
    return new Response(JSON.stringify(resp), {
      status: 200,
      headers: { "content-type": "application/json; charset=utf-8" },
    });
  }

  // No-filter path — walk pending → reviewed → actioned until we fill `limit`
  // or run out. Composite cursor carries {status, kv?}.
  let startStatusIdx = 0;
  let kvCursor: string | undefined;
  if (cursorParam !== undefined) {
    const decoded = decodeReportsCursor(cursorParam);
    if (decoded === null) {
      return errorFromKind("Malformed", "BadRequest", "invalid cursor");
    }
    startStatusIdx = STATUS_ORDER.indexOf(decoded.status);
    kvCursor = decoded.kv;
  }

  const items: AdminReportView[] = [];
  let nextCursor: string | undefined;
  for (let i = startStatusIdx; i < STATUS_ORDER.length; i++) {
    const status = STATUS_ORDER[i]!;
    const remaining = limit - items.length;
    if (remaining <= 0) {
      // Page filled exactly on a boundary — next cursor resumes at this status.
      nextCursor = encodeReportsCursor({ status, kv: kvCursor });
      break;
    }
    const page = await listReportsForStatus(c.env, status, remaining, kvCursor);
    items.push(...page.items);
    if (page.nextKvCursor) {
      // More within this status — stay here on resume.
      nextCursor = encodeReportsCursor({ status, kv: page.nextKvCursor });
      break;
    }
    // Status exhausted; fall through to next status with a fresh KV cursor.
    kvCursor = undefined;
  }

  const resp: { items: AdminReportView[]; next_cursor?: string } = { items };
  if (nextCursor) resp.next_cursor = nextCursor;
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
  const linked_artifact = await c.env.META.prepare(
    "SELECT * FROM artifacts WHERE id = ? LIMIT 1",
  )
    .bind(report.artifact_id)
    .first();
  return new Response(
    JSON.stringify({
      report: adminReportView(report),
      linked_artifact: linked_artifact ?? null,
    }),
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
  // Treat explicit JSON null as "not provided" — the CLI emits `notes: null`
  // when the operator omits `--notes`, and `typeof null === "object"` would
  // otherwise reject it. Defense-in-depth; the CLI is fixed separately.
  if (notes !== undefined && notes !== null && typeof notes !== "string") {
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

  // Contract §4.15: response is the updated report object directly (no wrapper).
  return new Response(JSON.stringify(adminReportView(updated)), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

// ---------------------------------------------------------------------------
// POST /v1/admin/artifact/:id/remove — spec §3 + §5 cascade consumer surface.
// ---------------------------------------------------------------------------

/**
 * Tombstone an artifact. Writes/updates `tombstones(content_hash)`, flips
 * `artifacts.is_removed = 1`, and best-effort deletes the R2 bundle blob +
 * thumbnail. Every step is idempotent by construction:
 *   - `INSERT OR REPLACE` on tombstones keys on content_hash → re-runs are
 *     structurally no-ops.
 *   - `UPDATE is_removed = 1` is naturally idempotent.
 *   - R2 `delete()` on a missing key resolves (R2 semantics match S3 delete).
 *
 * Returns "not_found" when no artifact row exists (caller decides whether
 * that's a 404 or, for the cascade caller, a silent skip). Returns
 * "already_tombstoned" when the artifact was previously tombstoned AND a
 * tombstone row already exists for its content_hash — the common
 * "this is the second time I ran this" signal. Otherwise "removed".
 *
 * Consumed by T8 (ban-author cascade) and the `/artifact/:id/remove` handler
 * below. Keep the return-value contract stable — T8 branches on it.
 */
export type TombstoneStatus = "removed" | "already_tombstoned" | "not_found";
export interface TombstoneResult {
  status: TombstoneStatus;
  /** Canonical content_hash of the artifact. Undefined only when `status ===
   *  "not_found"` (no artifact row to read it from). */
  content_hash?: string;
}

export async function tombstoneArtifact(
  env: AppEnv["Bindings"],
  id: string,
  reason: string,
): Promise<TombstoneResult> {
  const row = await env.META.prepare(
    "SELECT id, content_hash, thumbnail_hash, is_removed FROM artifacts WHERE id = ?",
  )
    .bind(id)
    .first<{
      id: string;
      content_hash: string;
      thumbnail_hash: string | null;
      is_removed: number;
    }>();
  if (!row) return { status: "not_found" };

  if (row.is_removed) {
    const existingTomb = await env.META.prepare(
      "SELECT content_hash FROM tombstones WHERE content_hash = ?",
    )
      .bind(row.content_hash)
      .first<{ content_hash: string }>();
    if (existingTomb) {
      return { status: "already_tombstoned", content_hash: row.content_hash };
    }
  }

  const now = Math.floor(Date.now() / 1000);
  await env.META.prepare(
    "INSERT OR REPLACE INTO tombstones (content_hash, reason, removed_at) VALUES (?, ?, ?)",
  )
    .bind(row.content_hash, reason, now)
    .run();
  await env.META.prepare(
    "UPDATE artifacts SET is_removed = 1, updated_at = ? WHERE id = ?",
  )
    .bind(now, id)
    .run();

  // Best-effort R2 cleanup. The tombstone row is the authoritative signal;
  // leftover blobs are an operational concern (GC sweep) not a correctness
  // one. Swallow per-object errors so a failing thumbnail delete doesn't
  // strand the bundle, and vice versa.
  try {
    await env.BLOBS.delete(`bundles/${row.content_hash}.omnipkg`);
  } catch {
    /* swallow */
  }
  if (row.thumbnail_hash) {
    try {
      await env.BLOBS.delete(`thumbnails/${row.thumbnail_hash}.png`);
    } catch {
      /* swallow */
    }
  }

  return { status: "removed", content_hash: row.content_hash };
}

app.post("/artifact/:id/remove", async (c) => {
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
  const { reason } = parsed as { reason?: unknown };
  if (typeof reason !== "string" || reason.length === 0) {
    return errorFromKind("Malformed", "BadRequest", "reason must be a non-empty string");
  }

  const id = c.req.param("id");
  const result = await tombstoneArtifact(c.env, id, reason);
  if (result.status === "not_found") {
    return errorFromKind("Malformed", "NotFound", `artifact ${id} not found`);
  }

  return new Response(
    JSON.stringify({
      artifact_id: id,
      status: result.status,
      content_hash: result.content_hash,
    }),
    { status: 200, headers: { "content-type": "application/json; charset=utf-8" } },
  );
});

// ---------------------------------------------------------------------------
// Denylist admin endpoints (T8) — spec §5, contract §4.17/§4.18.
//
// Pubkey denylist is authoritative in D1 (`authors.is_denied`) AND mirrored
// into KV (`denylist:pubkey:<hex>`) so the hot-path verifyJws check (and the
// rate-limit middleware) don't need a D1 round-trip. Device denylist lives
// in KV only (no `devices` table); the rate-limit middleware consumes it.
//
// Ban-author cascade is idempotent by construction: step 1 (D1+KV denylist)
// rewrites the same state; step 2 (cascade) iterates only `is_removed = 0`
// rows, which shrinks to 0 after the first run, so `cascade_count` is 0 on
// rerun. Per-row errors are caught and counted so a single R2 hiccup doesn't
// abort the sweep.
// ---------------------------------------------------------------------------

const HEX64_RE = /^[0-9a-f]{64}$/;

async function parseJsonBody(body: ArrayBuffer): Promise<Record<string, unknown> | Response> {
  let parsed: unknown;
  try {
    parsed = body.byteLength === 0 ? {} : JSON.parse(new TextDecoder().decode(body));
  } catch {
    return errorFromKind("Malformed", "BadRequest", "body is not valid JSON");
  }
  if (!parsed || typeof parsed !== "object") {
    return errorFromKind("Malformed", "BadRequest", "body must be a JSON object");
  }
  return parsed as Record<string, unknown>;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

// POST /v1/admin/pubkey/ban
app.post("/pubkey/ban", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }
  const parsedOrErr = await parseJsonBody(body);
  if (parsedOrErr instanceof Response) return parsedOrErr;
  const { pubkey, reason } = parsedOrErr as { pubkey?: unknown; reason?: unknown };

  if (typeof pubkey !== "string") {
    return errorFromKind("Malformed", "BadRequest", "pubkey must be a string");
  }
  const pubkeyHex = pubkey.toLowerCase();
  if (!HEX64_RE.test(pubkeyHex)) {
    return errorFromKind("Malformed", "BadRequest", "pubkey must be 64-char hex");
  }
  if (typeof reason !== "string" || reason.length === 0) {
    return errorFromKind("Malformed", "BadRequest", "reason must be a non-empty string");
  }

  const pubkeyBlob = hexDecode(pubkeyHex);
  const now = Math.floor(Date.now() / 1000);

  // D1 authoritative denylist flag. Create-or-flip in two statements to avoid
  // the BLOB-PK upsert dance on D1 (INSERT ... ON CONFLICT with BLOB PK can
  // be finicky; two statements are trivially idempotent).
  await c.env.META.prepare(
    `INSERT OR IGNORE INTO authors (pubkey, created_at, total_uploads, is_new_creator, is_denied)
     VALUES (?, ?, 0, 0, 1)`,
  )
    .bind(pubkeyBlob, now)
    .run();
  await c.env.META.prepare(
    `UPDATE authors SET is_denied = 1 WHERE pubkey = ?`,
  )
    .bind(pubkeyBlob)
    .run();

  // KV mirror for fast-path verifyJws + rate_limit denylist checks.
  await c.env.STATE.put(
    `denylist:pubkey:${pubkeyHex}`,
    JSON.stringify({ reason, at: now }),
  );

  // Cascade: tombstone every live artifact by this author.
  const liveRows = await c.env.META.prepare(
    "SELECT id FROM artifacts WHERE author_pubkey = ? AND is_removed = 0",
  )
    .bind(pubkeyBlob)
    .all<{ id: string }>();

  let cascade_count = 0;
  let cascade_errors = 0;
  for (const row of liveRows.results ?? []) {
    try {
      const result = await tombstoneArtifact(
        c.env,
        row.id,
        `author ban: ${reason}`,
      );
      if (result.status === "removed") cascade_count++;
    } catch {
      cascade_errors++;
    }
  }

  return jsonResponse({
    pubkey: pubkeyHex,
    status: "banned",
    cascade_count,
    cascade_errors,
  });
});

// POST /v1/admin/pubkey/unban
app.post("/pubkey/unban", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }
  const parsedOrErr = await parseJsonBody(body);
  if (parsedOrErr instanceof Response) return parsedOrErr;
  const { pubkey } = parsedOrErr as { pubkey?: unknown };

  if (typeof pubkey !== "string") {
    return errorFromKind("Malformed", "BadRequest", "pubkey must be a string");
  }
  const pubkeyHex = pubkey.toLowerCase();
  if (!HEX64_RE.test(pubkeyHex)) {
    return errorFromKind("Malformed", "BadRequest", "pubkey must be 64-char hex");
  }

  const pubkeyBlob = hexDecode(pubkeyHex);
  await c.env.META.prepare(
    `UPDATE authors SET is_denied = 0 WHERE pubkey = ?`,
  )
    .bind(pubkeyBlob)
    .run();
  await c.env.STATE.delete(`denylist:pubkey:${pubkeyHex}`);

  // Tombstones are intentionally NOT resurrected (spec §5 — unban lifts the
  // gate on future uploads but does not undo moderation decisions on prior
  // content).
  return jsonResponse({ pubkey: pubkeyHex, status: "unbanned" });
});

// POST /v1/admin/device/ban
app.post("/device/ban", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }
  const parsedOrErr = await parseJsonBody(body);
  if (parsedOrErr instanceof Response) return parsedOrErr;
  const { device_fp, reason } = parsedOrErr as {
    device_fp?: unknown;
    reason?: unknown;
  };

  if (typeof device_fp !== "string") {
    return errorFromKind("Malformed", "BadRequest", "device_fp must be a string");
  }
  const dfHex = device_fp.toLowerCase();
  if (!HEX64_RE.test(dfHex)) {
    return errorFromKind("Malformed", "BadRequest", "device_fp must be 64-char hex");
  }
  if (typeof reason !== "string" || reason.length === 0) {
    return errorFromKind("Malformed", "BadRequest", "reason must be a non-empty string");
  }

  const now = Math.floor(Date.now() / 1000);
  await c.env.STATE.put(
    `denylist:device:${dfHex}`,
    JSON.stringify({ reason, at: now }),
  );
  return jsonResponse({ device_fp: dfHex, status: "banned" });
});

// POST /v1/admin/device/unban
app.post("/device/unban", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }
  const parsedOrErr = await parseJsonBody(body);
  if (parsedOrErr instanceof Response) return parsedOrErr;
  const { device_fp } = parsedOrErr as { device_fp?: unknown };

  if (typeof device_fp !== "string") {
    return errorFromKind("Malformed", "BadRequest", "device_fp must be a string");
  }
  const dfHex = device_fp.toLowerCase();
  if (!HEX64_RE.test(dfHex)) {
    return errorFromKind("Malformed", "BadRequest", "device_fp must be 64-char hex");
  }

  await c.env.STATE.delete(`denylist:device:${dfHex}`);
  return jsonResponse({ device_fp: dfHex, status: "unbanned" });
});

// ---------------------------------------------------------------------------
// GET /v1/admin/stats — contract §4.19, spec §3 (T9).
//
// Aggregate dashboard counts. All independent reads run via Promise.all so
// the endpoint is one parallel round-trip per backend, not serial.
//
// Sources:
//   pending_reports : count of `reports-by-status:pending:*` KV keys.
//   banned_pubkeys  : D1 count of authors with is_denied = 1.
//   banned_devices  : count of `denylist:device:*` KV keys.
//   total_artifacts : D1 count of artifacts with is_removed = 0 (live only —
//                     tombstones don't count as "inventory").
//   total_installs  : D1 SUM(install_count) across all artifacts (live +
//                     removed; install counts are historical).
//   vocab_version   : `config:vocab` JSON .version, default 0.
//   limits_version  : `config:limits` JSON .version, default 0.
// ---------------------------------------------------------------------------
app.get("/stats", async (c) => {
  const body = await c.req.arrayBuffer();
  try {
    await requireModerator(c.req.raw, c.env, body);
  } catch (r) {
    return r as Response;
  }

  const [
    pendingReports,
    reviewedReports,
    actionedReports,
    bannedPubkeysRow,
    bannedDevices,
    totalArtifactsRow,
    tombstonedArtifactsRow,
    totalInstallsRow,
    vocabBlob,
    limitsBlob,
  ] = await Promise.all([
    countKvPrefix(c.env, "reports-by-status:pending:"),
    countKvPrefix(c.env, "reports-by-status:reviewed:"),
    countKvPrefix(c.env, "reports-by-status:actioned:"),
    c.env.META.prepare(
      "SELECT COUNT(*) AS c FROM authors WHERE is_denied = 1",
    ).first<{ c: number | bigint | string }>(),
    countKvPrefix(c.env, "denylist:device:"),
    c.env.META.prepare(
      "SELECT COUNT(*) AS c FROM artifacts WHERE is_removed = 0",
    ).first<{ c: number | bigint | string }>(),
    c.env.META.prepare(
      "SELECT COUNT(*) AS c FROM artifacts WHERE is_removed = 1",
    ).first<{ c: number | bigint | string }>(),
    c.env.META.prepare(
      "SELECT COALESCE(SUM(install_count), 0) AS c FROM artifacts",
    ).first<{ c: number | bigint | string }>(),
    c.env.STATE.get("config:vocab", "json") as Promise<
      { version?: number } | null
    >,
    c.env.STATE.get("config:limits", "json") as Promise<
      { version?: number } | null
    >,
  ]);

  return jsonResponse({
    pending_reports: pendingReports,
    reviewed_reports: reviewedReports,
    actioned_reports: actionedReports,
    banned_pubkeys: toNum(bannedPubkeysRow?.c),
    banned_devices: bannedDevices,
    total_artifacts: toNum(totalArtifactsRow?.c),
    tombstoned_artifacts: toNum(tombstonedArtifactsRow?.c),
    total_installs: toNum(totalInstallsRow?.c),
    vocab_version: vocabBlob?.version ?? 0,
    limits_version: limitsBlob?.version ?? 0,
  });
});

/** Coerce D1's possibly BigInt/string COUNT/SUM result to a plain number.
 *  D1 returns numbers in the common case; the cast is defensive against
 *  driver behavior on large counts. */
function toNum(v: number | bigint | string | undefined | null): number {
  if (v === null || v === undefined) return 0;
  if (typeof v === "number") return v;
  if (typeof v === "bigint") return Number(v);
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
}

/** Paginate `STATE.list({prefix})` to completion, returning the total key
 *  count. Single-page is the common case (pending queues stay small);
 *  the loop is for future growth. Local helper — not shared, no other
 *  caller today. */
async function countKvPrefix(
  env: AppEnv["Bindings"],
  prefix: string,
): Promise<number> {
  let count = 0;
  let cursor: string | undefined;
  do {
    const list = await env.STATE.list({ prefix, limit: 1000, cursor });
    count += list.keys.length;
    cursor = list.list_complete ? undefined : list.cursor;
  } while (cursor);
  return count;
}

export default app;
