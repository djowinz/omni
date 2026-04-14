use crate::error::{BundleError, UnsafeKind};
use crate::{MAX_CSS, MAX_FONT, MAX_IMAGE_REENCODED, MAX_OVERLAY, MAX_PATH_DEPTH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Manifest,
    Overlay,
    Css,
    Font,
    Image,
}

impl FileKind {
    pub(crate) fn max_size(self) -> u64 {
        match self {
            FileKind::Manifest => MAX_OVERLAY,
            FileKind::Overlay => MAX_OVERLAY,
            FileKind::Css => MAX_CSS,
            FileKind::Font => MAX_FONT,
            FileKind::Image => MAX_IMAGE_REENCODED,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FileKind::Manifest => "manifest",
            FileKind::Overlay => "overlay",
            FileKind::Css => "css",
            FileKind::Font => "font",
            FileKind::Image => "image",
        }
    }
}

/// Validate an intra-bundle path and infer the file kind for size-cap checks.
pub(crate) fn validate_path(path: &str) -> Result<FileKind, BundleError> {
    if path.is_empty() {
        return Err(BundleError::Unsafe { kind: UnsafeKind::Path, detail: "empty".into() });
    }
    // ustar simple-header limit; canonical_hash relies on this.
    if path.len() > 100 {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::PathTooLong,
            detail: format!("path too long: {} bytes", path.len()),
        });
    }
    if path.contains('\0') {
        return Err(BundleError::Unsafe { kind: UnsafeKind::Path, detail: "null byte".into() });
    }
    if !path.is_ascii() {
        return Err(BundleError::Unsafe { kind: UnsafeKind::NonAscii, detail: path.into() });
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(BundleError::Unsafe { kind: UnsafeKind::Path, detail: "absolute".into() });
    }
    for seg in path.split(['/', '\\']) {
        if seg == ".." {
            return Err(BundleError::Unsafe {
                kind: UnsafeKind::Path,
                detail: "parent traversal".into(),
            });
        }
        if seg == "." {
            return Err(BundleError::Unsafe {
                kind: UnsafeKind::Path,
                detail: "current-dir segment".into(),
            });
        }
        if seg.is_empty() {
            return Err(BundleError::Unsafe {
                kind: UnsafeKind::Path,
                detail: "empty segment".into(),
            });
        }
    }
    let components: Vec<&str> = path.split(['/', '\\']).collect();
    if components.len() > MAX_PATH_DEPTH + 1 {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::PathTooDeep,
            detail: format!("depth {}", components.len()),
        });
    }
    validate_placement(&components)
}

fn validate_placement(components: &[&str]) -> Result<FileKind, BundleError> {
    match components {
        ["manifest.json"] => Ok(FileKind::Manifest),
        ["overlay.omni"] => Ok(FileKind::Overlay),
        [dir, file] => match *dir {
            "themes" if file.ends_with(".css") => Ok(FileKind::Css),
            "fonts"
                if file.ends_with(".ttf") || file.ends_with(".otf") || file.ends_with(".woff2") =>
            {
                Ok(FileKind::Font)
            }
            "images"
                if file.ends_with(".png")
                    || file.ends_with(".jpg")
                    || file.ends_with(".jpeg")
                    || file.ends_with(".webp") =>
            {
                Ok(FileKind::Image)
            }
            _ => Err(BundleError::Unsafe {
                kind: UnsafeKind::Path,
                detail: format!("{dir}/{file}"),
            }),
        },
        _ => Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: components.join("/"),
        }),
    }
}

pub(crate) fn check_size(kind: FileKind, actual: u64) -> Result<(), BundleError> {
    let limit = kind.max_size();
    if actual > limit {
        Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!("{}={actual} > {limit}", kind.as_str()),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::UnsafeKind;
    use crate::MAX_CSS;

    #[test]
    fn accepts_manifest_and_overlay_at_root() {
        assert_eq!(validate_path("manifest.json").unwrap(), FileKind::Manifest);
        assert_eq!(validate_path("overlay.omni").unwrap(), FileKind::Overlay);
    }

    #[test]
    fn accepts_themes_fonts_images() {
        assert_eq!(validate_path("themes/default.css").unwrap(), FileKind::Css);
        assert_eq!(validate_path("fonts/x.ttf").unwrap(), FileKind::Font);
        assert_eq!(validate_path("fonts/x.otf").unwrap(), FileKind::Font);
        assert_eq!(validate_path("fonts/x.woff2").unwrap(), FileKind::Font);
        assert_eq!(validate_path("images/x.png").unwrap(), FileKind::Image);
        assert_eq!(validate_path("images/x.jpg").unwrap(), FileKind::Image);
        assert_eq!(validate_path("images/x.jpeg").unwrap(), FileKind::Image);
        assert_eq!(validate_path("images/x.webp").unwrap(), FileKind::Image);
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(matches!(
            validate_path("../etc/passwd"),
            Err(BundleError::Unsafe { kind: UnsafeKind::Path, .. })
        ));
        assert!(matches!(
            validate_path("themes/../x.css"),
            Err(BundleError::Unsafe { kind: UnsafeKind::Path, .. })
        ));
    }

    #[test]
    fn rejects_absolute_paths() {
        assert!(validate_path("/etc/passwd").is_err());
        assert!(validate_path("\\windows").is_err());
    }

    #[test]
    fn rejects_null_byte_and_non_ascii() {
        assert!(validate_path("themes/a\0b.css").is_err());
        assert!(validate_path("themes/café.css").is_err());
    }

    #[test]
    fn rejects_over_max_depth() {
        assert!(validate_path("a/b/c.css").is_err());
    }

    #[test]
    fn rejects_unknown_directory() {
        assert!(validate_path("scripts/evil.js").is_err());
    }

    #[test]
    fn rejects_unknown_extension_in_known_dir() {
        assert!(validate_path("themes/x.js").is_err());
        assert!(validate_path("fonts/x.bin").is_err());
        assert!(validate_path("images/x.bmp").is_err());
    }

    #[test]
    fn rejects_empty_segments() {
        assert!(validate_path("").is_err());
        assert!(validate_path("themes//x.css").is_err());
        assert!(validate_path("./themes/x.css").is_err());
    }

    #[test]
    fn rejects_paths_longer_than_ustar_limit() {
        // 101-byte path passes depth/ASCII/segment checks but exceeds ustar header.
        let long = format!("themes/{}.css", "a".repeat(90));
        assert!(long.len() > 100);
        assert!(matches!(
            validate_path(&long),
            Err(BundleError::Unsafe { kind: UnsafeKind::PathTooLong, .. })
        ));
    }

    #[test]
    fn check_size_respects_cap() {
        assert!(check_size(FileKind::Css, 10).is_ok());
        let err = check_size(FileKind::Css, MAX_CSS + 1).unwrap_err();
        assert!(matches!(err, BundleError::Unsafe { kind: UnsafeKind::SizeExceeded, .. }));
    }
}
