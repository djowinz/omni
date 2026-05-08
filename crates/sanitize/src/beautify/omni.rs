//! .omni beautifier — implementation in Task 4.

use crate::beautify::error::BeautifyError;

pub fn beautify_omni(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    Ok(bytes.to_vec())
}
