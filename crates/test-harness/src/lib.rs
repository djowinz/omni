//! Production-shaped test harness for the Omni workspace.
//!
//! This crate is dev-only (`publish = false`). It exposes factories that
//! construct shared state the way the production host binary does — real
//! `data_dir` tempdirs, `StubGuard`, deterministic identity keys — so
//! integration tests across `crates/host`, `crates/sanitize`, and
//! `crates/bundle` can share one construction path rather than open-coding
//! setup in every test file.
//!
//! See `docs/superpowers/specs/2026-04-19-integration-testing-discipline-design.md`
//! (Pillar 3) for the authoring rule.

pub mod factories;
pub mod reference_oracle;

pub use factories::{
    build_share_context, deterministic_keypair, marathon_fixture, reference_overlay_bytes,
    sample_list_row,
};
pub use reference_oracle::{assert_reference_parsers_agree, parse_canonical, ParsedShape};
