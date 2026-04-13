//! Scoped filesystem resolver for a single overlay/bundle root.
//! Pure Rust — no FFI. The C-ABI shim lives in `fs_dispatcher.rs`.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Max directory depth below the overlay root (umbrella §4.3 MAX_PATH_DEPTH).
pub const MAX_PATH_DEPTH: usize = 2;

/// Reasons a path may be rejected. Callers should log these; Ultralight
/// just sees a `file_exists → false`.
#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    Empty,
    NullByte,
    AbsolutePath,
    ParentEscape,
    UnsupportedScheme,
    Symlink,
    DepthExceeded,
    NotFound,
}

pub struct OverlayFilesystem {
    root: PathBuf,
    mode: Mode,
    /// Lazily-computed canonical form of `root`. Cached to avoid an
    /// extra `canonicalize` syscall on every `resolve` call (40-100
    /// calls per overlay mount during a typical UI load).
    canon_root: OnceLock<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Bundle / overlay mount: enforces `MAX_PATH_DEPTH`.
    Overlay,
    /// Ultralight built-in resources: no depth limit (Ultralight's own
    /// `resources/` tree is deeper than a bundle).
    Resources,
}

impl OverlayFilesystem {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            mode: Mode::Overlay,
            canon_root: OnceLock::new(),
        }
    }

    /// Construct the resources-dir fallback instance. Same sandboxing
    /// policy as `new`, but with `MAX_PATH_DEPTH` disabled because
    /// Ultralight's built-in `resources/` tree can be nested deeper
    /// than a user bundle's two-level `images/icons/x.png` layout.
    pub fn new_resources_root(root: PathBuf) -> Self {
        Self {
            root,
            mode: Mode::Resources,
            canon_root: OnceLock::new(),
        }
    }

    /// Request strings must be raw paths, not URL-encoded; `%`-containing
    /// inputs are rejected outright rather than decoded.
    pub fn resolve(&self, req: &str) -> Result<PathBuf, ResolveError> {
        if req.is_empty() {
            return Err(ResolveError::Empty);
        }
        if req.contains('\0') {
            return Err(ResolveError::NullByte);
        }

        let stripped = strip_file_scheme(req);

        // Windows drive letters (e.g. "C:/...", "C:\\...") must be classified
        // as AbsolutePath, not as a URL scheme.
        if has_drive_letter(stripped) {
            return Err(ResolveError::AbsolutePath);
        }

        // Reject percent-encoded inputs (e.g. `file:///%2e%2e/etc/passwd`).
        // Simpler and safer than decoding.
        if stripped.contains('%') {
            return Err(ResolveError::UnsupportedScheme);
        }

        if has_non_file_scheme(stripped) {
            return Err(ResolveError::UnsupportedScheme);
        }

        let p = Path::new(stripped);

        let components: Vec<Component> = p.components().collect();
        let mut depth: usize = 0;
        for (i, comp) in components.iter().enumerate() {
            match comp {
                Component::Normal(_) => {
                    // Don't count the last component (it's the filename).
                    if i + 1 < components.len() {
                        depth += 1;
                    }
                }
                Component::CurDir => {}
                Component::ParentDir => return Err(ResolveError::ParentEscape),
                Component::RootDir | Component::Prefix(_) => {
                    return Err(ResolveError::AbsolutePath)
                }
            }
        }
        if self.mode == Mode::Overlay && depth > MAX_PATH_DEPTH {
            return Err(ResolveError::DepthExceeded);
        }

        let joined = self.root.join(p);
        let canon = joined.canonicalize().map_err(|_| ResolveError::NotFound)?;
        // Cache the canonical root across calls. If canonicalize fails
        // (e.g. the root doesn't exist yet), fall back to the raw root —
        // the subsequent `starts_with` check still rejects escapes because
        // `canon` is absolute while a non-canonicalized relative root
        // would never be a prefix of it.
        let canon_root = self.canon_root.get_or_init(|| {
            self.root
                .canonicalize()
                .unwrap_or_else(|_| self.root.clone())
        });
        if !canon.starts_with(canon_root) {
            return Err(ResolveError::Symlink);
        }
        Ok(canon)
    }

    pub fn mime_type(path: &Path) -> &'static str {
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => return "application/octet-stream",
        };
        if ext.eq_ignore_ascii_case("ttf") {
            return "font/ttf";
        }
        if ext.eq_ignore_ascii_case("otf") {
            return "font/otf";
        }
        if ext.eq_ignore_ascii_case("woff") {
            return "font/woff";
        }
        if ext.eq_ignore_ascii_case("woff2") {
            return "font/woff2";
        }
        if ext.eq_ignore_ascii_case("png") {
            return "image/png";
        }
        if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
            return "image/jpeg";
        }
        if ext.eq_ignore_ascii_case("webp") {
            return "image/webp";
        }
        if ext.eq_ignore_ascii_case("gif") {
            return "image/gif";
        }
        if ext.eq_ignore_ascii_case("svg") {
            return "image/svg+xml";
        }
        if ext.eq_ignore_ascii_case("css") {
            return "text/css";
        }
        if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
            return "text/html";
        }
        if ext.eq_ignore_ascii_case("js") {
            return "application/javascript";
        }
        if ext.eq_ignore_ascii_case("omni") {
            return "application/xml";
        }
        "application/octet-stream"
    }
}

fn strip_file_scheme(s: &str) -> &str {
    s.strip_prefix("file:///")
        .or_else(|| s.strip_prefix("file://"))
        .unwrap_or(s)
}

fn has_non_file_scheme(s: &str) -> bool {
    if let Some(colon) = s.find(':') {
        let scheme = &s[..colon];
        if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()) {
            return scheme != "file";
        }
    }
    false
}

fn has_drive_letter(s: &str) -> bool {
    let s = s.strip_prefix('/').unwrap_or(s);
    let mut ch = s.chars();
    matches!((ch.next(), ch.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root() -> PathBuf {
        let id = std::process::id();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("omni_ofs_{id}_{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(p: &Path, bytes: &[u8]) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, bytes).unwrap();
    }

    #[test]
    fn relative_within_root_resolves() {
        let root = temp_root();
        write(&root.join("fonts/SpaceMono.ttf"), b"ttf");
        let fs = OverlayFilesystem::new(root.clone());
        assert!(fs.resolve("fonts/SpaceMono.ttf").is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parent_traversal_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(fs.resolve("../secret.png"), Err(ResolveError::ParentEscape));
        assert_eq!(
            fs.resolve("fonts/../../secret"),
            Err(ResolveError::ParentEscape)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn absolute_unix_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(fs.resolve("/etc/passwd"), Err(ResolveError::AbsolutePath));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn absolute_windows_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(
            fs.resolve("C:/Windows/System32/cmd.exe"),
            Err(ResolveError::AbsolutePath)
        );
        assert_eq!(
            fs.resolve("C:\\Windows\\System32\\cmd.exe"),
            Err(ResolveError::AbsolutePath)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn file_scheme_stripped_then_resolved() {
        let root = temp_root();
        write(&root.join("images/logo.png"), b"png");
        let fs = OverlayFilesystem::new(root.clone());
        assert!(fs.resolve("file:///images/logo.png").is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn http_scheme_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(
            fs.resolve("http://evil.com/x.png"),
            Err(ResolveError::UnsupportedScheme)
        );
        assert_eq!(
            fs.resolve("https://evil.com/x.png"),
            Err(ResolveError::UnsupportedScheme)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn null_byte_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root);
        assert_eq!(fs.resolve("fonts/\0.ttf"), Err(ResolveError::NullByte));
    }

    #[test]
    fn depth_2_allowed() {
        let root = temp_root();
        write(&root.join("images/icons/cpu.png"), b"png");
        let fs = OverlayFilesystem::new(root.clone());
        assert!(fs.resolve("images/icons/cpu.png").is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn allow_deep_permits_nested_resources() {
        let root = temp_root();
        write(&root.join("a/b/c/d/e.png"), b"x");
        let fs = OverlayFilesystem::new_resources_root(root.clone());
        assert!(fs.resolve("a/b/c/d/e.png").is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn depth_3_rejected() {
        let root = temp_root();
        write(&root.join("a/b/c/d.png"), b"png");
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(fs.resolve("a/b/c/d.png"), Err(ResolveError::DepthExceeded));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    #[cfg(unix)]
    fn symlink_escape_rejected() {
        use std::os::unix::fs::symlink;
        let root = temp_root();
        let outside = temp_root();
        write(&outside.join("secret"), b"s");
        symlink(outside.join("secret"), root.join("link")).unwrap();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(fs.resolve("link"), Err(ResolveError::Symlink));
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[test]
    #[cfg(windows)]
    fn unc_verbatim_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(
            fs.resolve(r"\\?\C:\Windows\System32\cmd.exe"),
            Err(ResolveError::AbsolutePath)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    #[cfg(windows)]
    fn unc_server_share_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(
            fs.resolve(r"\\server\share\secret"),
            Err(ResolveError::AbsolutePath)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn percent_encoded_rejected() {
        let root = temp_root();
        let fs = OverlayFilesystem::new(root.clone());
        assert_eq!(
            fs.resolve("%2e%2e/secret"),
            Err(ResolveError::UnsupportedScheme)
        );
        assert_eq!(
            fs.resolve("file:///%2e%2e/etc/passwd"),
            Err(ResolveError::UnsupportedScheme)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn curdir_segments_allowed() {
        let root = temp_root();
        write(&root.join("fonts/x.ttf"), b"");
        let fs = OverlayFilesystem::new(root.clone());
        assert!(fs.resolve("./fonts/x.ttf").is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mime_types() {
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.ttf")),
            "font/ttf"
        );
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.woff2")),
            "font/woff2"
        );
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.png")),
            "image/png"
        );
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.JPG")),
            "image/jpeg"
        );
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.css")),
            "text/css"
        );
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.omni")),
            "application/xml"
        );
        assert_eq!(
            OverlayFilesystem::mime_type(Path::new("a/b.xyz")),
            "application/octet-stream"
        );
    }
}
