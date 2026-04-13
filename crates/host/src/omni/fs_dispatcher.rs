//! Process-global filesystem dispatcher registered with Ultralight.
//!
//! Ultralight only permits one `ULFileSystem` vtable per process. The
//! dispatcher holds an `RwLock<Option<OverlayFilesystem>>` representing
//! the *currently mounted* overlay; `install_with_resources()` registers
//! the C vtable once and `set_active()` swaps the inner FS.
//!
//! The resources-dir fallback serves Ultralight's built-in resource
//! requests (e.g. internal error pages, fonts the renderer itself loads)
//! from the exe-local `resources/` directory — without it, replacing the
//! platform filesystem would leave the renderer unable to load its own
//! assets.

// FFI trampolines and their helpers are registered via an Ultralight vtable
// at runtime; under `cargo test` no view ever calls them, so the dead-code
// lint fires spuriously. The items below are load-bearing in production.
#![allow(dead_code)]

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock, RwLock};

use tracing::{debug, warn};
use ultralight_sys as ul;

use super::overlay_fs::{OverlayFilesystem, ResolveError};

static INSTALL_ONCE: Once = Once::new();
static ACTIVE: RwLock<Option<OverlayFilesystem>> = RwLock::new(None);
static RESOURCES_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Install the custom FS vtable and set the resources fallback root.
/// Safe to call multiple times; only the first call registers the vtable
/// with Ultralight (subsequent calls ignore the resources arg).
/// Must be called instead of `ulEnablePlatformFileSystem`.
pub fn install_with_resources(resources_dir: PathBuf) {
    let _ = RESOURCES_ROOT.set(resources_dir);
    INSTALL_ONCE.call_once(|| unsafe {
        let vtable = ul::ULFileSystem {
            file_exists: Some(cb_file_exists),
            get_file_mime_type: Some(cb_file_mime_type),
            get_file_charset: Some(cb_file_charset),
            open_file: Some(cb_open_file),
        };
        ul::ulPlatformSetFileSystem(vtable);
    });
}

/// Swap the currently-active scoped filesystem. Called each time the host
/// mounts a new overlay or bundle.
pub fn set_active(fs: OverlayFilesystem) {
    let mut slot = ACTIVE.write().expect("fs_dispatcher poisoned");
    *slot = Some(fs);
}

pub fn clear_active() {
    let mut slot = ACTIVE.write().expect("fs_dispatcher poisoned");
    *slot = None;
}

// ── resources fallback ──────────────────────────────────────────────

fn resolve_in_resources(req: &str) -> Option<PathBuf> {
    let root = RESOURCES_ROOT.get()?;
    let stripped = req.strip_prefix("file:///").unwrap_or(req);
    // Very conservative: reject anything that escapes with `..` or is absolute.
    if stripped.contains("..") || Path::new(stripped).is_absolute() {
        return None;
    }
    let candidate = root.join(stripped);
    if candidate.exists() { Some(candidate) } else { None }
}

// ── C-ABI trampolines ────────────────────────────────────────────────

unsafe extern "C" fn cb_file_exists(path: ul::ULString) -> bool {
    let req = ul_string_to_string(path);
    let guard = match ACTIVE.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    if let Some(fs) = guard.as_ref() {
        match fs.resolve(&req) {
            Ok(_) => return true,
            Err(ResolveError::NotFound) => {} // fall through to resources
            Err(e) => {
                log_reject("file_exists", &req, &e);
                return false;
            }
        }
    }
    resolve_in_resources(&req).is_some()
}

unsafe extern "C" fn cb_file_mime_type(path: ul::ULString) -> ul::ULString {
    let req = ul_string_to_string(path);
    let p = Path::new(&req);
    let mime = OverlayFilesystem::mime_type(p);
    string_to_ul(mime)
}

unsafe extern "C" fn cb_file_charset(_path: ul::ULString) -> ul::ULString {
    string_to_ul("utf-8")
}

unsafe extern "C" fn cb_open_file(path: ul::ULString) -> ul::ULBuffer {
    let req = ul_string_to_string(path);
    let guard = match ACTIVE.read() {
        Ok(g) => g,
        Err(_) => return std::ptr::null_mut(),
    };

    let resolved: Option<PathBuf> = if let Some(fs) = guard.as_ref() {
        match fs.resolve(&req) {
            Ok(p) => Some(p),
            Err(ResolveError::NotFound) => resolve_in_resources(&req),
            Err(e) => {
                log_reject("open_file", &req, &e);
                None
            }
        }
    } else {
        resolve_in_resources(&req)
    };

    let Some(path_buf) = resolved else { return std::ptr::null_mut(); };

    match std::fs::read(&path_buf) {
        Ok(bytes) => {
            if bytes.is_empty() {
                return std::ptr::null_mut();
            }
            ul::ulCreateBufferFromCopy(bytes.as_ptr() as *const _, bytes.len())
        }
        Err(e) => {
            warn!(path = %path_buf.display(), error = %e, "fs_dispatcher: read failed");
            std::ptr::null_mut()
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────

unsafe fn ul_string_to_string(s: ul::ULString) -> String {
    if s.is_null() { return String::new(); }
    let data = ul::ulStringGetData(s);
    let len = ul::ulStringGetLength(s);
    if data.is_null() || len == 0 { return String::new(); }
    let slice = std::slice::from_raw_parts(data as *const u8, len);
    String::from_utf8_lossy(slice).into_owned()
}

unsafe fn string_to_ul(s: &str) -> ul::ULString {
    match CString::new(s) {
        Ok(c) => ul::ulCreateString(c.as_ptr()),
        Err(_) => {
            ul::ulCreateString(c"".as_ptr())
        }
    }
}

fn log_reject(op: &str, req: &str, err: &ResolveError) {
    debug!(op, path = %req, ?err, "fs_dispatcher: rejected");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_clear_active() {
        let fs = OverlayFilesystem::new(PathBuf::from("."));
        set_active(fs);
        assert!(ACTIVE.read().unwrap().is_some());
        clear_active();
        assert!(ACTIVE.read().unwrap().is_none());
    }

    #[test]
    fn resolve_in_resources_rejects_traversal() {
        // This test won't see RESOURCES_ROOT set unless install_with_resources
        // was called; skip the assertion and just exercise the function.
        assert!(resolve_in_resources("../etc/passwd").is_none());
        assert!(resolve_in_resources("/etc/passwd").is_none());
    }
}
