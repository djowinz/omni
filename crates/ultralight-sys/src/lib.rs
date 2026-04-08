//! Minimal FFI bindings for the Ultralight C API.
//! Only the functions needed for headless CPU rendering are included.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_uint, c_int, c_void};

// Opaque pointer types matching the C typedefs (e.g. `typedef struct C_Config* ULConfig`).
pub type ULConfig = *mut c_void;
pub type ULRenderer = *mut c_void;
pub type ULView = *mut c_void;
pub type ULViewConfig = *mut c_void;
pub type ULSession = *mut c_void;
pub type ULString = *mut c_void;
pub type ULSurface = *mut c_void;
pub type ULBitmap = *mut c_void;

/// Integer rectangle (used for dirty bounds).
/// Matches the C definition in CAPI_Defines.h.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ULIntRect {
    pub left: c_int,
    pub top: c_int,
    pub right: c_int,
    pub bottom: c_int,
}

extern "C" {
    // ── String ──────────────────────────────────────────────────────────
    /// Create string from null-terminated ASCII C-string.
    pub fn ulCreateString(str: *const c_char) -> ULString;
    /// Create string from UTF-8 buffer with explicit length.
    pub fn ulCreateStringUTF8(str: *const c_char, len: usize) -> ULString;
    /// Destroy a string created with ulCreateString / ulCreateStringUTF8.
    pub fn ulDestroyString(str: ULString);

    // ── Config ──────────────────────────────────────────────────────────
    pub fn ulCreateConfig() -> ULConfig;
    pub fn ulDestroyConfig(config: ULConfig);
    pub fn ulConfigSetResourcePathPrefix(config: ULConfig, resource_path_prefix: ULString);
    pub fn ulConfigSetCachePath(config: ULConfig, cache_path: ULString);

    // ── Platform (AppCore convenience functions) ────────────────────────
    pub fn ulEnablePlatformFontLoader();
    pub fn ulEnablePlatformFileSystem(base_dir: ULString);
    pub fn ulEnableDefaultLogger(log_path: ULString);

    // ── Renderer ────────────────────────────────────────────────────────
    pub fn ulCreateRenderer(config: ULConfig) -> ULRenderer;
    pub fn ulDestroyRenderer(renderer: ULRenderer);
    pub fn ulUpdate(renderer: ULRenderer);
    pub fn ulRender(renderer: ULRenderer);
    pub fn ulRefreshDisplay(renderer: ULRenderer, display_id: c_uint);

    // ── View Config ─────────────────────────────────────────────────────
    pub fn ulCreateViewConfig() -> ULViewConfig;
    pub fn ulDestroyViewConfig(config: ULViewConfig);
    pub fn ulViewConfigSetIsAccelerated(config: ULViewConfig, is_accelerated: bool);
    pub fn ulViewConfigSetIsTransparent(config: ULViewConfig, is_transparent: bool);
    pub fn ulViewConfigSetInitialDeviceScale(config: ULViewConfig, initial_device_scale: f64);

    // ── View ────────────────────────────────────────────────────────────
    pub fn ulCreateView(
        renderer: ULRenderer,
        width: c_uint,
        height: c_uint,
        view_config: ULViewConfig,
        session: ULSession,
    ) -> ULView;
    pub fn ulDestroyView(view: ULView);
    pub fn ulViewLoadHTML(view: ULView, html_string: ULString);
    pub fn ulViewEvaluateScript(
        view: ULView,
        js_string: ULString,
        exception: *mut ULString,
    ) -> ULString;
    pub fn ulViewGetSurface(view: ULView) -> ULSurface;
    pub fn ulViewResize(view: ULView, width: c_uint, height: c_uint);

    // ── Surface ─────────────────────────────────────────────────────────
    pub fn ulSurfaceGetWidth(surface: ULSurface) -> c_uint;
    pub fn ulSurfaceGetHeight(surface: ULSurface) -> c_uint;
    pub fn ulSurfaceGetRowBytes(surface: ULSurface) -> c_uint;
    pub fn ulSurfaceLockPixels(surface: ULSurface) -> *mut c_void;
    pub fn ulSurfaceUnlockPixels(surface: ULSurface);
    pub fn ulSurfaceGetDirtyBounds(surface: ULSurface) -> ULIntRect;
    pub fn ulSurfaceClearDirtyBounds(surface: ULSurface);
}
