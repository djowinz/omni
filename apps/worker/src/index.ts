import { Hono } from "hono";
import type { AppEnv } from "./types";
import { errorResponse } from "./lib/errors";
import admin from "./routes/admin";
import artifact from "./routes/artifact";
import config from "./routes/config";
import download from "./routes/download";
import gallery from "./routes/gallery";
import list from "./routes/list";
import report from "./routes/report";
import upload from "./routes/upload";

export { BundleProcessor } from "./do/bundle_processor";

const app = new Hono<AppEnv>();

const SEMVER_RE = /^\d+\.\d+\.\d+$/;

/**
 * Global client-version gate (plan #008 W4T14, contract §2).
 *
 * Every authed request must carry `X-Omni-Version` (semver) and
 * `X-Omni-Sanitize-Version` so the Worker can reason about client capability
 * ahead of JWS verify. Missing/malformed → 400 Malformed.
 *
 * Exemptions (documented inline — any change needs a contract update):
 *   - `GET /v1/config/*` — unauthenticated discovery endpoints used to
 *     bootstrap the client before it knows which versions to advertise
 *     (spec §4a/4b). Applying the gate here would deadlock bootstrap.
 *   - `GET /v1/download/*` — unauthenticated CDN-cacheable downloads (spec §6).
 *     Edge caches strip arbitrary headers; requiring them here would defeat
 *     caching and the anon-install flow.
 */
app.use("*", async (c, next) => {
  const path = new URL(c.req.url).pathname;
  if (path.startsWith("/v1/config/")) return next();
  if (path.startsWith("/v1/download/") && c.req.method === "GET") return next();

  const version = c.req.header("X-Omni-Version");
  const saniVer = c.req.header("X-Omni-Sanitize-Version");
  if (!version || !SEMVER_RE.test(version)) {
    return errorResponse(400, "BAD_REQUEST", "missing/invalid X-Omni-Version", {
      kind: "Malformed",
      detail: "BadRequest",
    });
  }
  if (!saniVer) {
    return errorResponse(400, "BAD_REQUEST", "missing X-Omni-Sanitize-Version", {
      kind: "Malformed",
      detail: "BadRequest",
    });
  }
  await next();
});

app.route("/v1/upload", upload);
app.route("/v1/download", download);
app.route("/v1/list", list);
app.route("/v1/artifact", artifact);
app.route("/v1/config", config);
app.route("/v1/report", report);
app.route("/v1/me/gallery", gallery);
app.route("/v1/admin", admin);

app.notFound(() => errorResponse(404, "NOT_FOUND", "no route matched"));

app.onError((err) => {
  const message = err instanceof Error ? err.message : String(err);
  return errorResponse(500, "SERVER_ERROR", message);
});

export default app;
