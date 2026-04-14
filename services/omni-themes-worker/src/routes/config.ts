import { Hono } from "hono";
import type { AppEnv } from "../types";
import { notImplemented } from "../lib/errors";

const app = new Hono<AppEnv>();

// GET /v1/config/vocab — returns the current tag vocabulary.
// #008 will replace this with `c.env.STATE.get("config:vocab", "json")`.
app.get("/vocab", () => notImplemented("GET /v1/config/vocab"));

// GET /v1/config/limits — returns the current bundle-size policy.
// #008 will replace this with `c.env.STATE.get("config:limits", "json")`.
app.get("/limits", () => notImplemented("GET /v1/config/limits"));

export default app;
