import { Hono } from "hono";
import type { AppEnv } from "../types";

const app = new Hono<AppEnv>();

// POST /v1/upload (mounted at /v1/upload by index.ts)
app.post("/", (c) =>
  c.json(
    {
      error: {
        code: "NOT_IMPLEMENTED",
        message:
          "route POST /v1/upload is not implemented yet (sub-spec #007 skeleton; #008 will proxy to BundleProcessor DO)",
      },
    },
    501,
  ),
);

export default app;
