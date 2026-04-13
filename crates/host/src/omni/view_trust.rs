//! Trust classification for Ultralight Views.
//!
//! Determines which sandboxing layers (scoped FS, URL-scheme filter,
//! begin-loading rejection) apply to content rendered in a View.

/// Per-mount trust tag. Every View created by the host carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewTrust {
    /// The user's own hand-written overlay in their workspace.
    /// Scoped FS is still applied (defense-in-depth), but URL-scheme
    /// filtering is relaxed and network loads are permitted.
    LocalAuthored,
    /// A downloaded bundle installed from the explorer.
    /// Full sandbox: scoped FS + non-`file://` scheme rejection.
    BundleInstalled,
    /// Rendering a bundle specifically to produce a thumbnail.
    /// Same sandbox as `BundleInstalled`.
    ThumbnailGen,
}

impl ViewTrust {
    /// True when the trust level requires URL-scheme filtering and
    /// network-subsystem rejection.
    pub fn is_sandboxed(self) -> bool {
        matches!(self, ViewTrust::BundleInstalled | ViewTrust::ThumbnailGen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_authored_not_sandboxed() {
        assert!(!ViewTrust::LocalAuthored.is_sandboxed());
    }

    #[test]
    fn bundle_and_thumbnail_sandboxed() {
        assert!(ViewTrust::BundleInstalled.is_sandboxed());
        assert!(ViewTrust::ThumbnailGen.is_sandboxed());
    }
}
