# omni-themes-worker

Cloudflare Worker backend for Omni theme & bundle sharing.

This sub-spec (#007) ships only the infrastructure skeleton — every endpoint returns HTTP 501 with a structured error body. Real behavior lands in sub-specs #008, #010, #012.

## Retro-locked design decisions

These three choices were ratified during retro-validation of #007 (2026-04-12). Any change to them requires an umbrella update, not a sub-spec change.

1. **Router = Hono** (`hono@^4`). Typed bindings via `Hono<{ Bindings: Env }>`, typed route params (`c.req.param("id"): string`), middleware-ready for #008.
2. **All uploads route through `BundleProcessor` Durable Object** — theme and bundle alike. One auditable sanitize path. See umbrella §2.3 and §7 risk #7.
3. **Test strategy = two tiers.** Tier C: `app.request()` in-process for route/envelope logic. Tier B: `@cloudflare/vitest-pool-workers` for anything touching `env` or the DO. No `wrangler.unstable_dev`.

A fourth decision was added during retro v3 (2026-04-14) per retro-005 D6/D7: **KV config seeding via `npm run bootstrap` + versioned `seed/*.json` files.** Zero Worker hot-path logic; re-runnable post-redeploy. See "KV bootstrap" below.

## Layout

```
wrangler.toml              # bindings: BLOBS (R2), META (D1), STATE (KV), BUNDLE_PROCESSOR (DO)
migrations/
  0001_initial_schema.sql  # D1 schema (umbrella §6.2)
seed/
  vocab.json               # config:vocab KV seed (admin-editable via #012)
  limits.json              # config:limits KV seed (admin-editable via #012)
scripts/
  bootstrap-kv.mjs         # run once per deploy to write the seeds to KV
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

## Local dev loop

Run the full Omni stack locally — worker (miniflare R2/D1/KV) + Electron + host — in one command.

### Quick start

```bash
cargo build -p host                # build host binary (first run only)
make dev                           # start everything + auto-seed
```

### Commands

| Command                       | Effect                                                                             |
| ----------------------------- | ---------------------------------------------------------------------------------- |
| `make dev`                    | Start everything (wrangler + Electron + host) + seed                               |
| `make dev ARGS=--no-seed`     | Start everything with no seed (empty-state testing)                                |
| `make dev-worker-seeded`      | Start wrangler dev + admin pubkey + seed, NO Electron (API iteration via curl/etc) |
| `make dev-worker`             | Bare `wrangler dev` (no admin injection, no seed — public routes only)             |
| `make dev-seed`               | Re-run seed against a running dev stack (idempotent)                               |
| `make dev-reset`              | Wipe miniflare state + re-migrate + re-bootstrap + re-seed                         |
| `make dev-reset-identity`     | Regenerate user + admin keypairs (restart `make dev` after)                        |
| `make dev-kill`               | Force-kill any process bound to 8787 / 9473                                        |
| `make dev-admin ARGS='stats'` | Run any `omni-admin` subcommand (e.g. `review`, `stats`) against the local worker  |

The Make targets are thin wrappers around the `omni-dev` Rust binary at `tools/dev-orchestrator/`. You can also invoke it directly:

```bash
cargo run -p dev-orchestrator -- run
cargo run -p dev-orchestrator -- reset
cargo run -p dev-orchestrator -- admin -- stats --json
```

### Fixture caveats

- Fixture artifacts are **display-only**. Their `content_hash` is a dummy value and no bundle blob exists in R2. Clicking Install on a fixture will fail at blob fetch — this is expected.
- To validate install end-to-end, upload your own artifact via the Electron app's Publish flow, then Install it from Discover.

### Troubleshooting

- `EADDRINUSE 8787` — run `make dev-kill`, then retry `make dev`.
- "host binary missing" — run `cargo build -p host`.
- Admin commands fail with auth error after `make dev-reset-identity` — restart `make dev` so wrangler picks up the new admin pubkey.

## Local development

Requires Node ≥ 20.

```bash
cd apps/worker
npm install
npm test                   # 15 tests across tier C + tier B
npx wrangler dev --local   # interactive local dev at http://127.0.0.1:8787
node scripts/bootstrap-kv.mjs --local  # seed local KV
```

`wrangler dev --local` and `vitest-pool-workers` both use miniflare — no real Cloudflare IDs required.

## Production provisioning (manual, one-time)

These mutate the user's Cloudflare account and are NOT automated.

```bash
# 1. Authenticate
npx wrangler login

# 2. Create the R2 bucket
npx wrangler r2 bucket create omni-themes-blobs

# 3. Create the D1 database (records its UUID)
npx wrangler d1 create omni-themes-meta

# 4. Create the KV namespace (records its ID)
npx wrangler kv namespace create OMNI_THEMES_STATE

# 5. Paste the D1 UUID and KV ID into wrangler.toml placeholders.

# 6. Apply the D1 schema remotely
npm run migrate:remote

# 7. Seed KV config (config:vocab + config:limits)
npm run bootstrap

# 8. Deploy
npx wrangler deploy
```

Uncomment the `route =` lines in `wrangler.toml` with the user's real zone before prod deploy.

### KV bootstrap

The Worker depends on two KV entries under `STATE`:

- `config:vocab` — admin-editable tag vocabulary; clients fetch via `GET /v1/config/vocab` and cache for 24h.
- `config:limits` — admin-editable bundle-size policy; clients fetch via `GET /v1/config/limits`. `max_bundle_compressed` is also the HTTP upload-body cap.

Seed values live at `seed/vocab.json` and `seed/limits.json`. `npm run bootstrap` writes both via `wrangler kv key put`. Re-run any time you want to reset to seed values — admin edits via the #012 CLI are overwritten. Local-dev equivalent: `node scripts/bootstrap-kv.mjs --local`.

Security-level constants (path depth, compression ratio, path length) are NOT in KV — they stay compile-time in `omni-bundle`.

## Environment variables

| Var                            | Prod   | Dev   | Meaning                                        |
| ------------------------------ | ------ | ----- | ---------------------------------------------- |
| `OMNI_THEMES_ENV`              | `prod` | `dev` | Tags logs; controls error verbosity in #008.   |
| `OMNI_THEMES_RATE_LIMIT_SCALE` | `1`    | `10`  | Multiplies per-day quotas; `10` = 10× relaxed. |

## Contract references

- HTTP surface: `docs/contracts/worker-api.md` (includes §4.9 + §4.10 for config endpoints)
- D1 schema source-of-truth: umbrella §6.2
- Umbrella: `docs/superpowers/specs/2026-04-10-theme-sharing-umbrella.md`
- Sub-spec: `docs/superpowers/specs/2026-04-10-theme-sharing-007-worker-infrastructure.md` (includes §Retro findings + 2026-04-14 D6/D7 refinement)

## What this sub-spec does NOT do

- Signature verification (`src/lib/auth.ts` is a throwing stub — #008)
- Rate limiting (`src/lib/rate_limit.ts` is a throwing stub — #008)
- Sanitize pipeline (`src/lib/sanitize.ts` is a throwing stub — #008 inside the DO)
- Upload proxying to `BundleProcessor` (route wired, body is still 501 — #008 swaps the body)
- Config endpoint handlers (`GET /v1/config/vocab`, `GET /v1/config/limits`) — 501 stubs; #008 wires them to `env.STATE.get(...)`
- Moderation / admin endpoints (#012)
- Any real business logic for any endpoint (#008)
