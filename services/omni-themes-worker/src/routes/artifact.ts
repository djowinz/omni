import { Hono } from "hono";
import type { AppEnv } from "../types";
import { notImplemented } from "../lib/errors";

const app = new Hono<AppEnv>();

app.get("/:id", () => notImplemented("GET /v1/artifact/:id"));

// #008 will swap PATCH to proxy into env.BUNDLE_PROCESSOR (same path as uploads).
app.patch("/:id", () => notImplemented("PATCH /v1/artifact/:id"));

app.delete("/:id", () => notImplemented("DELETE /v1/artifact/:id"));

export default app;
