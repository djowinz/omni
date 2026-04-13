# omni-themes-worker

Cloudflare Worker backend for Omni theme & bundle sharing.

This sub-spec (#007) ships only the infrastructure skeleton — every endpoint returns HTTP 501 with a structured error body. Real behavior lands in sub-specs #008, #010, #012.

## Retro-locked design decisions

These three choices were ratified during retro-validation of #007 (2026-04-12). Any change to them requires an umbrella update, not a sub-spec change.

1. **Router = Hono** (`hono@^4`). Typed bindings via `Hono<{ Bindings: Env }>`, typed route params (`c.req.param("id"): string`), middleware-ready for #008.
2. **All uploads route through `BundleProcessor` Durable Object** — theme and bundle alike. One auditable sanitize path. See umbrella §2.3 and §7 risk #7.
3. **Test strategy = two tiers.** Tier C: `app.request()` in-process for route/envelope logic. Tier B: `@cloudflare/vitest-pool-workers` for anything touching `env` or the DO. No `wrangler.unstable_dev`.

## Layout

```
wrangler.toml              # bindings: BLOBS (R2), META (D1), STATE (KV), BUNDLE_PROCESSOR (DO)
migrations/
  0001_initial_schema.sql  # D1 schema (umbrella §6.2)
src/
  index.ts                 # Hono app entry; mounts sub-routers; exports BundleProcessor
  env.ts                   # typed Env interface
  types.ts                 # ErrorCode vocabulary + AppEnv type alias
  routes/                  # one Hono sub-router per endpoint group, all 501
  do/bundle_processor.ts   # Durable Object skeleton — single sanitize entry
  lib/                     # errors.ts + auth/rate_limit/sanitize throwing stubs
vitest.config.ts           # uses defineWorkersConfig (pool-workers)
test/
  errors.test.ts           # unit test (tier C)
  routes.test.ts           # route-wiring tests (tier C, app.request)
  do.test.ts               # DO-binding anchor (tier B, pool-workers)
```

## Local development

Requires Node ≥ 20.

```bash
cd services/omni-themes-worker
npm install
npm test                   # 13 tests across tier C + tier B
npx wrangler dev --local   # interactive local dev at http://127.0.0.1:8787
```

`wrangler dev --local` and `vitest-pool-workers` both use miniflare — no real Cloudflare IDs required.

## Production provisioning (manual, one-time)

These mutate the user's Cloudflare account and are NOT automated.

```bash
npx wrangler login
npx wrangler r2 bucket create omni-themes-blobs
npx wrangler d1 create omni-themes-meta              # record the UUID
npx wrangler kv:namespace create OMNI_THEMES_STATE   # record the ID
# Paste the D1 UUID and KV ID into wrangler.toml placeholders.
npm run migrate:remote
npx wrangler deploy
```

Uncomment the `route =` lines in `wrangler.toml` with the user's real zone before prod deploy.

## Environment variables

| Var | Prod | Dev | Meaning |
|---|---|---|---|
| `OMNI_THEMES_ENV` | `prod` | `dev` | Tags logs; controls error verbosity in #008. |
| `OMNI_THEMES_RATE_LIMIT_SCALE` | `1` | `10` | Multiplies per-day quotas; `10` = 10× relaxed. |

## Contract references

- HTTP surface: `docs/superpowers/specs/contracts/worker-api.md`
- D1 schema source-of-truth: umbrella §6.2
- Umbrella: `docs/superpowers/specs/2026-04-10-theme-sharing-umbrella.md`
- Sub-spec: `docs/superpowers/specs/2026-04-10-theme-sharing-007-worker-infrastructure.md` (includes §Retro findings)

## What this sub-spec does NOT do

- Signature verification (`src/lib/auth.ts` is a throwing stub — #008)
- Rate limiting (`src/lib/rate_limit.ts` is a throwing stub — #008)
- Sanitize pipeline (`src/lib/sanitize.ts` is a throwing stub — #008 inside the DO)
- Upload proxying to `BundleProcessor` (route wired, body is still 501 — #008 swaps the body)
- Moderation / admin endpoints (#012)
- Any real business logic for any endpoint (#008)
