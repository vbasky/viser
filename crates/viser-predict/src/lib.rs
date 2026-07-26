//! Feature-based bitrate ladder prediction without trial encodes.
//!
//! Extracts spatial/temporal complexity features from the source, predicts
//! rate-distortion points for each (resolution, codec, CRF) combination using
//! calibrated heuristics (or an ONNX model when the `onnx` feature is enabled),
//! then runs the standard hull + ladder selection path.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use viser_complexity::{AnalyzeOpts, Profile};
use viser_ffmpeg::{Codec, ProbeResult, Resolution, probe};
use viser_hull::{Hull, Point, compute_per_codec, compute_upper};
use viser_ladder::Ladder;
use viser_pertitle::Config;

#[cfg(feature = "onnx")]
mod onnx;

/// Output of a prediction run — same shape as per-title analysis but marked predicted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    pub source: String,
    pub source_info: ProbeResult,
    pub config: Config,
    pub complexity: Profile,
    pub points: Vec<Point>,
    pub hull: Hull,
    pub per_codec: std::collections::HashMap<Codec, Hull>,
    pub ladder: Ladder,
    pub duration: Duration,
    pub predicted: bool,
    pub warnings: Vec<String>,
    /// Path to the ONNX model used (empty when heuristics were used).
    #[serde(default)]
    pub model_path: String,
}

/// Predicts a per-title ladder from content features (no encodes).
///
/// When the `onnx` feature is enabled and `model_path` is non-empty, uses a
/// trained ONNX model for R-D point prediction. Falls back to heuristic
/// `predict_point()` when no model is provided.
pub async fn predict(source: &str, cfg: Config, model_path: &str) -> anyhow::Result<Result> {
    let start = Instant::now();
    cfg.encoding.validate()?;

    let source_info = probe(source).await?;
    let complexity = viser_complexity::analyze(
        source,
        AnalyzeOpts { segment_duration: Duration::from_secs(2), subsample: 5 },
    )
    .await?;

    let audio_bitrate_kbps =
        source_info.audio_stream().map(|a| a.bit_rate as f64 / 1000.0).unwrap_or(0.0);

    let mut points = Vec::new();
    let (use_onnx, warnings, model_path) = try_load_onnx(model_path).await;

    for &resolution in &cfg.encoding.resolutions {
        for &codec in &cfg.encoding.codecs {
            for &crf in &cfg.encoding.crf_values {
                let (bitrate, vmaf) = predict_point_or_onnx(
                    &use_onnx,
                    &complexity,
                    resolution,
                    codec,
                    crf,
                    audio_bitrate_kbps,
                );
                points.push(Point { resolution, codec, crf, bitrate, vmaf, psnr: 0.0, ssim: 0.0 });
            }
        }
    }

    points.sort_by(|a, b| a.bitrate.partial_cmp(&b.bitrate).unwrap_or(std::cmp::Ordering::Equal));

    let hull = compute_upper(&points);
    let per_codec = compute_per_codec(&points);
    let mut ladder_opts = cfg.ladder_opts.clone();
    ladder_opts.audio_bitrate_kbps = audio_bitrate_kbps;
    let ladder = viser_ladder::select(&hull, &ladder_opts);

    Ok(Result {
        source: source.to_string(),
        source_info,
        config: cfg,
        complexity,
        points,
        hull,
        per_codec,
        ladder,
        duration: start.elapsed(),
        predicted: true,
        warnings,
        model_path,
    })
}

/// Tries to load an ONNX model from the given path.
///
/// Returns `(Some(model), warnings, model_path)` on success, or `(None, heuristic_warning, "")`.
async fn try_load_onnx(model_path: &str) -> (Option<OnnxModel>, Vec<String>, String) {
    #[cfg(not(feature = "onnx"))]
    let _ = model_path;
    #[cfg(feature = "onnx")]
    if !model_path.is_empty() {
        match onnx::OnnxModel::load(model_path) {
            Ok(model) => {
                let warnings = vec![format!("R-D points predicted by ONNX model: {model_path}")];
                return (Some(OnnxModel::Onnx(model)), warnings, model_path.to_string());
            }
            Err(e) => {
                let warnings = vec![
                    format!("failed to load ONNX model '{model_path}': {e}; falling back to heuristics"),
                    "ladder predicted from complexity features; validate with per-title analyze before delivery".into(),
                ];
                return (None, warnings, String::new());
            }
        }
    }

    (None, vec!["ladder predicted from complexity features; validate with per-title analyze before delivery".into()], String::new())
}

/// Internal wrapper to avoid exposing tract types in the public API.
enum OnnxModel {
    #[cfg(feature = "onnx")]
    #[allow(dead_code)]
    Onnx(onnx::OnnxModel),
}

/// Predicts an R-D point, using the ONNX model when available.
fn predict_point_or_onnx(
    model: &Option<OnnxModel>,
    profile: &Profile,
    resolution: Resolution,
    codec: Codec,
    crf: i32,
    audio_bitrate_kbps: f64,
) -> (f64, f64) {
    #[cfg(not(feature = "onnx"))]
    {
        let _ = (model, audio_bitrate_kbps);
    }
    #[cfg(feature = "onnx")]
    if let Some(OnnxModel::Onnx(ref m)) = model {
        if let Ok(result) = m.predict(profile, resolution, codec, crf, audio_bitrate_kbps) {
            return result;
        }
    }
    predict_point(profile, resolution, codec, crf)
}

/// Heuristic R-D point prediction from complexity features.
///
/// Calibrated against typical VOD convex-hull shapes: complexity raises bitrate
/// at fixed quality and lowers achievable VMAF at fixed CRF.
pub fn predict_point(profile: &Profile, res: Resolution, codec: Codec, crf: i32) -> (f64, f64) {
    let pixels = (res.width * res.height).max(1) as f64;
    let complexity = profile.overall_score.clamp(0.0, 100.0) / 100.0;
    let spatial = profile.avg_spatial.clamp(0.0, 1.0);
    let temporal = (profile.avg_temporal / 75.0).clamp(0.0, 1.0);

    let codec_eff = match codec.family() {
        viser_ffmpeg::CodecFamily::H264 => 1.0,
        viser_ffmpeg::CodecFamily::H265 => 0.72,
        viser_ffmpeg::CodecFamily::Av1 => 0.58,
        viser_ffmpeg::CodecFamily::Vp9 => 0.65,
        viser_ffmpeg::CodecFamily::Other => 0.50,
    };

    let crf_factor = 2.0_f64.powf((23.0 - crf as f64) / 6.0);
    let motion_factor = 1.0 + temporal * 0.8;
    let detail_factor = 1.0 + spatial * 0.5;

    let base_kbps =
        pixels * 0.00012 * complexity * crf_factor * motion_factor * detail_factor / codec_eff;
    let bitrate = base_kbps.clamp(80.0, 50_000.0);

    let vmaf =
        (98.0 - (crf as f64 - 16.0) * 1.15 - complexity * 12.0 - temporal * 6.0).clamp(35.0, 99.0);

    (bitrate, vmaf)
}

impl Result {
    pub fn save_json(&self, path: &str) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::{RES_720P, RES_1080P};

    fn sample_profile(score: f64, spatial: f64, temporal: f64) -> Profile {
        Profile {
            frames: vec![],
            segments: vec![],
            avg_spatial: spatial,
            avg_temporal: temporal,
            overall_score: score,
        }
    }

    #[test]
    fn higher_complexity_raises_bitrate() {
        let easy = sample_profile(20.0, 0.55, 5.0);
        let hard = sample_profile(80.0, 0.85, 40.0);
        let (br_easy, _) = predict_point(&easy, RES_1080P, Codec::X264, 23);
        let (br_hard, _) = predict_point(&hard, RES_1080P, Codec::X264, 23);
        assert!(br_hard > br_easy);
    }

    #[test]
    fn lower_crf_improves_vmaf() {
        let p = sample_profile(50.0, 0.7, 15.0);
        let (_, vmaf_high) = predict_point(&p, RES_720P, Codec::X265, 18);
        let (_, vmaf_low) = predict_point(&p, RES_720P, Codec::X265, 32);
        assert!(vmaf_high > vmaf_low);
    }
}
