//! Thumbnail generation for themes and bundles.
//!
//! Composes the shipped Ultralight harness at `ViewTrust::ThumbnailGen`.
//! Never introduces a second renderer path (architectural invariant #8).
//!
//! Call order for bundles (invariants #6a + #19b):
//!   1. `bundle::unpack_manifest`  (zero file I/O)
//!   2. schema_version + resource_kinds pre-flight
//!   3. `identity::unpack_signed_bundle`
//!   4. stream `files()` to a `tempfile::TempDir`
//!   5. render via `render_omni_to_png`

pub mod bundle;
pub mod theme;

use std::collections::HashMap;
use std::path::Path;

use ::bundle::BundleError;
use ::identity::IdentityError;
use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
use image::{ColorType, ImageEncoder};

use crate::omni::history::SensorHistory;
use crate::omni::html_builder::build_initial_html;
use crate::omni::types::OmniFile;
use crate::omni::view_trust::ViewTrust;
use shared::SensorSnapshot;

/// Public error enum for thumbnail generation.
///
/// Carved on consumer semantics (invariant #19a). Third-party errors ride in
/// the `#[source]` chain rather than `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum ThumbnailError {
    #[error("bundle declares unsupported resource kind: {kind}")]
    UnsupportedKind { kind: String },

    #[error("bundle declares unsupported schema_version: {version}")]
    UnsupportedSchemaVersion { version: u32 },

    #[error("render failed: {detail}")]
    RenderFailed { detail: String },

    #[error("surface dimensions did not match configured size")]
    SurfaceDimensionsMismatch,

    #[error("encoded thumbnail exceeds size budget after retries: {bytes} bytes")]
    TooLarge { bytes: usize },

    #[error("identity error")]
    Identity(#[source] IdentityError),

    #[error("bundle error")]
    Bundle(#[source] BundleError),

    #[error("I/O error")]
    Io(#[source] std::io::Error),

    #[error("image encoding error")]
    Encode(#[source] image::ImageError),
}

/// Size budget from `contracts/worker-api.md` §4.1.
pub const MAX_THUMBNAIL_BYTES: usize = 256 * 1024;

/// Default render dimensions.
pub const DEFAULT_WIDTH: u32 = 800;
pub const DEFAULT_HEIGHT: u32 = 450;

/// Fallback dimensions used when the 800×450 PNG exceeds `MAX_THUMBNAIL_BYTES`
/// even under maximum PNG compression.
pub const FALLBACK_WIDTH: u32 = 600;
pub const FALLBACK_HEIGHT: u32 = 338;

#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    pub width: u32,
    pub height: u32,
    pub sample_values: HashMap<String, f64>,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            sample_values: default_sample_values(),
        }
    }
}

/// Render a parsed overlay at `config.width × config.height`, encode the
/// BGRA surface to a PNG whose size does not exceed [`MAX_THUMBNAIL_BYTES`],
/// and return the encoded bytes.
///
/// **Directory layout requirement.** This helper delegates theme resolution to
/// [`crate::workspace::structure::resolve_theme_path`], which searches
/// `data_dir/overlays/<overlay_name>/<theme_src>` first, then
/// `data_dir/themes/<theme_src>`. Callers (the `generate_for_theme` /
/// `generate_for_bundle` entry points in Task 4/5) must lay out a
/// `tempfile::TempDir` so that one of those paths resolves to the theme CSS
/// they want rendered.
///
/// **Signature pivot (away from the plan).** The plan originally proposed
/// `render_omni_to_png(overlay_dir, entry_html_relative, config)`, which
/// reflected an HTML/CSS/manifest file tree assumption. The shipped renderer
/// takes a parsed [`OmniFile`] + `data_dir` + `overlay_name` triple (see
/// [`build_initial_html`]), so the helper takes the same shape. Callers
/// pre-parse the `.omni` source and stage `data_dir` on disk.
pub(super) fn render_omni_to_png(
    omni_file: &OmniFile,
    data_dir: &Path,
    overlay_name: &str,
    config: &ThumbnailConfig,
) -> Result<Vec<u8>, ThumbnailError> {
    // Architectural invariant #24 prevents a second `UlRenderer` instance
    // — Ultralight's C API has process-global state that crashes on
    // multi-instance use (Windows `STATUS_STACK_BUFFER_OVERRUN` /
    // `STATUS_ACCESS_VIOLATION`). Instead, the render request is sent to
    // the main thread's live renderer via a process-wide channel installed
    // at host startup by `main.rs`. The live render loop drains the
    // channel between ticks and services the request on-thread — the live
    // preview briefly freezes (~100 ms) while the thumbnail captures, then
    // the live overlay is remounted. See spec
    // `docs/superpowers/specs/2026-04-19-ultralight-thumbnail-fix-design.md`.

    let snapshot = sample_values_to_snapshot(&config.sample_values);
    let hwinfo_values: HashMap<String, f64> = HashMap::new();
    let hwinfo_units: HashMap<String, String> = HashMap::new();
    let history = SensorHistory::default();

    let initial = build_initial_html(
        omni_file,
        &snapshot,
        config.width,
        config.height,
        data_dir,
        overlay_name,
        &hwinfo_values,
        &hwinfo_units,
        &history,
        ViewTrust::ThumbnailGen,
    );

    let overlay_root = crate::workspace::structure::overlay_dir(data_dir, overlay_name);

    let sender = crate::ul_renderer::get_thumbnail_channel().ok_or_else(|| {
        ThumbnailError::RenderFailed {
            detail: "thumbnail channel not installed — host render loop is not running".into(),
        }
    })?;

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    let request = crate::ul_renderer::ThumbnailRequest {
        overlay_root,
        html: initial.full_document,
        sample_values: config.sample_values.clone(),
        reply: reply_tx,
    };
    sender
        .send(request)
        .map_err(|_| ThumbnailError::RenderFailed {
            detail: "thumbnail channel closed".into(),
        })?;

    // Block until the main render thread replies. This function is called
    // from `tokio::task::spawn_blocking` (see `share::upload::render_thumbnail_inner`),
    // so a synchronous wait here does not stall the tokio worker pool's
    // async tasks. `std::sync::mpsc::Receiver::recv` is runtime-agnostic
    // (no tokio context required) — unlike `tokio::sync::oneshot::blocking_recv`
    // which asserts against tokio runtime detection internals.
    let pixels = reply_rx
        .recv()
        .map_err(|_| ThumbnailError::RenderFailed {
            detail: "thumbnail reply dropped (main render thread gone)".into(),
        })?
        .map_err(|detail| ThumbnailError::RenderFailed { detail })?;

    // Crop to the widget's bounding box + 5% breathing-room padding on
    // each side, then scale to the target thumbnail dimensions with
    // aspect preserved (letterbox in transparent pixels). If no bbox is
    // available, fall back to the full frame and let encode_with_size_cap
    // handle downscaling.
    let (cropped_w, cropped_h, cropped_bgra) = match pixels.widget_bbox {
        Some(b) => crop_with_padding(
            &pixels.bgra,
            pixels.width,
            pixels.height,
            b.x,
            b.y,
            b.w,
            b.h,
            0.05,
        ),
        None => (pixels.width, pixels.height, pixels.bgra),
    };

    let (out_w, out_h, out_bgra) = scale_to_fit_with_letterbox(
        &cropped_bgra,
        cropped_w,
        cropped_h,
        config.width,
        config.height,
    );

    encode_with_size_cap(&out_bgra, out_w, out_h)
}

/// Crop a BGRA surface to `(x, y, w, h)` plus `padding_ratio` of breathing
/// room on each side (clamped to surface bounds). Returns the new
/// (width, height, tightly-packed BGRA).
#[allow(clippy::too_many_arguments)]
fn crop_with_padding(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    padding_ratio: f32,
) -> (u32, u32, Vec<u8>) {
    let pad_x = ((w as f32) * padding_ratio).round() as u32;
    let pad_y = ((h as f32) * padding_ratio).round() as u32;
    let x0 = x.saturating_sub(pad_x);
    let y0 = y.saturating_sub(pad_y);
    let x1 = (x + w + pad_x).min(src_w);
    let y1 = (y + h + pad_y).min(src_h);
    let cw = x1.saturating_sub(x0);
    let ch = y1.saturating_sub(y0);
    if cw == 0 || ch == 0 {
        return (src_w, src_h, src.to_vec());
    }
    let src_row = (src_w as usize) * 4;
    let dst_row = (cw as usize) * 4;
    let mut out = Vec::with_capacity(dst_row * ch as usize);
    for row in 0..ch as usize {
        let src_off = (y0 as usize + row) * src_row + (x0 as usize) * 4;
        out.extend_from_slice(&src[src_off..src_off + dst_row]);
    }
    (cw, ch, out)
}

/// Resize `src` to fit within `target_w × target_h` preserving aspect,
/// then letterbox with transparent pixels into exactly `target_w × target_h`.
/// Returns (target_w, target_h, tightly-packed BGRA).
fn scale_to_fit_with_letterbox(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    target_w: u32,
    target_h: u32,
) -> (u32, u32, Vec<u8>) {
    if src_w == 0 || src_h == 0 {
        return (
            target_w,
            target_h,
            vec![0u8; (target_w * target_h * 4) as usize],
        );
    }
    let scale = (target_w as f32 / src_w as f32).min(target_h as f32 / src_h as f32);
    let scaled_w = ((src_w as f32 * scale).round() as u32).max(1).min(target_w);
    let scaled_h = ((src_h as f32 * scale).round() as u32).max(1).min(target_h);

    // `image::imageops::resize` operates per-channel so treating BGRA as
    // RGBA for the duration of the resize is correct — channel meanings
    // don't affect interpolation math, and the downstream `bgra_to_rgba`
    // in `encode_with_size_cap` still sees BGRA bytes on output.
    let src_img =
        match image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(src_w, src_h, src.to_vec()) {
            Some(img) => img,
            None => {
                return (
                    target_w,
                    target_h,
                    vec![0u8; (target_w * target_h * 4) as usize],
                )
            }
        };
    let scaled = image::imageops::resize(
        &src_img,
        scaled_w,
        scaled_h,
        image::imageops::FilterType::Lanczos3,
    );
    let scaled_bytes = scaled.as_raw();

    // Letterbox into transparent target frame.
    let mut out = vec![0u8; (target_w * target_h * 4) as usize];
    let off_x = (target_w - scaled_w) / 2;
    let off_y = (target_h - scaled_h) / 2;
    let target_row = (target_w as usize) * 4;
    let scaled_row = (scaled_w as usize) * 4;
    for row in 0..scaled_h as usize {
        let src_off = row * scaled_row;
        let dst_off = (off_y as usize + row) * target_row + (off_x as usize) * 4;
        out[dst_off..dst_off + scaled_row]
            .copy_from_slice(&scaled_bytes[src_off..src_off + scaled_row]);
    }
    (target_w, target_h, out)
}

/// Map the sample-values HashMap into a [`SensorSnapshot`] by matching the
/// well-known dotted keys to the concrete struct fields.
///
/// Unmapped keys are silently dropped — only keys the reference overlay binds
/// to via `data-sensor` actually drive pixels. Added keys that future overlays
/// consume should be mirrored here so theme authors can exercise them at
/// thumbnail time.
fn sample_values_to_snapshot(values: &HashMap<String, f64>) -> SensorSnapshot {
    let mut s = SensorSnapshot::default();
    for (key, v) in values {
        let v = *v;
        match key.as_str() {
            "cpu.usage" => s.cpu.total_usage_percent = v as f32,
            "cpu.temp" => s.cpu.package_temp_c = v as f32,
            "gpu.usage" => s.gpu.usage_percent = v as f32,
            "gpu.temp" => s.gpu.temp_c = v as f32,
            "gpu.vram_used" => s.gpu.vram_used_mb = v as u32,
            "gpu.vram_total" => s.gpu.vram_total_mb = v as u32,
            "ram.used" => s.ram.used_mb = v as u64,
            "ram.total" => s.ram.total_mb = v as u64,
            "ram.usage" => s.ram.usage_percent = v as f32,
            "fps.current" | "frame.fps" => {
                s.frame.fps = v as f32;
                s.frame.available = true;
            }
            _ => {}
        }
    }
    // If ram.used/total were set but ram.usage wasn't, derive it so themes
    // binding either surface render with consistent values.
    if s.ram.usage_percent == 0.0 && s.ram.total_mb > 0 {
        s.ram.usage_percent = (s.ram.used_mb as f64 / s.ram.total_mb as f64 * 100.0) as f32;
    }
    s
}

/// Encode a BGRA buffer to PNG, applying the §8 size-cap retry pipeline:
/// 1. Default compression
/// 2. Best compression
/// 3. Downscale to FALLBACK_WIDTH × FALLBACK_HEIGHT (Lanczos3) + Best
///
/// Returns [`ThumbnailError::TooLarge`] if no stage fits under the cap.
fn encode_with_size_cap(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ThumbnailError> {
    let rgba = bgra_to_rgba(bgra);
    let png = bgra_to_png(&rgba, width, height, CompressionType::Default)?;
    if png.len() <= MAX_THUMBNAIL_BYTES {
        return Ok(png);
    }

    let png = bgra_to_png(&rgba, width, height, CompressionType::Best)?;
    if png.len() <= MAX_THUMBNAIL_BYTES {
        return Ok(png);
    }

    // Downscale. `ImageBuffer::from_raw` cannot fail here — length is
    // `width * height * 4` by construction — but guard defensively.
    let src = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or(ThumbnailError::SurfaceDimensionsMismatch)?;
    let small = image::imageops::resize(
        &src,
        FALLBACK_WIDTH,
        FALLBACK_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    );
    let small_bytes = small.into_raw();
    let png = bgra_to_png(
        &small_bytes,
        FALLBACK_WIDTH,
        FALLBACK_HEIGHT,
        CompressionType::Best,
    )?;
    if png.len() <= MAX_THUMBNAIL_BYTES {
        return Ok(png);
    }

    Err(ThumbnailError::TooLarge { bytes: png.len() })
}

/// Swap BGRA premultiplied channels to RGBA. `image` 0.24 exposes no direct
/// BGRA ingestion path; justified DIY under writing-lessons rule #16 (simple
/// requirement, simple solution, will not expand — spec §7).
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for chunk in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
    }
    rgba
}

/// Encode a tightly-packed RGBA8 buffer to PNG at the given compression level.
fn bgra_to_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    compression: CompressionType,
) -> Result<Vec<u8>, ThumbnailError> {
    let mut out = Vec::new();
    let encoder = PngEncoder::new_with_quality(&mut out, compression, PngFilterType::Adaptive);
    encoder
        .write_image(rgba, width, height, ColorType::Rgba8)
        .map_err(ThumbnailError::Encode)?;
    Ok(out)
}

/// Deterministic sample-values. Spec §5 — values stay below any warn/crit
/// thresholds so untouched themes render in a neutral state.
pub fn default_sample_values() -> HashMap<String, f64> {
    HashMap::from([
        ("cpu.usage".into(), 42.0),
        ("cpu.temp".into(), 58.0),
        ("gpu.usage".into(), 67.0),
        ("gpu.temp".into(), 71.0),
        ("gpu.vram_used".into(), 6800.0),
        ("gpu.vram_total".into(), 8192.0),
        ("ram.used".into(), 16384.0),
        ("ram.total".into(), 32768.0),
        ("net.down".into(), 1200.0),
        ("net.up".into(), 340.0),
        ("fps.current".into(), 144.0),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_rgba_swaps_red_and_blue() {
        // Single BGRA pixel: B=10, G=20, R=30, A=40 → RGBA: 30, 20, 10, 40.
        let bgra = [10u8, 20, 30, 40];
        let rgba = bgra_to_rgba(&bgra);
        assert_eq!(rgba, vec![30, 20, 10, 40]);
    }

    #[test]
    fn sample_values_to_snapshot_populates_known_fields() {
        let snap = sample_values_to_snapshot(&default_sample_values());
        assert_eq!(snap.cpu.total_usage_percent, 42.0);
        assert_eq!(snap.cpu.package_temp_c, 58.0);
        assert_eq!(snap.gpu.usage_percent, 67.0);
        assert_eq!(snap.gpu.temp_c, 71.0);
        assert_eq!(snap.gpu.vram_used_mb, 6800);
        assert_eq!(snap.gpu.vram_total_mb, 8192);
        assert_eq!(snap.ram.used_mb, 16384);
        assert_eq!(snap.ram.total_mb, 32768);
        // Derived from used/total since default_sample_values doesn't set it.
        assert!((snap.ram.usage_percent - 50.0).abs() < 0.1);
        assert_eq!(snap.frame.fps, 144.0);
        assert!(snap.frame.available);
    }

    #[test]
    fn sample_values_to_snapshot_ignores_unknown_keys() {
        let mut vals = HashMap::new();
        vals.insert("no.such.sensor".to_string(), 999.0);
        vals.insert("cpu.usage".to_string(), 33.0);
        let snap = sample_values_to_snapshot(&vals);
        assert_eq!(snap.cpu.total_usage_percent, 33.0);
    }

    #[test]
    fn encode_with_size_cap_emits_a_valid_png_for_small_input() {
        // 8×8 solid-magenta BGRA — trivially well under 256 KiB.
        let w = 8u32;
        let h = 8u32;
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            bgra.extend_from_slice(&[0xFF, 0x00, 0xFF, 0xFF]); // B, G, R, A
        }
        let png = encode_with_size_cap(&bgra, w, h).expect("encode");
        assert!(png.len() <= MAX_THUMBNAIL_BYTES);
        // PNG magic header.
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }
}
