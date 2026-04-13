import { Hono } from "hono";
import type { AppEnv } from "./types";
import { errorResponse } from "./lib/errors";
import upload from "./routes/upload";
import download from "./routes/download";
import list from "./routes/list";
import artifact from "./routes/artifact";
import report from "./routes/report";
import gallery from "./routes/gallery";

export { BundleProcessor } from "./do/bundle_processor";

const app = new Hono<AppEnv>();

app.route("/v1/upload", upload);
app.route("/v1/download", download);
app.route("/v1/list", list);
app.route("/v1/artifact", artifact);
app.route("/v1/report", report);
app.route("/v1/me/gallery", gallery);

app.notFound(() => errorResponse(404, "NOT_FOUND", "no route matched"));

app.onError((err) => {
  const message = err instanceof Error ? err.message : String(err);
  return errorResponse(500, "SERVER_ERROR", message);
});

export default app;
