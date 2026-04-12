//! Throwaway smoke test: proves every Phase 0 symbol resolves.
//! Deleted after `cargo check --workspace` passes in Task 11.

#![allow(dead_code, unused_imports)]

use omni_sanitize::{
    sanitize_bundle, sanitize_theme, FileKind, FileReport, SanitizeError, SanitizeReport,
    SanitizeVersion, SANITIZE_VERSION,
};

use omni_bundle::{
    canonical_hash, pack, unpack, BundleError, FileEntry, Manifest, Tag,
    MAX_BUNDLE_COMPRESSED, MAX_BUNDLE_UNCOMPRESSED, MAX_COMPRESSION_RATIO, MAX_CSS, MAX_ENTRIES,
    MAX_FONT, MAX_IMAGE_RAW, MAX_IMAGE_REENCODED, MAX_OVERLAY, MAX_PATH_DEPTH, MAX_THEME_ONLY,
};

use omni_guard_trait::{DeviceId, Guard, GuardError, Signature, StubGuard};

use omni_identity::{Fingerprint, IdentityError, Keypair, PublicKey};

const _ASSERT_VERSION_IS_ONE: () = {
    if SANITIZE_VERSION.0 != 1 {
        panic!("SANITIZE_VERSION must be 1 in Phase 0");
    }
};

const _ASSERT_SIZE_LIMITS: () = {
    if MAX_ENTRIES != 32 {
        panic!("MAX_ENTRIES must be 32");
    }
    if MAX_PATH_DEPTH != 2 {
        panic!("MAX_PATH_DEPTH must be 2");
    }
    if MAX_COMPRESSION_RATIO != 100 {
        panic!("MAX_COMPRESSION_RATIO must be 100");
    }
};

fn _tag_enum_covers_vocabulary() {
    let _all = [
        Tag::Dark, Tag::Light, Tag::Minimal, Tag::Gaming, Tag::Neon, Tag::Retro,
        Tag::Cyberpunk, Tag::Pastel, Tag::HighContrast, Tag::Monospace, Tag::Racing,
        Tag::Flightsim, Tag::Mmo, Tag::Fps, Tag::Productivity, Tag::Creative,
    ];
}

fn _guard_impls_the_trait() {
    fn takes_guard<G: Guard>(_g: &G) {}
    takes_guard(&StubGuard);
}
