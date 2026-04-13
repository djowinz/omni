import { Hono } from "hono";
import type { AppEnv } from "../types";
import { notImplemented } from "../lib/errors";

const app = new Hono<AppEnv>();

app.post("/", () => notImplemented("POST /v1/report"));

export default app;
