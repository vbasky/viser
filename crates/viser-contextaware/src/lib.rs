//! Device-aware encoding ladder generation for the `viser` video-encoding-optimizer workspace.
//!
//! Runs per-title analysis tuned for each device class (Mobile, Desktop, TV, TV 4K), applying
//! per-class resolution caps, codec preferences, and VMAF model selection. `analyze` produces a
//! convex hull and bitrate ladder for every device `Profile`.

mod profile;

pub use profile::*;

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use viser_encoding::{Config as EncodingConfig, ProgressSender};
use viser_hull::{Hull, Point};
use viser_ladder::{self, Ladder};

/// Config for context-aware analysis.
#[derive(Debug, Clone)]
pub struct Config {
    /// Device profiles to analyze, each with its own resolution and codec constraints.
    pub profiles: Vec<Profile>,
    /// CRF quality values to sweep across all profiles.
    pub crf_values: Vec<i32>,
    /// Generic encoder preset name shared by all profiles.
    pub preset: String,
    /// Frame subsample interval for VMAF scoring; 0 evaluates every frame.
    pub subsample: i32,
    /// Number of concurrent encodes per profile analysis.
    pub parallel: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profiles: all_profiles(),
            crf_values: vec![18, 22, 26, 30, 34, 38, 42],
            preset: "veryfast".into(),
            subsample: 5,
            parallel: 2,
        }
    }
}

/// Analysis output for a single device profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceResult {
    /// The device profile this result was produced for.
    pub profile: Profile,
    /// Convex hull of quality-vs-bitrate points for the profile.
    pub hull: Hull,
    /// Selected bitrate ladder derived from the hull.
    pub ladder: Ladder,
    /// All evaluated encoding points.
    pub points: Vec<Point>,
    /// Number of encode trials run for this profile.
    pub trial_count: usize,
}

/// Combined result of a context-aware analysis across all device profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    /// Source video path that was analyzed.
    pub source: String,
    /// Per-device analysis results.
    pub devices: Vec<DeviceResult>,
    /// Total wall-clock time of the analysis.
    pub duration: Duration,
}

/// Progress update emitted as each device profile finishes.
#[derive(Debug, Clone)]
pub struct Progress {
    /// Number of profiles completed so far.
    pub device_done: usize,
    /// Total number of profiles to analyze.
    pub device_total: usize,
    /// Name of the most recently completed profile.
    pub device_name: String,
}

/// Runs per-title analysis for each device profile.
pub async fn analyze(
    source: &str,
    cfg: Config,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<Result> {
    let start = Instant::now();
    let mut devices = Vec::new();
    let sender = ProgressSender::new(progress_tx);

    for (i, profile) in cfg.profiles.iter().enumerate() {
        let pt_cfg = viser_pertitle::Config {
            encoding: EncodingConfig {
                resolutions: profile.resolutions.clone(),
                crf_values: cfg.crf_values.clone(),
                codecs: profile.codecs.clone(),
                preset: cfg.preset.clone(),
                subsample: cfg.subsample,
                parallel: cfg.parallel,
                ..Default::default()
            },
            ladder_opts: profile.ladder_opts.clone(),
            vmaf_model: profile.vmaf_model.clone(),
            checkpoint_path: String::new(),
            opt_metric: Default::default(),
            allow_hdr: false,
            ..Default::default()
        };

        let pt_result = viser_pertitle::analyze(source, pt_cfg, None)
            .await
            .map_err(|e| anyhow::anyhow!("analysis for {} failed: {e}", profile.name))?;

        let device_ladder = viser_ladder::select(&pt_result.hull, &profile.ladder_opts);

        devices.push(DeviceResult {
            profile: profile.clone(),
            hull: pt_result.hull,
            ladder: device_ladder,
            points: pt_result.points,
            trial_count: pt_result.trial_count,
        });

        sender.send(Progress {
            device_done: i + 1,
            device_total: cfg.profiles.len(),
            device_name: profile.name.clone(),
        });
    }

    Ok(Result { source: source.to_string(), devices, duration: start.elapsed() })
}
