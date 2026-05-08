//! Error type for the fork-time beautifier.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BeautifyError {
    #[error("CSS beautify failed: {0}")]
    Css(String),
    #[error("HTML beautify failed: {0}")]
    Html(String),
    #[error("Omni beautify failed: {0}")]
    Omni(String),
}
