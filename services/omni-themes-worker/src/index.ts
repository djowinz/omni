import { Hono } from "hono";
import type { AppEnv } from "./types";
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

app.notFound((c) =>
  c.json(
    { error: { code: "NOT_FOUND", message: "no route matched" } },
    404,
  ),
);

app.onError((err, c) => {
  const message = err instanceof Error ? err.message : String(err);
  return c.json(
    { error: { code: "SERVER_ERROR", message } },
    500,
  );
});

export default app;
