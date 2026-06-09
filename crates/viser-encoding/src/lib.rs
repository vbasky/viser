//! Shared encoding configuration for the `viser` video-encoding-optimizer workspace.
//!
//! Provides the common `Config` of encoding parameters used across all optimization modes,
//! codec-specific preset mapping (`preset_for_codec`), non-blocking progress reporting, and
//! cleanup of orphaned temp directories left behind by crashes.

mod cleanup;
mod progress;

pub use cleanup::*;
pub use progress::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::{
    Codec, EncoderBackend, RES_480P, RES_720P, RES_1080P, RateControlMode, Resolution,
};

/// Common encoding parameters shared across all optimization modes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Output resolutions to encode and evaluate.
    pub resolutions: Vec<Resolution>,
    /// CRF (constant rate factor) quality values to sweep.
    pub crf_values: Vec<i32>,
    /// Codecs to encode with.
    pub codecs: Vec<Codec>,
    /// Generic preset name (e.g. `"veryfast"`); mapped per codec via `preset_for_codec`.
    pub preset: String,
    /// Frame subsample interval for VMAF scoring; 0 evaluates every frame.
    pub subsample: i32,
    /// Number of concurrent encodes; 0 means auto (see `effective_parallel`).
    pub parallel: i32,
    /// Rate control mode (e.g. CRF) used for encoding.
    pub rate_control: RateControlMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            resolutions: vec![RES_480P, RES_720P, RES_1080P],
            crf_values: vec![18, 22, 26, 30, 34, 38, 42],
            codecs: vec![Codec::X264],
            preset: "veryfast".into(),
            subsample: 5,
            parallel: 0,
            rate_control: RateControlMode::Crf,
        }
    }
}

impl Config {
    /// Validates the configuration, returning an error if any field is empty or out of range.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.resolutions.is_empty() {
            anyhow::bail!("must specify at least one resolution");
        }
        if self.crf_values.is_empty() {
            anyhow::bail!("must specify at least one CRF value");
        }
        if self.codecs.is_empty() {
            anyhow::bail!("must specify at least one codec");
        }
        for codec in &self.codecs {
            let _ = codec;
        }
        if self.subsample < 0 {
            anyhow::bail!("subsample must be >= 0, got {}", self.subsample);
        }
        Ok(())
    }

    /// Returns the actual parallelism to use.
    /// If parallel is 0, uses num_cpus/2 with a floor of 2.
    pub fn effective_parallel(&self) -> usize {
        if self.parallel > 0 {
            return self.parallel as usize;
        }
        let p = num_cpus() / 2;
        p.max(2)
    }
}

/// Maps a generic preset name to codec-specific presets.
///
/// Software encoders get passthrough (except SVT-AV1 which maps to numeric presets).
/// Hardware encoders get per-backend preset mapping:
/// - NVENC: p1..p7
/// - QSV: passthrough (veryfast..veryslow)
/// - VideoToolbox: realtime flag for fast presets
/// - VAAPI: compression_level 1..5
/// - AMF: speed/balanced/quality
pub fn preset_for_codec(codec: Codec, preset: &str) -> String {
    if codec.is_hardware() {
        return hw_preset_for_codec(codec, preset);
    }
    if codec != Codec::SvtAv1 {
        return preset.to_string();
    }
    match preset {
        "ultrafast" => "12",
        "superfast" => "11",
        "veryfast" => "10",
        "faster" => "9",
        "fast" => "8",
        "medium" => "6",
        "slow" => "4",
        "slower" => "2",
        "veryslow" => "0",
        other => return other.to_string(),
    }
    .to_string()
}

fn hw_preset_for_codec(codec: Codec, preset: &str) -> String {
    match codec.backend() {
        EncoderBackend::Nvenc => match preset {
            "ultrafast" | "superfast" => "p1".into(),
            "veryfast" => "p2".into(),
            "faster" => "p3".into(),
            "fast" => "p4".into(),
            "medium" => "p5".into(),
            "slow" => "p6".into(),
            "slower" | "veryslow" => "p7".into(),
            other => other.to_string(),
        },
        EncoderBackend::Qsv => preset.to_string(),
        EncoderBackend::Vaapi => match preset {
            "ultrafast" | "superfast" => "1".into(),
            "veryfast" | "faster" => "2".into(),
            "fast" | "medium" => "3".into(),
            "slow" => "4".into(),
            "slower" | "veryslow" => "5".into(),
            other => other.to_string(),
        },
        EncoderBackend::Amf => match preset {
            "ultrafast" | "superfast" => "speed".into(),
            "veryfast" | "faster" | "fast" => "balanced".into(),
            "medium" | "slow" | "slower" | "veryslow" => "quality".into(),
            other => other.to_string(),
        },
        EncoderBackend::VideoToolbox => preset.to_string(),
        EncoderBackend::Software => preset.to_string(),
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::{Codec, RateControlMode};

    #[test]
    fn test_config_default() {
        let cfg = Config::default();
        assert_eq!(cfg.resolutions.len(), 3);
        assert_eq!(cfg.crf_values.len(), 7);
        assert_eq!(cfg.codecs.len(), 1);
        assert_eq!(cfg.preset, "veryfast");
        assert_eq!(cfg.subsample, 5);
        assert_eq!(cfg.rate_control, RateControlMode::Crf);
    }

    #[test]
    fn test_config_validate_ok() {
        let cfg = Config::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_config_validate_empty_resolutions() {
        let cfg = Config { resolutions: vec![], ..Config::default() };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_empty_crf() {
        let cfg = Config { crf_values: vec![], ..Config::default() };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_empty_codecs() {
        let cfg = Config { codecs: vec![], ..Config::default() };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_negative_subsample() {
        let cfg = Config { subsample: -1, ..Config::default() };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_zero_subsample_ok() {
        let cfg = Config { subsample: 0, ..Config::default() };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_effective_parallel_uses_explicit() {
        let cfg = Config { parallel: 8, ..Config::default() };
        assert_eq!(cfg.effective_parallel(), 8);
    }

    #[test]
    fn test_effective_parallel_auto() {
        let cfg = Config { parallel: 0, ..Config::default() };
        let p = cfg.effective_parallel();
        assert!(p >= 2);
    }

    #[test]
    fn test_preset_for_codec_passthrough() {
        assert_eq!(preset_for_codec(Codec::X264, "veryfast"), "veryfast");
        assert_eq!(preset_for_codec(Codec::X265, "slow"), "slow");
    }

    #[test]
    fn test_preset_for_codec_svtav1_maps() {
        assert_eq!(preset_for_codec(Codec::SvtAv1, "ultrafast"), "12");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "superfast"), "11");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "veryfast"), "10");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "faster"), "9");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "fast"), "8");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "medium"), "6");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "slow"), "4");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "slower"), "2");
        assert_eq!(preset_for_codec(Codec::SvtAv1, "veryslow"), "0");
    }

    #[test]
    fn test_preset_for_codec_svtav1_passthrough_unknown() {
        assert_eq!(preset_for_codec(Codec::SvtAv1, "custom"), "custom");
    }

    #[test]
    fn test_preset_for_codec_nvenc_maps() {
        assert_eq!(preset_for_codec(Codec::NvencH264, "ultrafast"), "p1");
        assert_eq!(preset_for_codec(Codec::NvencH264, "medium"), "p5");
        assert_eq!(preset_for_codec(Codec::NvencH264, "veryslow"), "p7");
    }

    #[test]
    fn test_preset_for_codec_qsv_passthrough() {
        assert_eq!(preset_for_codec(Codec::QsvH264, "veryfast"), "veryfast");
        assert_eq!(preset_for_codec(Codec::QsvH264, "medium"), "medium");
    }

    #[test]
    fn test_preset_for_codec_vaapi_maps() {
        assert_eq!(preset_for_codec(Codec::VaapiH264, "ultrafast"), "1");
        assert_eq!(preset_for_codec(Codec::VaapiH264, "medium"), "3");
        assert_eq!(preset_for_codec(Codec::VaapiH264, "veryslow"), "5");
    }

    #[test]
    fn test_preset_for_codec_amf_maps() {
        assert_eq!(preset_for_codec(Codec::AmfH264, "ultrafast"), "speed");
        assert_eq!(preset_for_codec(Codec::AmfH264, "fast"), "balanced");
        assert_eq!(preset_for_codec(Codec::AmfH264, "medium"), "quality");
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let cfg = Config::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.resolutions.len(), cfg.resolutions.len());
        assert_eq!(back.crf_values, cfg.crf_values);
        assert_eq!(back.preset, cfg.preset);
    }
}
