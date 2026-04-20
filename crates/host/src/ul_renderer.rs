//! Safe wrapper around Ultralight C API for headless overlay rendering.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::omni::fs_dispatcher;
use crate::omni::overlay_fs::OverlayFilesystem;
use crate::omni::trust_filter;
use crate::omni::view_trust::ViewTrust;

/// Filename of the scratch HTML written into the overlay root on mount.
const SCRATCH_NAME: &str = ".omni_current.html";

/// Raw BGRA pixel buffer produced by a thumbnail render request.
pub struct ThumbnailPixels {
    pub width: u32,
    pub height: u32,
    pub row_bytes: u32,
    /// Tightly-packed BGRA (row-stride-stripped) — `width * 4 * height` bytes.
    pub bgra: Vec<u8>,
}

/// A request dispatched to the main render thread to capture a thumbnail.
/// The thumbnail pipeline (`crate::share::thumbnail`) sends this through
/// [`get_thumbnail_channel`]; the live render loop in `main.rs` drains the
/// receiver between ticks and services each request synchronously on the
/// main thread (Ultralight has process-global state that is not safe to
/// drive from multiple Renderer instances — see `docs/superpowers/specs/
/// 2026-04-19-ultralight-thumbnail-fix-design.md`).
pub struct ThumbnailRequest {
    pub overlay_root: PathBuf,
    pub html: String,
    pub sample_values: HashMap<String, f64>,
    pub reply: oneshot::Sender<Result<ThumbnailPixels, String>>,
}

static THUMBNAIL_CHANNEL: OnceLock<mpsc::UnboundedSender<ThumbnailRequest>> = OnceLock::new();

/// Install the process-wide thumbnail request channel. Called from
/// `main.rs` once at host startup; subsequent calls are ignored.
pub fn install_thumbnail_channel(sender: mpsc::UnboundedSender<ThumbnailRequest>) {
    let was_set = THUMBNAIL_CHANNEL.set(sender).is_err();
    if was_set {
        tracing::warn!("install_thumbnail_channel called twice; ignoring second call");
    } else {
        tracing::info!("install_thumbnail_channel: thumbnail render channel installed");
    }
}

/// Fetch the thumbnail request sender. Returns `None` if `main.rs` has
/// not yet installed a channel (integration-test context without a live
/// renderer, or a startup-order bug).
pub fn get_thumbnail_channel() -> Option<mpsc::UnboundedSender<ThumbnailRequest>> {
    THUMBNAIL_CHANNEL.get().cloned()
}

/// Params saved from the most recent `mount()` so `render_thumbnail_to_png`
/// can restore the live view after capturing a thumbnail frame.
#[derive(Clone)]
struct MountState {
    overlay_root: PathBuf,
    html: String,
    trust: ViewTrust,
}

/// Safe wrapper around Ultralight renderer + view.
pub struct UlRenderer {
    renderer: ultralight_sys::ULRenderer,
    view: ultralight_sys::ULView,
    config: ultralight_sys::ULConfig,
    view_config: ultralight_sys::ULViewConfig,
    width: u32,
    height: u32,
    last_scratch_dir: Mutex<Option<PathBuf>>,
    /// Current mount registration in `fs_dispatcher`. Populated by
    /// `mount()`; replaced on subsequent mounts; dropped in the explicit
    /// `Drop` impl BEFORE `ulDestroyView` so no in-flight Ultralight
    /// callback reads a mount whose overlay_root has been cleaned up.
    mount_handle: Mutex<Option<fs_dispatcher::MountHandle>>,
    /// Saved mount params (root + HTML + trust) from the most recent
    /// `mount()`. `render_thumbnail_to_png` uses these to restore the
    /// live view after capturing a thumbnail frame.
    last_mount_state: Mutex<Option<MountState>>,
}

impl UlRenderer {
    /// Initialize Ultralight with the given viewport dimensions.
    pub fn init(width: u32, height: u32, resources_dir: &Path) -> Result<Self, String> {
        unsafe {
            // Set up platform handlers (from AppCore)
            ultralight_sys::ulEnablePlatformFontLoader();

            fs_dispatcher::install_with_resources(resources_dir.to_path_buf());

            // Configure
            let config = ultralight_sys::ulCreateConfig();
            let res_prefix = CString::new("./resources/").unwrap();
            let res_prefix_str = ultralight_sys::ulCreateString(res_prefix.as_ptr());
            ultralight_sys::ulConfigSetResourcePathPrefix(config, res_prefix_str);
            ultralight_sys::ulDestroyString(res_prefix_str);

            // Create renderer
            let renderer = ultralight_sys::ulCreateRenderer(config);
            if renderer.is_null() {
                ultralight_sys::ulDestroyConfig(config);
                return Err("Failed to create Ultralight renderer".into());
            }

            // Create view (transparent, CPU renderer)
            let view_config = ultralight_sys::ulCreateViewConfig();
            ultralight_sys::ulViewConfigSetIsAccelerated(view_config, false);
            ultralight_sys::ulViewConfigSetIsTransparent(view_config, true);

            let view = ultralight_sys::ulCreateView(
                renderer,
                width,
                height,
                view_config,
                std::ptr::null_mut(), // default session
            );
            if view.is_null() {
                ultralight_sys::ulDestroyViewConfig(view_config);
                ultralight_sys::ulDestroyRenderer(renderer);
                ultralight_sys::ulDestroyConfig(config);
                return Err("Failed to create Ultralight view".into());
            }

            info!(width, height, "Ultralight renderer initialized");

            Ok(Self {
                renderer,
                view,
                config,
                view_config,
                width,
                height,
                last_scratch_dir: Mutex::new(None),
                mount_handle: Mutex::new(None),
                last_mount_state: Mutex::new(None),
            })
        }
    }

    /// Mount `overlay_root` in the filesystem dispatcher, write a scratch
    /// HTML at `overlay_root/.omni_current.html`, apply the trust filter,
    /// and load the scratch via a `file:///mount-{id}/.omni_current.html`
    /// URL so the dispatcher can route requests for this renderer's assets
    /// without colliding with any other `UlRenderer` instance in the same
    /// process.
    ///
    /// A previous mount on this same `UlRenderer` is released (its entry
    /// removed from the dispatcher) before the new one is registered.
    pub fn mount(&self, overlay_root: &Path, html: &str, trust: ViewTrust) -> Result<(), String> {
        self.mount_internal(overlay_root, html, trust, /* save_state = */ true)
    }

    /// Internal mount with an option to skip saving state for restoration.
    /// `render_thumbnail_to_png` uses `save_state = false` for the transient
    /// thumbnail mount so the restoration target is preserved.
    fn mount_internal(
        &self,
        overlay_root: &Path,
        html: &str,
        trust: ViewTrust,
        save_state: bool,
    ) -> Result<(), String> {
        // Register the new mount FIRST. The returned handle replaces any
        // previous one inside `mount_handle`; the previous handle's Drop
        // removes its dispatcher entry. Holding both briefly is harmless —
        // the URL we load references the NEW id, so no callback races to
        // the old entry before it is removed.
        let fs = OverlayFilesystem::new(overlay_root.to_path_buf());
        let handle = fs_dispatcher::register_mount(fs);
        let mount_id = handle.id();
        {
            let mut slot = self.mount_handle.lock().expect("mount_handle poisoned");
            *slot = Some(handle);
        }

        if save_state {
            let mut slot = self
                .last_mount_state
                .lock()
                .expect("last_mount_state poisoned");
            *slot = Some(MountState {
                overlay_root: overlay_root.to_path_buf(),
                html: html.to_string(),
                trust,
            });
        }

        unsafe {
            trust_filter::apply(self.view, trust);
        }

        std::fs::create_dir_all(overlay_root).map_err(|e| {
            format!(
                "failed to create overlay root {}: {e}",
                overlay_root.display()
            )
        })?;

        // If a previous mount used a different directory, best-effort-remove
        // its scratch file so we don't leak orphans when switching overlays.
        {
            let guard = self
                .last_scratch_dir
                .lock()
                .expect("scratch dir mutex poisoned");
            if let Some(prev) = guard.as_ref() {
                if prev.as_path() != overlay_root {
                    let _ = std::fs::remove_file(prev.join(SCRATCH_NAME));
                }
            }
        }

        let scratch = overlay_root.join(SCRATCH_NAME);
        std::fs::write(&scratch, html)
            .map_err(|e| format!("failed to write scratch HTML to {}: {e}", scratch.display()))?;

        *self
            .last_scratch_dir
            .lock()
            .expect("scratch dir mutex poisoned") = Some(overlay_root.to_path_buf());

        let url = format!("file:///mount-{}/{}", mount_id, SCRATCH_NAME);
        unsafe {
            let c = std::ffi::CString::new(url).map_err(|e| format!("url cstring: {e}"))?;
            let ul_url = ultralight_sys::ulCreateString(c.as_ptr());
            ultralight_sys::ulViewLoadURL(self.view, ul_url);
            ultralight_sys::ulDestroyString(ul_url);
        }
        Ok(())
    }

    /// Execute a JavaScript string in the view.
    pub fn evaluate_script(&self, js: &str) {
        unsafe {
            let c_js =
                CString::new(js).unwrap_or_else(|_| CString::new(js.replace('\0', "")).unwrap());
            let ul_js = ultralight_sys::ulCreateString(c_js.as_ptr());
            let mut exception: ultralight_sys::ULString = std::ptr::null_mut();
            ultralight_sys::ulViewEvaluateScript(self.view, ul_js, &mut exception);
            ultralight_sys::ulDestroyString(ul_js);
            // We don't check the exception for now -- sensor updates are simple assignments
        }
    }

    /// Evaluate JS and return its string result, or the exception message.
    /// Safe wrapper over ulViewEvaluateScript that reads the ULString result.
    pub fn evaluate_script_result(&self, js: &str) -> Result<String, String> {
        unsafe {
            let c_js = std::ffi::CString::new(js)
                .unwrap_or_else(|_| std::ffi::CString::new(js.replace('\0', "")).unwrap());
            let ul_js = ultralight_sys::ulCreateString(c_js.as_ptr());
            let mut exception: ultralight_sys::ULString = std::ptr::null_mut();
            let result = ultralight_sys::ulViewEvaluateScript(self.view, ul_js, &mut exception);
            ultralight_sys::ulDestroyString(ul_js);

            let read_ul_string = |s: ultralight_sys::ULString| -> Option<String> {
                if s.is_null() {
                    return None;
                }
                let data = ultralight_sys::ulStringGetData(s);
                let len = ultralight_sys::ulStringGetLength(s);
                if data.is_null() || len == 0 {
                    return Some(String::new());
                }
                let bytes = std::slice::from_raw_parts(data as *const u8, len);
                Some(String::from_utf8_lossy(bytes).into_owned())
            };

            // UL owns these ULStrings; don't destroy.
            // ulViewEvaluateScript may set exception to a non-null empty string
            // when there is no actual exception (it pre-allocates the out-param).
            // Only treat it as an error when the exception string is non-empty.
            if !exception.is_null() {
                let len = ultralight_sys::ulStringGetLength(exception);
                if len > 0 {
                    let msg = read_ul_string(exception).unwrap_or_default();
                    return Err(msg);
                }
            }
            Ok(read_ul_string(result).unwrap_or_default())
        }
    }

    /// Update timers, begin a new display frame, then render.
    /// The three-step sequence is required for CSS transitions and animations:
    /// 1. ulUpdate — advance internal timers and dispatch callbacks
    /// 2. ulRefreshDisplay — signal that a new display frame is beginning
    /// 3. ulRender — render all views that need painting
    pub fn update_and_render(&self) {
        unsafe {
            ultralight_sys::ulUpdate(self.renderer);
            ultralight_sys::ulRefreshDisplay(self.renderer, 0);
            ultralight_sys::ulRender(self.renderer);
        }
    }

    /// Lock the surface pixels and call the provided closure with the pixel data and dirty rect.
    /// Pixel format: BGRA, premultiplied alpha.
    pub fn with_pixels<F>(&self, f: F)
    where
        F: FnOnce(u32, u32, u32, &[u8], (u32, u32, u32, u32)),
    {
        unsafe {
            let surface = ultralight_sys::ulViewGetSurface(self.view);
            if surface.is_null() {
                return;
            }

            let width = ultralight_sys::ulSurfaceGetWidth(surface);
            let height = ultralight_sys::ulSurfaceGetHeight(surface);
            let row_bytes = ultralight_sys::ulSurfaceGetRowBytes(surface);
            let dirty = ultralight_sys::ulSurfaceGetDirtyBounds(surface);

            let pixels_ptr = ultralight_sys::ulSurfaceLockPixels(surface);
            if !pixels_ptr.is_null() {
                let total_bytes = (row_bytes * height) as usize;
                let pixels = std::slice::from_raw_parts(pixels_ptr as *const u8, total_bytes);

                let dirty_rect = (
                    dirty.left.max(0) as u32,
                    dirty.top.max(0) as u32,
                    (dirty.right - dirty.left).max(0) as u32,
                    (dirty.bottom - dirty.top).max(0) as u32,
                );

                f(width, height, row_bytes, pixels, dirty_rect);

                ultralight_sys::ulSurfaceUnlockPixels(surface);
                ultralight_sys::ulSurfaceClearDirtyBounds(surface);
            }
        }
    }

    /// Resize the view.
    pub fn resize(&mut self, new_width: u32, new_height: u32) {
        unsafe {
            ultralight_sys::ulViewResize(self.view, new_width, new_height);
        }
        self.width = new_width;
        self.height = new_height;
        info!(
            width = new_width,
            height = new_height,
            "Ultralight view resized"
        );
    }

    /// Handle a thumbnail render request on the main thread.
    ///
    /// Saves the currently-mounted live overlay, temporarily mounts the
    /// thumbnail overlay at `overlay_root` with the provided HTML, injects
    /// `sample_values` via `__omni_update`, renders three ticks, captures
    /// BGRA pixels, then restores the live overlay.
    ///
    /// Called by the main render loop in response to messages on the
    /// thumbnail channel. Must be invoked on the same thread that owns
    /// this `UlRenderer` — Ultralight's C API is not thread-safe.
    pub fn render_thumbnail_to_png(
        &self,
        overlay_root: &Path,
        html: &str,
        sample_values: &HashMap<String, f64>,
    ) -> Result<ThumbnailPixels, String> {
        // Snapshot the live mount state BEFORE we overwrite it with the
        // thumbnail mount. `mount_internal(save_state = false)` below
        // leaves this slot untouched so the restore path can read it
        // after rendering.
        let saved = self
            .last_mount_state
            .lock()
            .expect("last_mount_state poisoned")
            .clone();

        // Mount the thumbnail overlay transiently.
        self.mount_internal(overlay_root, html, ViewTrust::ThumbnailGen, false)?;

        // Inject sample values through the privileged bootstrap.
        let payload = serde_json::to_string(sample_values)
            .map_err(|e| format!("sample_values JSON encode: {e}"))?;
        self.evaluate_script(&format!(
            "if(window.__omni_update){{__omni_update({payload});}}"
        ));

        // Three ticks: first kicks off async load, second lands the painted
        // frame, third settles any one-step CSS transition so themes with
        // transitions don't capture mid-interpolation.
        for _ in 0..3 {
            self.update_and_render();
        }

        let mut captured: Option<ThumbnailPixels> = None;
        self.with_pixels(|w, h, row_bytes, pixels, _dirty| {
            let tight_row = (w as usize) * 4;
            let mut buf = Vec::with_capacity(tight_row * h as usize);
            for row in 0..h as usize {
                let start = row * row_bytes as usize;
                buf.extend_from_slice(&pixels[start..start + tight_row]);
            }
            captured = Some(ThumbnailPixels {
                width: w,
                height: h,
                row_bytes,
                bgra: buf,
            });
        });

        let pixels = captured.ok_or_else(|| "with_pixels produced no surface data".to_string())?;

        // Restore the live mount so the render loop's next tick resumes
        // the user's overlay. If nothing was mounted before (early boot,
        // tests), leave the thumbnail mount in place; the next explicit
        // `mount()` call will replace it.
        if let Some(state) = saved {
            self.mount_internal(&state.overlay_root, &state.html, state.trust, true)?;
        }

        Ok(pixels)
    }
}

impl Drop for UlRenderer {
    fn drop(&mut self) {
        // Remove the dispatcher mount BEFORE destroying the Ultralight
        // view. `ulDestroyView` may synchronously fire final FS callbacks
        // to unwind in-flight loads; if the mount is still in the map at
        // that point, callbacks could reference an overlay_root whose
        // owning tempdir has already been cleaned up by the caller.
        // Removing the handle first guarantees those callbacks fall
        // through to `RESOURCES_FS` (or return None) harmlessly.
        if let Ok(mut slot) = self.mount_handle.lock() {
            let _ = slot.take(); // drops MountHandle -> removes from MOUNTS
        }

        // Best-effort-remove the scratch file from the last successful mount.
        if let Ok(mut guard) = self.last_scratch_dir.lock() {
            if let Some(dir) = guard.take() {
                let _ = std::fs::remove_file(dir.join(SCRATCH_NAME));
            }
        }

        unsafe {
            ultralight_sys::ulDestroyView(self.view);
            ultralight_sys::ulDestroyViewConfig(self.view_config);
            ultralight_sys::ulDestroyRenderer(self.renderer);
            ultralight_sys::ulDestroyConfig(self.config);
        }
        info!("Ultralight renderer destroyed");
    }
}
