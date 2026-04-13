import { Hono } from "hono";
import type { AppEnv } from "../types";

const app = new Hono<AppEnv>();

// GET /v1/artifact/:id
app.get("/:id", (c) =>
  c.json(
    {
      error: {
        code: "NOT_IMPLEMENTED",
        message: "route GET /v1/artifact/:id is not implemented yet (sub-spec #007 skeleton)",
      },
    },
    501,
  ),
);

// PATCH /v1/artifact/:id
app.patch("/:id", (c) =>
  c.json(
    {
      error: {
        code: "NOT_IMPLEMENTED",
        message:
          "route PATCH /v1/artifact/:id is not implemented yet (sub-spec #007 skeleton; #008 will proxy to BundleProcessor DO)",
      },
    },
    501,
  ),
);

// DELETE /v1/artifact/:id
app.delete("/:id", (c) =>
  c.json(
    {
      error: {
        code: "NOT_IMPLEMENTED",
        message: "route DELETE /v1/artifact/:id is not implemented yet (sub-spec #007 skeleton)",
      },
    },
    501,
  ),
);

export default app;
