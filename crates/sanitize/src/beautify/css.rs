//! CSS beautifier — implementation in Task 2.

use crate::beautify::error::BeautifyError;

pub fn beautify_css(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    // Stub: pass-through until Task 2 lands. Tests in Task 1's mod.rs
    // dispatch_tests rely on this not erroring.
    Ok(bytes.to_vec())
}
