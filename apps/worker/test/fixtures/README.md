# Worker test fixtures

Deterministic signed `.omnipkg` bundles used across every route test in
`apps/worker/test/`. Generated on demand (`pnpm pretest`)
from a hardcoded seed — **never committed to git**.

## Regenerating

From `apps/worker/`:

```bash
pnpm pretest          # build:wasm + generate
# or directly:
node test/fixtures/generate.mjs
```

This writes:

- `theme-only.omnipkg` — one CSS file. Exercises the Worker's inline-sanitize
  path (no DO dispatch).
- `bundle-with-font.omnipkg` — CSS + a font entry. Triggers the
  `BundleProcessor` Durable Object dispatch path.
- `bundle-tampered.omnipkg` — post-sign byte flip of the `bundle-with-font`
  base, inside the zip payload region covered by the canonical hash. Must be
  rejected by `Identity.unpackSignedBundle`.
- `fixtures.json` — index of `{ content_hash_hex, size_bytes, … }` per
  fixture, plus the shared `_meta` block (`seed_hex`, `pubkey_hex`, `df_hex`).
  Tests load this instead of recomputing hashes.

## Seeds

| Thing               | Value                                           |
| ------------------- | ----------------------------------------------- |
| `TEST_KEYPAIR_SEED` | `Uint8Array(32).fill(7)` — pure test material   |
| Pubkey              | Derived from seed via Web Crypto (Ed25519)      |
| Device fingerprint  | `sha256(pubkey \|\| "omni-test-df")` — 32 bytes |

The DF derivation is a fixture-only convention. Real Omni clients derive DF
from `omni-guard::device_fingerprint()` against hardware identity. Tests that
need a stable DF bind to this computed value.

## Font fixture caveat

`bundle-with-font.omnipkg` embeds a 12-byte minimal SFNT stub (`numTables=0`).
It is **not** an OTS-parseable font: `omni-sanitize`'s font handler will
reject it at sanitize time. This is intentional — the fixture's purpose is
exercising the _dispatch route_, not the sanitize-success path. A future
sanitize-success fixture should swap in an OFL-licensed open font (e.g.
Roboto-subset) and document the license here.

## Why binaries are gitignored

`.omnipkg` files are outputs of the WASM bundle-pack pipeline. Committing
them would pin a specific WASM build's output bytes and silently mask parity
bugs between native Rust and WASM pack. Regenerating on every CI run proves
the pack pipeline is still deterministic.
