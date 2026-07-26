//! ONNX model inference for ExtraTrees-based R-D point prediction.
//!
//! Replaces the heuristic `predict_point()` with a trained ExtraTrees regressor
//! exported to ONNX, providing more accurate bitrate/VMAF estimates.
//!
//! ## Model format
//!
//! The ONNX model takes a single float16/float32 input tensor of shape
//! `[1, 7]` with features in this order:
//!
//! | Index | Feature        | Range      | Description                |
//! |-------|----------------|------------|----------------------------|
//! | 0     | overall_score  | 0–100      | Composite complexity       |
//! | 1     | avg_spatial    | 0–1        | Normalised luma entropy    |
//! | 2     | avg_temporal   | 0–1        | Normalised motion          |
//! | 3     | log_pixels     | 10–20      | log2(width × height)       |
//! | 4     | codec_eff      | 0.5–1.0    | Codec efficiency factor    |
//! | 5     | crf_norm       | 0–1        | crf / 63                   |
//! | 6     | audio_bitrate  | 0–500      | Audio bitrate kbps         |
//!
//! Outputs two scalars: `log_bitrate` (log-transformed bitrate) and `vmaf` (0–100).

use anyhow::Context;
use viser_complexity::Profile;
use viser_ffmpeg::{Codec, CodecFamily, ProbeResult};

/// A loaded ONNX model ready for inference.
pub struct OnnxModel {
    model: tract_onnx::prelude::SimplePlan<
        tract_onnx::prelude::TypedFact,
        tract_onnx::prelude::BoxDynBackend,
    >,
}

impl OnnxModel {
    /// Load an ONNX model from a file path.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .context("failed to load ONNX model")?
            .with_input_fact(
                0,
                tract_onnx::prelude::InferenceFact::dt_shape(
                    tract_onnx::prelude::f32::tensor_type(),
                    tvec![1, 7],
                ),
            )
            .context("failed to set input shape")?
            .into_optimized()
            .context("failed to optimise ONNX model")?
            .into_runnable()
            .context("failed to compile ONNX model")?;
        Ok(Self { model })
    }

    /// Predict a single R-D point from complexity features.
    pub fn predict(
        &self,
        profile: &Profile,
        resolution: viser_ffmpeg::Resolution,
        codec: Codec,
        crf: i32,
        audio_bitrate_kbps: f64,
    ) -> anyhow::Result<(f64, f64)> {
        use tract_onnx::prelude::*;

        let complexity = profile.overall_score.clamp(0.0, 100.0) / 100.0;
        let spatial = profile.avg_spatial.clamp(0.0, 1.0);
        let temporal = (profile.avg_temporal / 75.0).clamp(0.0, 1.0);
        let log_pixels = ((resolution.width * resolution.height) as f64).log2();
        let codec_eff = match codec.family() {
            CodecFamily::H264 => 1.0,
            CodecFamily::H265 => 0.72,
            CodecFamily::Av1 => 0.58,
            CodecFamily::Vp9 => 0.65,
            CodecFamily::Other => 0.50,
        };
        let crf_norm = (crf as f64 / 63.0).clamp(0.0, 1.0);

        let input = tvec![
            complexity,
            spatial,
            temporal,
            log_pixels,
            codec_eff,
            crf_norm,
            audio_bitrate_kbps
        ];
        let tensor = tensor1(&input).into_shape(&[1, 7])?.into_tensor();

        let result = self.model.run(tvec![tensor.into()]).context("ONNX inference failed")?;

        let log_bitrate = result[0]
            .to_array_view::<f32>()
            .context("failed to read log_bitrate output")?[0] as f64;
        let vmaf =
            result[1].to_array_view::<f32>().context("failed to read vmaf output")?[0] as f64;

        let bitrate = (log_bitrate.exp().clamp(80.0, 50_000.0) * 100.0).round() / 100.0;
        let vmaf = vmaf.clamp(35.0, 99.0);

        Ok((bitrate, vmaf))
    }
}

/// Returns default path for the built-in ONNX model.
pub fn default_model_path() -> &'static str {
    // Uses the XGBoost/ExtraTrees model bundled with the crate at build time.
    // This can be overridden via --predict-model in the CLI.
    "./models/predictor.onnx"
}
