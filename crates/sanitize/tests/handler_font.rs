use sanitize::{sanitize_bundle, SanitizeError};

mod common;

#[test]
fn rejects_wrong_magic() {
    // Use all-0x41 bytes (ASCII 'A') — this does NOT match any executable
    // magic signature (MZ/ELF/Mach-O/PK/gzip), so dispatch reaches the font
    // handler, which rejects it as a bad font.
    let bytes = vec![0x41u8; 200];
    let (manifest, files) = common::bundle_with_font("fonts/x.ttf", bytes);
    let err = sanitize_bundle(&manifest, files).unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "font", .. }));
}

#[test]
fn rejects_too_short() {
    let bytes = b"\x01\x01".to_vec(); // 2 bytes, non-magic
    let (manifest, files) = common::bundle_with_font("fonts/x.ttf", bytes);
    let err = sanitize_bundle(&manifest, files).unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "font", .. }));
}

#[test]
fn accepts_real_ttf() {
    let bytes = include_bytes!("fixtures/font/ok.ttf").to_vec();
    let (manifest, files) = common::bundle_with_font("fonts/ok.ttf", bytes.clone());
    let (out, _r) = sanitize_bundle(&manifest, files).unwrap();
    assert_eq!(
        out["fonts/ok.ttf"].len(),
        bytes.len(),
        "pass-through preserves size"
    );
}
