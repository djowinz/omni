//! Unified error type for the omni-host crate.

use std::fmt;

/// Covers the three failure domains in the host: Win32 API errors,
/// standard I/O errors, and freeform messages.
#[derive(Debug)]
pub enum HostError {
    Win32(windows::core::Error),
    Io(std::io::Error),
    Message(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win32(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Message(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Win32(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Message(_) => None,
        }
    }
}

impl From<windows::core::Error> for HostError {
    fn from(e: windows::core::Error) -> Self {
        Self::Win32(e)
    }
}

impl From<std::io::Error> for HostError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<String> for HostError {
    fn from(s: String) -> Self {
        Self::Message(s)
    }
}

impl From<&str> for HostError {
    fn from(s: &str) -> Self {
        Self::Message(s.to_string())
    }
}
