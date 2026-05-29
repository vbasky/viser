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
    pub profiles: Vec<Profile>,
    pub crf_values: Vec<i32>,
    pub preset: String,
    pub subsample: i32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceResult {
    pub profile: Profile,
    pub hull: Hull,
    pub ladder: Ladder,
    pub points: Vec<Point>,
    pub trial_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    pub source: String,
    pub devices: Vec<DeviceResult>,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub device_done: usize,
    pub device_total: usize,
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
            allow_hdr: false,
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
