//! Process-global filesystem dispatcher registered with Ultralight.
//!
//! Ultralight only permits one `ULFileSystem` vtable per process. The
//! dispatcher holds an `RwLock<Option<OverlayFilesystem>>` representing
//! the *currently mounted* overlay; `install_with_resources()` registers
//! the C vtable once and `set_active()` swaps the inner FS.
//!
//! Both the active overlay and the resources-dir fallback are served by
//! `OverlayFilesystem` instances — same sandboxing logic, just different
//! roots. The resources-dir instance is constructed via
//! `OverlayFilesystem::new_resources_root(...)` which disables the
//! `MAX_PATH_DEPTH` limit because Ultralight's internal assets can be
//! nested more than two directories deep.

use std::ffi::CString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock, RwLock};

use tracing::{debug, warn};
use ultralight_sys as ul;

use super::overlay_fs::{OverlayFilesystem, ResolveError};

#[allow(dead_code)]
static INSTALL_ONCE: Once = Once::new();
static ACTIVE: RwLock<Option<OverlayFilesystem>> = RwLock::new(None);
static RESOURCES_FS: OnceLock<OverlayFilesystem> = OnceLock::new();

/// Install the custom FS vtable and set the resources fallback root.
/// Safe to call multiple times; only the first call registers the vtable
/// with Ultralight (subsequent calls ignore the resources arg).
/// Must be called instead of `ulEnablePlatformFileSystem`.
#[allow(dead_code)]
pub fn install_with_resources(resources_dir: PathBuf) {
    if RESOURCES_FS
        .set(OverlayFilesystem::new_resources_root(resources_dir.clone()))
        .is_err()
    {
        warn!(
            path = %resources_dir.display(),
            "fs_dispatcher: install_with_resources called a second time; ignoring new path"
        );
    }
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
    RESOURCES_FS.get()?.resolve(req).ok()
}

// ── C-ABI trampolines ────────────────────────────────────────────────
//
// The trampolines and their helpers are only reached through the
// Ultralight vtable registered inside `INSTALL_ONCE.call_once`. Under
// `cargo test` no test invokes `install_with_resources`, so rustc flags
// them as dead. They are load-bearing in production.
#[allow(dead_code)]
unsafe extern "C" fn cb_file_exists(path: ul::ULString) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
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
    }))
    .unwrap_or(false)
}

#[allow(dead_code)]
unsafe extern "C" fn cb_file_mime_type(path: ul::ULString) -> ul::ULString {
    catch_unwind(AssertUnwindSafe(|| {
        let req = ul_string_to_string(path);
        let p = Path::new(&req);
        let mime = OverlayFilesystem::mime_type(p);
        string_to_ul(mime)
    }))
    .unwrap_or_else(|_| string_to_ul("application/octet-stream"))
}

#[allow(dead_code)]
unsafe extern "C" fn cb_file_charset(_path: ul::ULString) -> ul::ULString {
    catch_unwind(AssertUnwindSafe(|| string_to_ul("utf-8")))
        .unwrap_or_else(|_| string_to_ul("utf-8"))
}

#[allow(dead_code)]
unsafe extern "C" fn cb_open_file(path: ul::ULString) -> ul::ULBuffer {
    catch_unwind(AssertUnwindSafe(|| {
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
    }))
    .unwrap_or(std::ptr::null_mut())
}

// ── helpers ──────────────────────────────────────────────────────────

#[allow(dead_code)]
unsafe fn ul_string_to_string(s: ul::ULString) -> String {
    if s.is_null() { return String::new(); }
    let data = ul::ulStringGetData(s);
    let len = ul::ulStringGetLength(s);
    if data.is_null() || len == 0 { return String::new(); }
    let slice = std::slice::from_raw_parts(data as *const u8, len);
    String::from_utf8_lossy(slice).into_owned()
}

#[allow(dead_code)]
unsafe fn string_to_ul(s: &str) -> ul::ULString {
    match CString::new(s) {
        Ok(c) => ul::ulCreateString(c.as_ptr()),
        Err(_) => {
            ul::ulCreateString(c"".as_ptr())
        }
    }
}

#[allow(dead_code)]
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
    fn resolve_in_resources_without_install_returns_none() {
        // Without install_with_resources, RESOURCES_FS is unset, so the
        // fallback must simply return None (no panic, no lookup).
        // This test is order-sensitive only if install_with_resources has
        // been called earlier in the same test binary; in that case the
        // resolver will reject traversal via the unified policy.
        let result = resolve_in_resources("../etc/passwd");
        assert!(result.is_none());
        let result = resolve_in_resources("/etc/passwd");
        assert!(result.is_none());
    }
}
