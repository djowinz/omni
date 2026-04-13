import { Hono } from "hono";
import type { AppEnv } from "../types";
import { notImplemented } from "../lib/errors";

const app = new Hono<AppEnv>();

app.get("/:id", () => notImplemented("GET /v1/download/:id"));

export default app;
