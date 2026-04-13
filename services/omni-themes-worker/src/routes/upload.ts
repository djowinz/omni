import { Hono } from "hono";
import type { AppEnv } from "../types";
import { notImplemented } from "../lib/errors";

const app = new Hono<AppEnv>();

// POST /v1/upload (mounted at /v1/upload by index.ts).
// #008 will swap this body to proxy the request into env.BUNDLE_PROCESSOR.
app.post("/", () => notImplemented("POST /v1/upload"));

export default app;
