use crate::error::{BundleError, UnsafeKind};
use crate::{MAX_PATH_DEPTH, MAX_PATH_LENGTH};

/// Universal path-safety validator. Rejects traversal, non-ASCII, null bytes,
/// absolute paths, excessive depth, and ustar header-incompatible lengths.
/// Does NOT validate directory placement or file extensions — those rules
/// belong to omni-sanitize per retro-005 D5.
pub(crate) fn validate_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty() {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: "empty path".into(),
        });
    }
    if path.len() > MAX_PATH_LENGTH {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::PathTooLong,
            detail: format!("{} bytes > {}", path.len(), MAX_PATH_LENGTH),
        });
    }
    if path.contains('\0') {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: "null byte".into(),
        });
    }
    if !path.is_ascii() {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::NonAscii,
            detail: format!("non-ascii path ({} bytes)", path.len()),
        });
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: format!("absolute: {path}"),
        });
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
    if components.len() > MAX_PATH_DEPTH {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::PathTooDeep,
            detail: format!("depth {}", components.len()),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_paths() {
        assert!(validate_path("manifest.json").is_ok());
        assert!(validate_path("overlay.omni").is_ok());
        assert!(validate_path("themes/default.css").is_ok());
        assert!(validate_path("fonts/x.ttf").is_ok());
        assert!(validate_path("sounds/beep.ogg").is_ok()); // NEW: no placement rules
        assert!(validate_path("shaders/blur.glsl").is_ok()); // NEW: no placement rules
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(matches!(
            validate_path("../etc/passwd"),
            Err(BundleError::Unsafe { kind: UnsafeKind::Path, .. })
        ));
    }

    #[test]
    fn rejects_absolute() {
        assert!(matches!(
            validate_path("/etc/passwd"),
            Err(BundleError::Unsafe { kind: UnsafeKind::Path, .. })
        ));
        assert!(matches!(
            validate_path("\\windows"),
            Err(BundleError::Unsafe { kind: UnsafeKind::Path, .. })
        ));
    }

    #[test]
    fn rejects_non_ascii_and_null() {
        assert!(matches!(
            validate_path("themes/café.css"),
            Err(BundleError::Unsafe { kind: UnsafeKind::NonAscii, .. })
        ));
        assert!(matches!(
            validate_path("themes/a\0b.css"),
            Err(BundleError::Unsafe { kind: UnsafeKind::Path, .. })
        ));
    }

    #[test]
    fn rejects_too_deep() {
        assert!(matches!(
            validate_path("a/b/c.css"),
            Err(BundleError::Unsafe { kind: UnsafeKind::PathTooDeep, .. })
        ));
    }

    #[test]
    fn rejects_too_long() {
        let long = format!("themes/{}.css", "a".repeat(90));
        assert!(long.len() > MAX_PATH_LENGTH);
        assert!(matches!(
            validate_path(&long),
            Err(BundleError::Unsafe { kind: UnsafeKind::PathTooLong, .. })
        ));
    }
}
