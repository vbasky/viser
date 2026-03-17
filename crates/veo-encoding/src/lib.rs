mod cleanup;
mod progress;

pub use cleanup::*;
pub use progress::*;

use serde::{Deserialize, Serialize};
use veo_ffmpeg::{Codec, RateControlMode, Resolution, RES_480P, RES_720P, RES_1080P};

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
    }.to_string()
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
