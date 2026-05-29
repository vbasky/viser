mod cleanup;
mod progress;

pub use cleanup::*;
pub use progress::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::{Codec, RES_480P, RES_720P, RES_1080P, RateControlMode, Resolution};

/// Common encoding parameters shared across all optimization modes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub resolutions: Vec<Resolution>,
    pub crf_values: Vec<i32>,
    pub codecs: Vec<Codec>,
    pub preset: String,
    pub subsample: i32,
    pub parallel: i32,
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
            match codec {
                Codec::X264 | Codec::X265 | Codec::SvtAv1 => {}
            }
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
pub fn preset_for_codec(codec: Codec, preset: &str) -> String {
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
    fn test_config_serde_roundtrip() {
        let cfg = Config::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.resolutions.len(), cfg.resolutions.len());
        assert_eq!(back.crf_values, cfg.crf_values);
        assert_eq!(back.preset, cfg.preset);
    }
}
