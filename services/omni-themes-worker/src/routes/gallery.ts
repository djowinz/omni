import { Hono } from "hono";
import type { AppEnv } from "../types";

const app = new Hono<AppEnv>();

// GET /v1/me/gallery
app.get("/", (c) =>
  c.json(
    {
      error: {
        code: "NOT_IMPLEMENTED",
        message: "route GET /v1/me/gallery is not implemented yet (sub-spec #007 skeleton)",
      },
    },
    501,
  ),
);

export default app;
