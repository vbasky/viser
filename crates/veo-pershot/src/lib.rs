mod trellis;

pub use trellis::*;

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use veo_encoding::ProgressSender;
use veo_ffmpeg::{extract, Codec, Resolution};
use veo_hull::{Hull, Point};
use veo_ladder::Opts as LadderOpts;
use veo_shot::{self, DetectOpts, Shot};

/// Config defines parameters for per-shot analysis.
#[derive(Debug, Clone)]
pub struct Config {
    pub encoding: veo_encoding::Config,
    pub shot_opts: DetectOpts,
    pub ladder_opts: LadderOpts,
}

impl Default for Config {
    fn default() -> Self {
        let mut enc = veo_encoding::Config::default();
        enc.crf_values = vec![22, 26, 30, 34, 38];
        Self {
            encoding: enc,
            shot_opts: DetectOpts::default(),
            ladder_opts: LadderOpts::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotResult {
    pub shot: Shot,
    pub points: Vec<Point>,
    pub hull: Hull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    pub source: String,
    pub shots: Vec<ShotResult>,
    pub duration: Duration,
    pub shot_count: usize,
    pub trial_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<TrellisAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrellisAssignment {
    pub shot_index: usize,
    pub resolution: Resolution,
    pub codec: Codec,
    pub crf: i32,
    pub bitrate: f64,
    pub vmaf: f64,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub shot_done: usize,
    pub shot_total: usize,
    pub shot_index: usize,
}

/// Runs per-shot analysis: detect shots, analyze each independently.
pub async fn analyze(
    source: &str,
    cfg: Config,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<Result> {
    let start = Instant::now();

    // Step 1: Detect shots
    let shots = veo_shot::detect(source, cfg.shot_opts).await?;
    let sender = ProgressSender::new(progress_tx);

    // Step 2: Create temp directory
    let tmp_dir = tempfile::Builder::new().prefix("veo-pershot-").tempdir()?;

    // Step 3: Analyze each shot
    let mut shot_results = Vec::new();
    let mut total_trials = 0;

    for (i, s) in shots.iter().enumerate() {
        let seg_path = tmp_dir.path().join(format!("shot_{i:03}.mkv"));
        let seg_str = seg_path.to_string_lossy().to_string();

        extract(source, &seg_str, s.start.as_secs_f64(), s.duration.as_secs_f64()).await?;

        let shot_cfg = veo_pertitle::Config {
            encoding: cfg.encoding.clone(),
            ladder_opts: cfg.ladder_opts.clone(),
            checkpoint_path: String::new(),
            vmaf_model: String::new(),
        };

        let shot_analysis = veo_pertitle::analyze(&seg_str, shot_cfg, None).await?;

        shot_results.push(ShotResult {
            shot: s.clone(),
            points: shot_analysis.points,
            hull: shot_analysis.hull,
        });
        total_trials += shot_analysis.trial_count;

        let _ = std::fs::remove_file(&seg_path);

        sender.send(Progress {
            shot_done: i + 1,
            shot_total: shots.len(),
            shot_index: i,
        });
    }

    Ok(Result {
        source: source.to_string(),
        shots: shot_results,
        duration: start.elapsed(),
        shot_count: shots.len(),
        trial_count: total_trials,
        assignments: vec![],
    })
}
