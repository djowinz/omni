//! Safe wrapper around Ultralight C API for headless overlay rendering.

use std::ffi::CString;
use std::path::Path;

use tracing::info;

use crate::omni::fs_dispatcher;
use crate::omni::overlay_fs::OverlayFilesystem;
use crate::omni::trust_filter;
use crate::omni::view_trust::ViewTrust;

/// Safe wrapper around Ultralight renderer + view.
pub struct UlRenderer {
    renderer: ultralight_sys::ULRenderer,
    view: ultralight_sys::ULView,
    config: ultralight_sys::ULConfig,
    view_config: ultralight_sys::ULViewConfig,
    width: u32,
    height: u32,
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
            })
        }
    }

    /// Mount an overlay or bundle into the View. Scopes the platform FS to
    /// `overlay_root`, applies the trust filter, and loads `html` via a
    /// synthetic `file:///` URL so relative `url(...)` references resolve
    /// against the overlay directory.
    ///
    /// A scratch file `.omni_current.html` is written inside `overlay_root`.
    /// Callers must have write access to that directory.
    pub fn mount(
        &self,
        overlay_root: &Path,
        html: &str,
        trust: ViewTrust,
    ) -> Result<(), String> {
        fs_dispatcher::set_active(OverlayFilesystem::new(overlay_root.to_path_buf()));
        unsafe { trust_filter::apply(self.view, trust); }

        let scratch = overlay_root.join(".omni_current.html");
        std::fs::write(&scratch, html)
            .map_err(|e| format!("failed to write scratch HTML to {}: {e}", scratch.display()))?;

        let url = "file:///.omni_current.html";
        unsafe {
            let c = std::ffi::CString::new(url).map_err(|e| format!("url cstring: {e}"))?;
            let ul_url = ultralight_sys::ulCreateString(c.as_ptr());
            ultralight_sys::ulViewLoadURL(self.view, ul_url);
            ultralight_sys::ulDestroyString(ul_url);
        }
        Ok(())
    }

    /// Load an HTML string into the view.
    #[deprecated(note = "use mount(...) with an explicit overlay root")]
    #[allow(dead_code)]
    pub fn load_html(&self, html: &str) {
        unsafe {
            let c_html = CString::new(html).unwrap_or_else(|_| {
                // Strip null bytes if present
                CString::new(html.replace('\0', "")).unwrap()
            });
            let ul_html = ultralight_sys::ulCreateString(c_html.as_ptr());
            ultralight_sys::ulViewLoadHTML(self.view, ul_html);
            ultralight_sys::ulDestroyString(ul_html);
        }
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
}

impl Drop for UlRenderer {
    fn drop(&mut self) {
        unsafe {
            ultralight_sys::ulDestroyView(self.view);
            ultralight_sys::ulDestroyViewConfig(self.view_config);
            ultralight_sys::ulDestroyRenderer(self.renderer);
            ultralight_sys::ulDestroyConfig(self.config);
        }
        info!("Ultralight renderer destroyed");
    }
}
