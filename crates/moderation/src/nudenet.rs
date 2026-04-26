//! NudeNet ONNX detector wrapper.
//!
//! Loads `apps/desktop/resources/moderation/nudenet.onnx` (the v3.4 320n
//! detector — see crate-level doc) into an `ort::Session` and runs single-image
//! inference. Output is collapsed to one [`ModerationResult`]: max confidence
//! across all "exposed" classes plus the triggering label (or `"safe"` when no
//! unsafe class clears the lower-bound detection threshold).
//!
//! # Reference parser
//!
//! Preprocessing matches the upstream Python implementation
//! (`https://github.com/notAI-tech/NudeNet`, `nudenet/nudenet.py`):
//!
//! 1. Decode bytes (any format `image` supports — PNG, JPEG).
//! 2. Convert to RGB.
//! 3. Pad to a square with bottom/right zero padding (preserves aspect ratio
//!    without distorting features).
//! 4. Resize the padded square to `INPUT_SIZE x INPUT_SIZE` (320×320 for the
//!    320n model) using bilinear interpolation.
//! 5. Normalize to `[0.0, 1.0]` (`px / 255.0`) and lay out NCHW (batch=1,
//!    channels=3, H=320, W=320).
//!
//! Output tensor is `[1, 4 + num_classes, num_anchors]` (Ultralytics YOLO
//! style). The Python reference transposes + squeezes to
//! `[num_anchors, 4 + num_classes]`; we extract column-major directly so we
//! never materialise the transpose. For each row we look at columns
//! `[4..4+NUM_CLASSES]` (per-class confidence), find the max, and if it
//! clears [`DETECTION_THRESHOLD`] (0.2 — matches upstream) we keep it as a
//! candidate. The wrapper then reduces all candidate rows to a single
//! `(label, score)` pair: the global max across rows whose class index is in
//! [`UNSAFE_CLASS_INDICES`].
//!
//! Higher-level rejection threshold (INV-7.7.3 = 0.8) is applied by callers.

use std::path::Path;

use ndarray::Array4;
use ort::{
    inputs,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};

/// Square input dimension for the bundled 320n detector.
pub const INPUT_SIZE: u32 = 320;

/// Lower bound for keeping a row as a detection candidate before the
/// per-class reduction. Matches the upstream Python `_postprocess` constant.
pub const DETECTION_THRESHOLD: f32 = 0.2;

/// Class label list for the v3.4 NudeNet 320n detector. Order is
/// load-bearing — index = class id in the model's output tensor. See
/// `nudenet/nudenet.py` `__labels` (commit pinned in crate doc).
pub const CLASS_LABELS: [&str; 18] = [
    "FEMALE_GENITALIA_COVERED",
    "FACE_FEMALE",
    "BUTTOCKS_EXPOSED",
    "FEMALE_BREAST_EXPOSED",
    "FEMALE_GENITALIA_EXPOSED",
    "MALE_BREAST_EXPOSED",
    "ANUS_EXPOSED",
    "FEET_EXPOSED",
    "BELLY_COVERED",
    "FEET_COVERED",
    "ARMPITS_COVERED",
    "ARMPITS_EXPOSED",
    "FACE_MALE",
    "BELLY_EXPOSED",
    "MALE_GENITALIA_EXPOSED",
    "ANUS_COVERED",
    "FEMALE_BREAST_COVERED",
    "BUTTOCKS_COVERED",
];

/// Class indices that count as "unsafe" for moderation purposes. Matches the
/// EXPOSED variants from [`CLASS_LABELS`]. Coverage classes (FACE_*, *_COVERED,
/// FEET_EXPOSED, ARMPITS_EXPOSED, BELLY_EXPOSED) intentionally don't trigger —
/// they're informational, not policy violations.
pub const UNSAFE_CLASS_INDICES: [usize; 5] = [
    2,  // BUTTOCKS_EXPOSED
    3,  // FEMALE_BREAST_EXPOSED
    4,  // FEMALE_GENITALIA_EXPOSED
    6,  // ANUS_EXPOSED
    14, // MALE_GENITALIA_EXPOSED
];

/// Result of a single moderation check.
///
/// `unsafe_score` is the maximum per-anchor confidence across the
/// [`UNSAFE_CLASS_INDICES`] set. `label` is the corresponding class name from
/// [`CLASS_LABELS`], or `"safe"` if no unsafe-class detection cleared
/// [`DETECTION_THRESHOLD`].
///
/// Callers compare `unsafe_score` against their own threshold (INV-7.7.3 =
/// 0.8 in the Omni upload pipeline).
#[derive(Debug, Clone, PartialEq)]
pub struct ModerationResult {
    pub unsafe_score: f32,
    pub label: String,
}

impl ModerationResult {
    /// Convenience constructor for the "no unsafe detection" case.
    pub fn safe() -> Self {
        Self { unsafe_score: 0.0, label: "safe".to_string() }
    }
}

/// Errors raised by [`NudeNetModel`].
#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    /// ORT failure (model load, session run, tensor extraction).
    #[error("ort: {0}")]
    Ort(#[from] ort::Error),

    /// Image decode/format failure.
    #[error("image decode: {0}")]
    Image(#[from] image::ImageError),

    /// File I/O failure (reading model from disk).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Output tensor had an unexpected shape — usually means the bundled
    /// model file isn't a NudeNet-format detector. `expected` describes the
    /// shape pattern we wanted; `got` is the actual shape from the model.
    #[error("unexpected output tensor shape: expected {expected}, got {got:?}")]
    UnexpectedShape { expected: &'static str, got: Vec<usize> },
}

/// Loaded NudeNet model. Cheap to construct once at host startup; expensive
/// to construct repeatedly. Holds an `ort::Session` whose lifetime is tied to
/// the bundled model file plus the ONNX Runtime native library.
pub struct NudeNetModel {
    session: Session,
}

impl NudeNetModel {
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
        // Surface a clear IO error before letting ORT report a less friendly
        // "model load failed" — file-missing is the most common deployment
        // misconfiguration.
        if !path.exists() {
            return Err(ModerationError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("model not found: {}", path.display()),
            )));
        }

        // `with_optimization_level` returns `Result<_, ort::Error<SessionBuilder>>`
        // — convert to the type-erased `ort::Error<()>` (which is what the
        // public `ort::Error` alias points at) before propagating via `?`.
        // ort already implements the necessary `From` between the two.
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort::Error::from)?
            .commit_from_file(path)?;
        Ok(Self { session })
    }

    /// Run NudeNet on a single image. `image_bytes` is the raw file body —
    /// the wrapper handles decode + preprocessing internally.
    ///
    /// Returns the highest-confidence unsafe-class detection, or
    /// [`ModerationResult::safe`] if nothing crossed [`DETECTION_THRESHOLD`].
    ///
    /// Takes `&mut self` because `ort::Session::run` requires exclusive
    /// access. Callers that need concurrent inference (e.g. host's pack-time
    /// fan-out in OWI-51) should wrap the model in a `Mutex` — single-image
    /// inference on the bundled detector is fast enough (~50–150 ms on CPU
    /// for 320×320) that contention is acceptable.
    pub fn check(&mut self, image_bytes: &[u8]) -> Result<ModerationResult, ModerationError> {
        let input = preprocess(image_bytes)?;
        let outputs = self.session.run(inputs![Tensor::from_array(input)?])?;

        // NudeNet has a single output. Consuming `into_iter()` gives us
        // `(name, DynValue)` — easier than name-based lookup which varies
        // between exporters.
        let (_name, output_value) = outputs.into_iter().next().ok_or_else(|| {
            ModerationError::UnexpectedShape { expected: "1 output tensor", got: vec![] }
        })?;
        let output = output_value.try_extract_array::<f32>()?;

        // Expected shape: [1, 4 + NUM_CLASSES, num_anchors]. NudeNet's 320n
        // detector exports as YOLO Ultralytics style (channels = 22).
        let shape = output.shape().to_vec();
        let (num_channels, num_anchors) = match shape.as_slice() {
            [1, c, a] => (*c, *a),
            _ => {
                return Err(ModerationError::UnexpectedShape {
                    expected: "[1, 4 + num_classes, num_anchors]",
                    got: shape,
                });
            }
        };
        let expected_channels = 4 + CLASS_LABELS.len();
        if num_channels != expected_channels {
            return Err(ModerationError::UnexpectedShape {
                expected: "channels = 4 + 18 (NudeNet detector)",
                got: shape,
            });
        }

        // Reduce across anchors, looking only at unsafe-class columns.
        // Output layout is [batch=1][channel][anchor], so the per-class score
        // for class `c` at anchor `a` is `output[[0, 4 + c, a]]`.
        let mut best_score = 0.0_f32;
        let mut best_class: Option<usize> = None;
        for &class_idx in &UNSAFE_CLASS_INDICES {
            let channel = 4 + class_idx;
            for anchor in 0..num_anchors {
                let score = output[[0, channel, anchor]];
                if score >= DETECTION_THRESHOLD && score > best_score {
                    best_score = score;
                    best_class = Some(class_idx);
                }
            }
        }

        Ok(match best_class {
            Some(idx) => ModerationResult {
                unsafe_score: best_score,
                label: CLASS_LABELS[idx].to_string(),
            },
            None => ModerationResult::safe(),
        })
    }
}

/// Decode + preprocess a single image to NCHW f32 [1, 3, INPUT_SIZE, INPUT_SIZE].
///
/// Matches upstream NudeNet preprocessing:
/// 1. Decode (PNG/JPEG via `image` crate).
/// 2. Convert to RGB (`swapRB=True` in cv2.dnn.blobFromImage on a BGR cv2
///    decode reads in RGB order — same end state).
/// 3. Pad to square with bottom/right zero padding.
/// 4. Resize to (INPUT_SIZE, INPUT_SIZE) bilinear.
/// 5. Normalize to [0.0, 1.0] and emit NCHW.
fn preprocess(image_bytes: &[u8]) -> Result<Array4<f32>, ModerationError> {
    let img = image::load_from_memory(image_bytes)?.to_rgb8();
    let (w, h) = (img.width(), img.height());
    let max_dim = w.max(h);

    // Pad to square, bottom-right zero fill (matches `cv2.copyMakeBorder` with
    // top=0, left=0, bottom=y_pad, right=x_pad).
    let mut square = image::RgbImage::new(max_dim, max_dim);
    image::imageops::overlay(&mut square, &img, 0, 0);

    // Resize to model input. Triangle = bilinear; NudeNet's
    // `cv2.dnn.blobFromImage` defaults to bilinear so we match.
    let resized = image::imageops::resize(
        &square,
        INPUT_SIZE,
        INPUT_SIZE,
        image::imageops::FilterType::Triangle,
    );

    // NCHW f32 normalised to [0, 1].
    let size = INPUT_SIZE as usize;
    let mut tensor = Array4::<f32>::zeros((1, 3, size, size));
    for (x, y, pixel) in resized.enumerate_pixels() {
        let xs = x as usize;
        let ys = y as usize;
        let [r, g, b] = pixel.0;
        tensor[[0, 0, ys, xs]] = r as f32 / 255.0;
        tensor[[0, 1, ys, xs]] = g as f32 / 255.0;
        tensor[[0, 2, ys, xs]] = b as f32 / 255.0;
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
    fn class_labels_count_matches_unsafe_indices_bound() {
        for &idx in &UNSAFE_CLASS_INDICES {
            assert!(idx < CLASS_LABELS.len(), "unsafe index {idx} out of range");
        }
    }

    #[test]
    fn preprocess_produces_correct_shape() {
        // 1x1 PNG. Tiny smoke that the decode path is wired.
        let png = include_bytes!("../tests/fixtures/clean-pixel.png");
        let arr = preprocess(png).expect("preprocess");
        assert_eq!(arr.shape(), &[1, 3, INPUT_SIZE as usize, INPUT_SIZE as usize]);
    }
}
