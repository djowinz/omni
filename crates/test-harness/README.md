# test-harness

Dev-only workspace crate providing production-shaped construction helpers and
the reference-parser oracle helper for cross-crate integration tests.

See `docs/superpowers/specs/2026-04-19-integration-testing-discipline-design.md`
(pillars 2 and 3) for the authoring rules.

## Factories

- `build_share_context(tempdir)` — real `data_dir`, `StubGuard`,
  deterministic identity, inert theme-swap.
- `deterministic_keypair()` — fixed 32-byte seed; same bytes every run.
- `marathon_fixture()` — `(Manifest, files)` rooted at the reference overlay.
- `reference_overlay_bytes()` — packed + signed `.omnipkg` blob.
- `sample_list_row()` — populated `CachedArtifactDetail` matching the
  `/v1/list` wire shape.

## Reference-parser oracle

- `parse_canonical(bytes)` — extract structural shape via the
  `bundle::omni_schema` constants.
- `assert_reference_parsers_agree(fixture, sut_shape)` — panics if the SUT
  misses or invents top-level elements.

## Rule (writing-lessons §D7)

Integration tests construct shared state via these factories. Ad-hoc
construction in test code (open-coded `ShareContext` builders, hand-rolled
fixture manifests) is forbidden outside this crate itself.

## Adding a factory

Add the function + doc comment to `src/factories.rs`, re-export from
`src/lib.rs`, add a smoke test in the same file asserting the factory
returns non-empty, type-valid output.
