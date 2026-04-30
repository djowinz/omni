//! Whole-image NSFW classifier wrapper (Falconsai/nsfw_image_detection).
//!
//! Loads `crates/moderation/resources/nsfw_falconsai.onnx`
//! (Falconsai/nsfw_image_detection, ViT-base/16 fine-tuned for binary
//! `{normal, nsfw}` classification — see model card on HuggingFace) into an
//! `ort::Session` and runs single-image inference. Output is collapsed to one
//! [`ModerationResult`]: the softmaxed `nsfw` probability plus a fixed label
//! (`"nsfw"` when the score crosses the caller's threshold, `"safe"` below).
//!
//! # Why this replaced NudeNet
//!
//! NudeNet is a *part-presence detector* — it bounds and labels exposed body
//! regions. That's structurally inadequate for "covered but suggestive"
//! content (bikini photos, game cover art): no `EXPOSED_*` class fires, so no
//! amount of threshold tuning catches it. Expanding the unsafe set to include
//! `COVERED_*` classes was tried 2026-04-28 and produced inconsistent results
//! (Tifa-in-dress flagged, 2B/Nier missed) because per-class detection
//! confidence doesn't correlate with suggestiveness.
//!
//! Falconsai is trained as a single-output `{normal, nsfw}` ViT classifier
//! over whole-image composition, which DOES capture the suggestiveness axis.
//!
//! # Reference preprocessing
//!
//! Verified directly from the Falconsai HF `preprocessor_config.json`
//! (`ViTImageProcessor`, fetched 2026-04-28):
//!
//! 1. Decode bytes (PNG/JPEG via `image` crate).
//! 2. Convert to RGB.
//! 3. Resize to 224×224 with bilinear interpolation (PIL `resample=2`).
//!    NOTE: NO aspect-ratio padding — `ViTImageProcessor` resizes directly to
//!    a square, distorting non-square images. We match that behavior.
//! 4. Rescale by `1/255` then normalize per-channel with `mean=[0.5,0.5,0.5]`,
//!    `std=[0.5,0.5,0.5]` → final values in `[-1.0, 1.0]`.
//! 5. Layout NCHW: `[1, 3, 224, 224]` f32.
//!
//! Output tensor is `[1, 2]` logits with `id2label = {0: normal, 1: nsfw}`.
//! We softmax then return index 1 as `unsafe_score`.
//!
//! Higher-level rejection threshold is applied by callers in
//! `host::share::moderation::REJECTION_THRESHOLD` (INV-7.7.3).

use std::path::Path;

use ndarray::Array4;
use ort::{
    inputs,
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

use crate::{ModerationError, ModerationResult};

/// Square input dimension for the bundled Falconsai ViT-base/16 classifier.
pub const INPUT_SIZE: u32 = 224;

/// Per-channel normalization (matches Falconsai `preprocessor_config.json`
/// `image_mean` — `ViTImageProcessor` default).
pub const PIXEL_MEAN: f32 = 0.5;
/// Per-channel normalization stddev (matches `image_std`).
pub const PIXEL_STD: f32 = 0.5;

/// Loaded NSFW classifier. Cheap to construct once at host startup; expensive
/// to construct repeatedly. Holds an `ort::Session` whose lifetime is tied to
/// the bundled model file plus the ONNX Runtime native library.
pub struct NsfwClassifier {
    session: Session,
}

impl NsfwClassifier {
    /// Load the model from a filesystem path. Used by callers (the host) that
    /// resolve the bundled model location at runtime via electron-builder's
    /// `extraResources` install layout.
    ///
    /// # Errors
    /// - [`ModerationError::Io`] if the file does not exist or is unreadable.
    /// - [`ModerationError::Ort`] if the file isn't a valid ONNX model the
    ///   bundled ORT version (1.24.x) can load.
    pub fn load(model_path: impl AsRef<Path>) -> Result<Self, ModerationError> {
        let path = model_path.as_ref();
        if !path.exists() {
            return Err(ModerationError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("model not found: {}", path.display()),
            )));
        }
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort::Error::from)?
            .commit_from_file(path)?;
        Ok(Self { session })
    }

    /// Run the classifier on a single image. `image_bytes` is the raw file
    /// body — the wrapper handles decode + preprocessing internally.
    ///
    /// Takes `&mut self` because `ort::Session::run` requires exclusive
    /// access. Callers that need concurrent inference (e.g. host's pack-time
    /// fan-out) should wrap the model in a `Mutex` — single-image inference on
    /// the int8/fp16-quantized ViT runs in ~80–200 ms on CPU.
    pub fn check(&mut self, image_bytes: &[u8]) -> Result<ModerationResult, ModerationError> {
        let input = preprocess(image_bytes)?;
        let outputs = self.session.run(inputs![Tensor::from_array(input)?])?;

        // Falconsai exports a single output tensor `logits` with shape [1, 2].
        // Consume by `into_iter()` rather than name-lookup so the wrapper is
        // resilient to exporter naming variation.
        let (_name, output_value) =
            outputs
                .into_iter()
                .next()
                .ok_or_else(|| ModerationError::UnexpectedShape {
                    expected: "1 output tensor",
                    got: vec![],
                })?;
        let output = output_value.try_extract_array::<f32>()?;
        let shape = output.shape().to_vec();
        if shape != [1, 2] {
            return Err(ModerationError::UnexpectedShape {
                expected: "[1, 2] (binary {normal, nsfw} logits)",
                got: shape,
            });
        }

        // Softmax over the 2 logits. id2label = {0: normal, 1: nsfw}; we
        // return p(nsfw) as `unsafe_score`. Implemented inline because it's
        // 2 floats — pulling in ndarray softmax helpers would be overkill.
        let l0 = output[[0, 0]];
        let l1 = output[[0, 1]];
        let max = l0.max(l1);
        let e0 = (l0 - max).exp();
        let e1 = (l1 - max).exp();
        let nsfw = e1 / (e0 + e1);

        // Label semantics: "nsfw" if the model leans nsfw at all (p > 0.5),
        // else "safe". The actual rejection decision uses the score against
        // the caller's threshold — the label is purely diagnostic for the
        // INV-7.7.6 detail strip.
        let label = if nsfw > 0.5 { "nsfw" } else { "safe" };

        Ok(ModerationResult {
            unsafe_score: nsfw,
            label: label.to_string(),
        })
    }
}

/// Decode + preprocess a single image to NCHW f32 [1, 3, INPUT_SIZE, INPUT_SIZE].
///
/// Matches Falconsai's `ViTImageProcessor` exactly:
/// 1. Decode (PNG/JPEG via `image` crate).
/// 2. Convert to RGB.
/// 3. Resize to (224, 224) bilinear — direct distortion, NO aspect padding.
/// 4. Per channel: `(px/255 - 0.5) / 0.5` → `[-1.0, 1.0]`. Equivalent to
///    `px/127.5 - 1.0` but written in the rescale+normalize form so the
///    relationship to the HF preprocessor config stays obvious.
fn preprocess(image_bytes: &[u8]) -> Result<Array4<f32>, ModerationError> {
    let img = image::load_from_memory(image_bytes)?.to_rgb8();
    let resized = image::imageops::resize(
        &img,
        INPUT_SIZE,
        INPUT_SIZE,
        image::imageops::FilterType::Triangle,
    );

    let size = INPUT_SIZE as usize;
    let mut tensor = Array4::<f32>::zeros((1, 3, size, size));
    let inv_std_255 = 1.0 / (PIXEL_STD * 255.0);
    let mean_term = PIXEL_MEAN / PIXEL_STD;
    for (x, y, pixel) in resized.enumerate_pixels() {
        let xs = x as usize;
        let ys = y as usize;
        let [r, g, b] = pixel.0;
        tensor[[0, 0, ys, xs]] = (r as f32) * inv_std_255 - mean_term;
        tensor[[0, 1, ys, xs]] = (g as f32) * inv_std_255 - mean_term;
        tensor[[0, 2, ys, xs]] = (b as f32) * inv_std_255 - mean_term;
    }
    Ok(tensor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderation_result_safe_default() {
        let safe = ModerationResult::safe();
        assert_eq!(safe.unsafe_score, 0.0);
        assert_eq!(safe.label, "safe");
    }

    #[test]
    fn preprocess_produces_correct_shape() {
        let png = include_bytes!("../tests/fixtures/clean-pixel.png");
        let arr = preprocess(png).expect("preprocess");
        assert_eq!(
            arr.shape(),
            &[1, 3, INPUT_SIZE as usize, INPUT_SIZE as usize]
        );
    }

    #[test]
    fn preprocess_normalizes_to_signed_unit_range() {
        // White pixel → (255/255 - 0.5)/0.5 = 1.0
        // Black pixel → (0/255 - 0.5)/0.5 = -1.0
        // Verify the constants resolve to the right end values.
        let inv_std_255 = 1.0 / (PIXEL_STD * 255.0);
        let mean_term = PIXEL_MEAN / PIXEL_STD;
        assert!((255.0 * inv_std_255 - mean_term - 1.0).abs() < 1e-6);
        assert!((0.0 * inv_std_255 - mean_term - (-1.0)).abs() < 1e-6);
    }
}
