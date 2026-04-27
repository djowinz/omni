//! Safe wrapper around Ultralight C API for headless overlay rendering.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use std::sync::mpsc as std_mpsc;

use tokio::sync::mpsc;
use tracing::info;

use crate::omni::fs_dispatcher;
use crate::omni::overlay_fs::OverlayFilesystem;
use crate::omni::trust_filter;
use crate::omni::view_trust::ViewTrust;

/// Filename of the scratch HTML written into the overlay root on mount.
const SCRATCH_NAME: &str = ".omni_current.html";

/// Raw BGRA pixel buffer produced by a thumbnail render request, plus
/// the bounding box of the first widget so downstream can crop to the
/// content area and discard the empty viewport around it.
pub struct ThumbnailPixels {
    pub width: u32,
    pub height: u32,
    pub row_bytes: u32,
    /// Tightly-packed BGRA (row-stride-stripped) — `width * 4 * height` bytes.
    pub bgra: Vec<u8>,
    /// Bounding box of the primary widget in surface coordinates
    /// (pixels, origin top-left). `None` when the widget query returned
    /// no element, or the element had zero-size bounds — caller falls
    /// back to the full frame.
    pub widget_bbox: Option<WidgetBbox>,
}

#[derive(Debug, Clone, Copy)]
pub struct WidgetBbox {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
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
    /// `std::sync::mpsc::Sender` (not `tokio::sync::oneshot`) so the
    /// receiver's `.recv()` works from any thread without tokio runtime
    /// entanglement. The consumer calls `reply.send(Ok(...))` exactly
    /// once; a dropped sender without sending is treated as a render
    /// failure by the caller.
    pub reply: std_mpsc::Sender<Result<ThumbnailPixels, String>>,
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
    ///
    /// `device_scale`, when `Some(s)`, is applied via
    /// `ulViewConfigSetInitialDeviceScale` BEFORE the view is created so the
    /// Ultralight view comes up DPI-aware. `None` preserves today's behavior
    /// (implicit scale = 1.0). Spec:
    /// docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
    pub fn init(
        width: u32,
        height: u32,
        device_scale: Option<f64>,
        resources_dir: &Path,
    ) -> Result<Self, String> {
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

            // Build the view through the same helper `recreate_view` uses, so
            // the view-config flag set (accelerated/transparent/initial-device-
            // scale) lives in exactly one place and can't drift between init
            // and recreation paths.
            let (view, view_config) =
                match Self::try_create_view(renderer, width, height, device_scale) {
                    Some(pair) => pair,
                    None => {
                        ultralight_sys::ulDestroyRenderer(renderer);
                        ultralight_sys::ulDestroyConfig(config);
                        return Err("Failed to create Ultralight view".into());
                    }
                };

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

    /// Tear down the current view + view_config and create a fresh pair with
    /// the given dimensions and device scale. Renderer is preserved (process-
    /// global Ultralight state is not safe to recreate). The fs_dispatcher
    /// mount handle is dropped before `ulDestroyView` to mirror the Drop-impl
    /// invariant — see `Drop for UlRenderer` below.
    ///
    /// Caller is responsible for re-mounting the live overlay via `mount()`
    /// afterward; this method leaves the new view empty.
    ///
    /// **Failure semantics:** if `ulCreateView` returns null at the new dims,
    /// the renderer attempts a rollback by recreating the view at the previous
    /// dimensions with no device scale (the most permissive Ultralight
    /// configuration). On rollback success the function returns `Err` but the
    /// renderer remains usable at the previous geometry; the caller may log
    /// and continue. If rollback ALSO fails, the renderer is truly
    /// unrecoverable and the function panics.
    ///
    /// `last_mount_state` and `last_scratch_dir` are intentionally preserved
    /// across recreation so the caller's required `mount()` (or any in-flight
    /// thumbnail capture's restore path) refers to the correct overlay.
    ///
    /// Used by main.rs when the resolved DPI scale changes (per-overlay
    /// `<dpi-scale>` directive switched, or `Auto` mode game-window-monitor DPI
    /// changed). Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md
    pub fn recreate_view(
        &mut self,
        new_width: u32,
        new_height: u32,
        device_scale: Option<f64>,
    ) -> Result<(), String> {
        let old_width = self.width;
        let old_height = self.height;

        // 1. Drop mount handle FIRST — see Drop impl invariant.
        {
            let mut slot = self.mount_handle.lock().expect("mount_handle poisoned");
            let _ = slot.take();
        }

        // 2. Tear down view + view_config.
        unsafe {
            ultralight_sys::ulDestroyView(self.view);
            ultralight_sys::ulDestroyViewConfig(self.view_config);
        }

        // 3. Try to create the new view at requested dims + scale.
        if let Some((view, view_config)) =
            Self::try_create_view(self.renderer, new_width, new_height, device_scale)
        {
            self.view = view;
            self.view_config = view_config;
            self.width = new_width;
            self.height = new_height;
            info!(
                width = new_width,
                height = new_height,
                ?device_scale,
                "Ultralight view recreated"
            );
            return Ok(());
        }

        // 4. Rollback: try original dimensions with NO device scale (most permissive).
        if let Some((view, view_config)) =
            Self::try_create_view(self.renderer, old_width, old_height, None)
        {
            self.view = view;
            self.view_config = view_config;
            // self.width / self.height already hold old values (untouched in step 3).
            let msg = format!(
                "ulCreateView failed at {}x{} scale={:?}; rolled back to {}x{} no-scale",
                new_width, new_height, device_scale, old_width, old_height
            );
            tracing::warn!(error = %msg, "Ultralight view recreation rolled back");
            return Err(msg);
        }

        // 5. Both attempts failed — Ultralight is in an unrecoverable state.
        panic!(
            "ulCreateView failed in recreate_view (target {}x{} scale {:?}) AND rollback to {}x{} no-scale ALSO failed; renderer is unrecoverable",
            new_width, new_height, device_scale, old_width, old_height
        );
    }

    /// Build a fresh `ULView` + `ULViewConfig` against the given renderer.
    /// Returns `None` if `ulCreateView` returns null, after destroying the
    /// orphaned `view_config` so the FFI doesn't leak. Caller is responsible
    /// for storing the returned pair.
    fn try_create_view(
        renderer: ultralight_sys::ULRenderer,
        width: u32,
        height: u32,
        device_scale: Option<f64>,
    ) -> Option<(ultralight_sys::ULView, ultralight_sys::ULViewConfig)> {
        unsafe {
            let view_config = ultralight_sys::ulCreateViewConfig();
            ultralight_sys::ulViewConfigSetIsAccelerated(view_config, false);
            ultralight_sys::ulViewConfigSetIsTransparent(view_config, true);
            if let Some(scale) = device_scale {
                ultralight_sys::ulViewConfigSetInitialDeviceScale(view_config, scale);
            }
            let view = ultralight_sys::ulCreateView(
                renderer,
                width,
                height,
                view_config,
                std::ptr::null_mut(),
            );
            if view.is_null() {
                ultralight_sys::ulDestroyViewConfig(view_config);
                None
            } else {
                Some((view, view_config))
            }
        }
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

        // Inner body runs mount-through-capture. Whatever it returns, we
        // attempt to restore the live mount in the guaranteed-tail below —
        // including on error paths — so a rare mid-sequence failure (JSON
        // encode, `with_pixels` returning nothing) doesn't leave the view
        // stuck on the thumbnail overlay.
        let result = self.render_thumbnail_inner(overlay_root, html, sample_values);

        // Always attempt restoration. Log-and-continue on restore failure
        // rather than overwriting the caller's error — a restore failure
        // here is unlikely and less actionable than the primary cause.
        if let Some(state) = saved {
            if let Err(e) = self.mount_internal(&state.overlay_root, &state.html, state.trust, true)
            {
                tracing::error!(
                    error = %e,
                    "live overlay restore after thumbnail capture failed; next mount() will recover"
                );
            }
        }

        result
    }

    /// Inner body of [`render_thumbnail_to_png`] covering the
    /// mount-thumbnail → inject → tick → capture sequence. Extracted so
    /// the outer method can guarantee live-overlay restoration on every
    /// exit path, error or success.
    fn render_thumbnail_inner(
        &self,
        overlay_root: &Path,
        html: &str,
        sample_values: &HashMap<String, f64>,
    ) -> Result<ThumbnailPixels, String> {
        // Mount the thumbnail overlay transiently. `save_state = false`
        // leaves the saved live-mount snapshot intact so the caller's
        // restore path can read it.
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

        // Query the first widget's bounding box in the rendered DOM. The
        // html_builder unwraps widgets directly as body children, so the
        // first child IS the primary widget's template root. Empty body
        // (malformed overlay) returns None; caller falls back to the
        // full frame.
        let bbox_js = r#"(function(){
            var el = document.body && document.body.firstElementChild;
            if (!el) return "null";
            var r = el.getBoundingClientRect();
            if (r.width <= 0 || r.height <= 0) return "null";
            return JSON.stringify({x: r.left, y: r.top, w: r.width, h: r.height});
        })();"#;
        let widget_bbox = match self.evaluate_script_result(bbox_js) {
            Ok(s) if s == "null" => None,
            Ok(s) => parse_bbox_json(&s),
            Err(e) => {
                tracing::warn!(error = %e, "widget bbox query failed; falling back to full frame");
                None
            }
        };

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
                widget_bbox,
            });
        });

        captured.ok_or_else(|| "with_pixels produced no surface data".to_string())
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

        // Invariant: the mount must be released before destroying the
        // Ultralight view. A future refactor that drops this invariant
        // would reintroduce the use-after-free risk the mount-handle
        // Drop-first ordering was designed to prevent.
        debug_assert!(
            self.mount_handle
                .lock()
                .map(|g| g.is_none())
                .unwrap_or(true),
            "mount_handle must be released before ulDestroyView"
        );

        unsafe {
            ultralight_sys::ulDestroyView(self.view);
            ultralight_sys::ulDestroyViewConfig(self.view_config);
            ultralight_sys::ulDestroyRenderer(self.renderer);
            ultralight_sys::ulDestroyConfig(self.config);
        }
        info!("Ultralight renderer destroyed");
    }
}

/// Parse the `{x, y, w, h}` JSON emitted by the widget-bbox query script.
/// Coordinates are CSS pixels (sub-pixel float); we truncate to integer
/// surface coordinates and clamp negative origins to zero.
fn parse_bbox_json(s: &str) -> Option<WidgetBbox> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    let w = v.get("w")?.as_f64()?;
    let h = v.get("h")?.as_f64()?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some(WidgetBbox {
        x: x.max(0.0).floor() as u32,
        y: y.max(0.0).floor() as u32,
        w: w.ceil() as u32,
        h: h.ceil() as u32,
    })
}
