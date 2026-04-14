import { Hono } from "hono";
import type { AppEnv } from "./types";
import { errorResponse } from "./lib/errors";
import artifact from "./routes/artifact";
import config from "./routes/config";
import download from "./routes/download";
import gallery from "./routes/gallery";
import list from "./routes/list";
import report from "./routes/report";
import upload from "./routes/upload";

export { BundleProcessor } from "./do/bundle_processor";

const app = new Hono<AppEnv>();

app.route("/v1/upload", upload);
app.route("/v1/download", download);
app.route("/v1/list", list);
app.route("/v1/artifact", artifact);
app.route("/v1/config", config);
app.route("/v1/report", report);
app.route("/v1/me/gallery", gallery);

app.notFound(() => errorResponse(404, "NOT_FOUND", "no route matched"));

app.onError((err) => {
  const message = err instanceof Error ? err.message : String(err);
  return errorResponse(500, "SERVER_ERROR", message);
});

export default app;
