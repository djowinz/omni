import { Hono } from "hono";
import type { AppEnv } from "../types";

const app = new Hono<AppEnv>();

// POST /v1/report
app.post("/", (c) =>
  c.json(
    {
      error: {
        code: "NOT_IMPLEMENTED",
        message: "route POST /v1/report is not implemented yet (sub-spec #007 skeleton)",
      },
    },
    501,
  ),
);

export default app;
