//! HTML beautifier — implementation in Task 3.

use crate::beautify::error::BeautifyError;

pub fn beautify_html(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    Ok(bytes.to_vec())
}
