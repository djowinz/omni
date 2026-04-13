import { Hono } from "hono";
import type { AppEnv } from "../types";
import { notImplemented } from "../lib/errors";

const app = new Hono<AppEnv>();

app.get("/", () => notImplemented("GET /v1/list"));

export default app;
