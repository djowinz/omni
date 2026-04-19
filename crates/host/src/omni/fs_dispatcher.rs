//! Process-global filesystem dispatcher registered with Ultralight.
//!
//! Ultralight only permits one `ULFileSystem` vtable per process. The
//! dispatcher maintains a routing table of `OverlayFilesystem` mounts keyed
//! by a monotonic `u64` mount ID. Each `UlRenderer::mount` registers an
//! entry via [`register_mount`] and receives a [`MountHandle`]; dropping
//! the handle removes the entry.
//!
//! Requested URL paths arrive with a `mount-{id}/` prefix (e.g.
//! `mount-42/themes/theme.css`). The C-ABI callbacks split on the first
//! `/`, parse the `{id}`, look up the entry, and resolve the remainder
//! against that mount's `OverlayFilesystem`. Paths that don't carry the
//! prefix (e.g. Ultralight's own `./resources/` assets) fall through to
//! the `RESOURCES_FS` fallback.
//!
//! Per architectural invariant #24: the `MOUNTS` table is ambient mutable
//! state, but it is routing infrastructure rather than a shared object of
//! disagreement. Ultralight's FS callback is a C function pointer with no
//! context parameter — the routing table IS the context, keyed on the URL
//! path that Ultralight DOES supply.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Once, OnceLock, RwLock};

use tracing::{debug, warn};
use ultralight_sys as ul;

use super::overlay_fs::{OverlayFilesystem, ResolveError};
use super::ul_string;

static INSTALL_ONCE: Once = Once::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static MOUNTS: RwLock<Option<HashMap<u64, OverlayFilesystem>>> = RwLock::new(None);
static RESOURCES_FS: OnceLock<OverlayFilesystem> = OnceLock::new();

/// Install the custom FS vtable and set the resources fallback root.
/// Safe to call multiple times; only the first call registers the vtable
/// with Ultralight (subsequent calls ignore the resources arg).
/// Must be called instead of `ulEnablePlatformFileSystem`.
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
    INSTALL_ONCE.call_once(|| {
        {
            let mut slot = MOUNTS.write().expect("fs_dispatcher poisoned");
            if slot.is_none() {
                *slot = Some(HashMap::new());
            }
        }
        unsafe {
            let vtable = ul::ULFileSystem {
                file_exists: Some(cb_file_exists),
                get_file_mime_type: Some(cb_file_mime_type),
                get_file_charset: Some(cb_file_charset),
                open_file: Some(cb_open_file),
            };
            ul::ulPlatformSetFileSystem(vtable);
        }
    });
}

/// RAII handle for a registered mount. The entry stays in `MOUNTS` while
/// this handle is alive; dropping it removes the entry.
pub struct MountHandle {
    id: u64,
}

impl MountHandle {
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for MountHandle {
    fn drop(&mut self) {
        if let Ok(mut slot) = MOUNTS.write() {
            if let Some(map) = slot.as_mut() {
                map.remove(&self.id);
            }
        }
    }
}

/// Register an `OverlayFilesystem` under a freshly-allocated mount ID.
/// Returns an RAII handle whose Drop removes the entry.
pub fn register_mount(fs: OverlayFilesystem) -> MountHandle {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut slot = MOUNTS.write().expect("fs_dispatcher poisoned");
    let map = slot.get_or_insert_with(HashMap::new);
    map.insert(id, fs);
    MountHandle { id }
}

// ── resources fallback ──────────────────────────────────────────────

fn resolve_in_resources(req: &str) -> Option<PathBuf> {
    RESOURCES_FS.get()?.resolve(req).ok()
}

// ── routing ─────────────────────────────────────────────────────────

/// Route a request string to a concrete filesystem path.
///
/// If `req` starts with `mount-<digits>/`, look up the entry and resolve
/// the remainder against it. Otherwise (or if the mount-ID isn't in the
/// map), fall through to `RESOURCES_FS`.
fn route(req: &str) -> Option<PathBuf> {
    if let Some((head, tail)) = req.split_once('/') {
        if let Some(id_str) = head.strip_prefix("mount-") {
            if let Ok(id) = id_str.parse::<u64>() {
                let slot = MOUNTS.read().ok()?;
                if let Some(map) = slot.as_ref() {
                    if let Some(fs) = map.get(&id) {
                        match fs.resolve(tail) {
                            Ok(p) => return Some(p),
                            Err(ResolveError::NotFound) => return resolve_in_resources(tail),
                            Err(e) => {
                                log_reject("route", tail, &e);
                                return None;
                            }
                        }
                    }
                }
            }
        }
    }
    resolve_in_resources(req)
}

// ── C-ABI trampolines ────────────────────────────────────────────────

unsafe extern "C" fn cb_file_exists(path: ul::ULString) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        let req = ul_string::from_ul(path);
        // Leave this debug trace in place through Wave D verification — it
        // confirms Ultralight delivers the `mount-{id}/...` prefix verbatim.
        // Downgrade to `trace!` once stable.
        debug!(req = %req, "fs_dispatcher: cb_file_exists");
        route(&req).is_some()
    }))
    .unwrap_or(false)
}

unsafe extern "C" fn cb_file_mime_type(path: ul::ULString) -> ul::ULString {
    catch_unwind(AssertUnwindSafe(|| {
        let req = ul_string::from_ul(path);
        let p = Path::new(&req);
        let mime = OverlayFilesystem::mime_type(p);
        ul_string::to_ul(mime)
    }))
    .unwrap_or_else(|_| ul_string::to_ul("application/octet-stream"))
}

unsafe extern "C" fn cb_file_charset(_path: ul::ULString) -> ul::ULString {
    catch_unwind(AssertUnwindSafe(|| ul_string::to_ul("utf-8")))
        .unwrap_or_else(|_| ul_string::to_ul("utf-8"))
}

unsafe extern "C" fn cb_open_file(path: ul::ULString) -> ul::ULBuffer {
    catch_unwind(AssertUnwindSafe(|| {
        let req = ul_string::from_ul(path);
        let Some(path_buf) = route(&req) else {
            return std::ptr::null_mut();
        };

        match std::fs::read(&path_buf) {
            Ok(bytes) => {
                // Zero-length buffers are well-defined here: Ultralight's
                // `ulCreateBufferFromCopy` performs no size assertion (see
                // vendor/ultralight/include/CAPI/CAPI_Buffer.h), and
                // `bytes.as_ptr()` on an empty Vec returns a non-null,
                // aligned dangling pointer per the Rust reference — a 0-byte
                // copy is a no-op. This preserves intentionally-empty assets
                // (e.g. empty CSS files) instead of surfacing them as 404s.
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

fn log_reject(op: &str, req: &str, err: &ResolveError) {
    debug!(op, path = %req, ?err, "fs_dispatcher: rejected");
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Serializes the fs_dispatcher tests that share the process-global
    /// `MOUNTS` static. Each test acquires this lock before resetting the
    /// map, ensuring `clear_mounts_for_test` + mount operations don't race
    /// with other tests running in parallel test threads.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Test-only reference to the C-ABI callbacks and helpers so that
    /// `cargo test` builds (which don't invoke `install_with_resources`)
    /// don't flag them as dead. They are load-bearing in production via
    /// the Ultralight vtable registered inside `INSTALL_ONCE.call_once`.
    #[allow(dead_code)]
    fn _keep_alive() {
        let _: unsafe extern "C" fn(ul::ULString) -> bool = cb_file_exists;
        let _: unsafe extern "C" fn(ul::ULString) -> ul::ULString = cb_file_mime_type;
        let _: unsafe extern "C" fn(ul::ULString) -> ul::ULString = cb_file_charset;
        let _: unsafe extern "C" fn(ul::ULString) -> ul::ULBuffer = cb_open_file;
        let _: fn(&str, &str, &ResolveError) = log_reject;
        let _: fn(PathBuf) = install_with_resources;
    }

    /// Reset the shared `MOUNTS` static to an empty map. Must be called
    /// while the caller holds `TEST_LOCK` to prevent races between parallel
    /// test threads.
    fn clear_mounts_for_test() {
        let mut slot = MOUNTS.write().unwrap();
        *slot = Some(HashMap::new());
    }

    #[test]
    fn register_and_drop_round_trip() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_mounts_for_test();
        let fs = OverlayFilesystem::new(PathBuf::from("."));
        let handle = register_mount(fs);
        {
            let slot = MOUNTS.read().unwrap();
            let map = slot.as_ref().unwrap();
            assert_eq!(map.len(), 1);
            assert!(map.contains_key(&handle.id()));
        }
        drop(handle);
        let slot = MOUNTS.read().unwrap();
        let map = slot.as_ref().unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn two_mounts_route_independently() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_mounts_for_test();
        // Two mounts with distinct tempdirs so resolve returns distinct paths.
        let tmp_a = tempfile::tempdir().unwrap();
        let tmp_b = tempfile::tempdir().unwrap();
        std::fs::write(tmp_a.path().join("a.txt"), b"in A").unwrap();
        std::fs::write(tmp_b.path().join("b.txt"), b"in B").unwrap();

        let h_a = register_mount(OverlayFilesystem::new(tmp_a.path().to_path_buf()));
        let h_b = register_mount(OverlayFilesystem::new(tmp_b.path().to_path_buf()));

        let resolved_a = route(&format!("mount-{}/a.txt", h_a.id()));
        let resolved_b = route(&format!("mount-{}/b.txt", h_b.id()));

        // `OverlayFilesystem::resolve` calls `canonicalize()` which on Windows
        // returns UNC-prefixed paths (`\\?\C:\...`). Canonicalize the expected
        // paths too so the comparison is format-stable across platforms.
        let expect_a = tmp_a.path().join("a.txt").canonicalize().unwrap();
        let expect_b = tmp_b.path().join("b.txt").canonicalize().unwrap();
        assert_eq!(resolved_a.as_deref(), Some(expect_a.as_path()));
        assert_eq!(resolved_b.as_deref(), Some(expect_b.as_path()));
    }

    #[test]
    fn unknown_mount_id_falls_through_to_resources() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_mounts_for_test();
        // With no mount registered and no RESOURCES_FS installed (or
        // installed with a path that doesn't contain the request), the
        // lookup returns None.
        let resolved = route("mount-99999/absent.txt");
        assert!(resolved.is_none());
    }

    #[test]
    fn non_mount_prefix_falls_through_to_resources() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_mounts_for_test();
        // A request that doesn't start with `mount-{digits}/` bypasses
        // the routing table entirely and tries RESOURCES_FS.
        let resolved = route("resources/something.dat");
        // Without an installed RESOURCES_FS this returns None; the assertion
        // is that we didn't panic and we didn't look at MOUNTS (which is
        // empty).
        assert!(resolved.is_none());
    }

    #[test]
    fn dropping_all_handles_clears_map() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_mounts_for_test();
        let fs_a = OverlayFilesystem::new(PathBuf::from("."));
        let fs_b = OverlayFilesystem::new(PathBuf::from("."));
        let h_a = register_mount(fs_a);
        let h_b = register_mount(fs_b);
        {
            let slot = MOUNTS.read().unwrap();
            assert_eq!(slot.as_ref().unwrap().len(), 2);
        }
        drop(h_a);
        drop(h_b);
        let slot = MOUNTS.read().unwrap();
        assert!(slot.as_ref().unwrap().is_empty());
    }
}
