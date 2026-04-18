use sanitize::{sanitize_theme, SanitizeError};

fn check(bytes: &[u8]) -> SanitizeError {
    sanitize_theme(bytes).unwrap_err()
}

#[test]
fn rejects_pe_magic() {
    let mut b = vec![0x4D, 0x5A];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn rejects_elf_magic() {
    let mut b = vec![0x7F, 0x45, 0x4C, 0x46];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn rejects_mach_o_fat() {
    let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn rejects_mach_o_64_le() {
    let mut b = vec![0xCF, 0xFA, 0xED, 0xFE];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn rejects_mach_o_32_le() {
    let mut b = vec![0xCE, 0xFA, 0xED, 0xFE];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn rejects_nested_zip_magic() {
    let mut b = vec![0x50, 0x4B, 0x03, 0x04];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn rejects_gzip_magic() {
    let mut b = vec![0x1F, 0x8B];
    b.extend(b"body{}");
    assert!(matches!(
        check(&b),
        SanitizeError::RejectedExecutableMagic { .. }
    ));
}

#[test]
fn accepts_plain_css() {
    let (out, _r) = sanitize_theme(b"body{color:red}").unwrap();
    assert!(!out.is_empty());
}
