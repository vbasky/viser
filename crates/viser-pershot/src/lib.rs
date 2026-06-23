//! Per-shot encoding with Trellis bit allocation for the `viser`
//! video-encoding-optimizer workspace.
//!
//! Detects shot boundaries, runs an independent per-title analysis on each shot, then
//! optimizes the distribution of bits across shots using a Lagrangian constant-slope
//! search. Use `analyze` to produce per-shot hulls and `trellis_optimize` to derive the
//! optimal per-shot encoding assignments for a target bitrate.

mod trellis;

pub use trellis::*;

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use viser_encoding::ProgressSender;
use viser_ffmpeg::{Codec, Resolution, extract};
use viser_hull::{Hull, Point};
use viser_ladder::Opts as LadderOpts;
use viser_shot::{self, DetectOpts, Shot};

/// Config defines parameters for per-shot analysis.
#[derive(Debug, Clone)]
pub struct Config {
    /// Encoding search space applied to each shot's per-title analysis.
    pub encoding: viser_encoding::Config,
    /// Shot-detection options (e.g. scene-change threshold, minimum duration).
    pub shot_opts: DetectOpts,
    /// Ladder selection options forwarded to the per-shot analysis.
    pub ladder_opts: LadderOpts,
    /// VMAF model name passed to the quality measurement; empty uses the default model.
    pub vmaf_model: String,
    /// Quality metric optimized along each shot's hull. VMAF (the default) is the most
    /// accurate but slowest; PSNR/SSIM use native FFmpeg filters and run far faster.
    pub opt_metric: viser_quality::Metric,
    /// Allow best-effort analysis of HDR sources instead of bailing out.
    pub allow_hdr: bool,
    /// How HDR/high bit-depth sources are prepared before quality scoring.
    pub hdr_scoring: viser_quality::HdrScoringMode,
}

impl Default for Config {
    fn default() -> Self {
        let mut enc = viser_encoding::Config::default();
        enc.crf_values = vec![22, 26, 30, 34, 38];
        Self {
            encoding: enc,
            shot_opts: DetectOpts::default(),
            ladder_opts: LadderOpts::default(),
            vmaf_model: String::new(),
            opt_metric: viser_quality::Metric::default(),
            allow_hdr: false,
            hdr_scoring: viser_quality::HdrScoringMode::Auto,
        }
    }
}

/// Per-title analysis result for a single detected shot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotResult {
    /// The detected shot (boundaries and metadata).
    pub shot: Shot,
    /// All measured trial points for this shot.
    pub points: Vec<Point>,
    /// Convex upper hull of this shot's points.
    pub hull: Hull,
}

/// Complete output of a per-shot analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    /// Path to the analyzed source video.
    pub source: String,
    /// The quality metric carried by each shot `Point`'s `vmaf` axis. VMAF unless
    /// `--metric` selected PSNR/SSIM; recorded so consumers read the scores correctly.
    #[serde(default)]
    pub metric: viser_quality::Metric,
    /// Per-shot analysis results in shot order.
    pub shots: Vec<ShotResult>,
    /// Wall-clock duration of the full per-shot analysis.
    pub duration: Duration,
    /// Number of detected shots.
    pub shot_count: usize,
    /// Total number of trials across all shots.
    pub trial_count: usize,
    /// Optional Trellis bit-allocation assignments (empty until optimized).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<TrellisAssignment>,
}

/// Optimal encoding assignment for a single shot from Trellis optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrellisAssignment {
    /// Index of the shot this assignment applies to.
    pub shot_index: usize,
    /// Chosen output resolution.
    pub resolution: Resolution,
    /// Chosen codec.
    pub codec: Codec,
    /// Chosen CRF value.
    pub crf: i32,
    /// Expected bitrate at this assignment.
    pub bitrate: f64,
    /// Expected VMAF at this assignment.
    pub vmaf: f64,
}

/// Progress update emitted as each shot finishes its analysis.
#[derive(Debug, Clone)]
pub struct Progress {
    /// Number of shots analyzed so far.
    pub shot_done: usize,
    /// Total number of shots to analyze.
    pub shot_total: usize,
    /// Index of the shot that just completed.
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
    let shots = viser_shot::detect(source, cfg.shot_opts).await?;
    let sender = ProgressSender::new(progress_tx);

    // Step 2: Create temp directory
    let tmp_dir = tempfile::Builder::new().prefix("viser-pershot-").tempdir()?;

    // Step 3: Extract shot segments, then analyze in parallel
    let parallel = cfg.encoding.effective_parallel().min(shots.len());
    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallel));
    let source = Arc::new(source.to_string());
    let sender = Arc::new(sender);

    let mut segment_paths = Vec::new();
    for (i, s) in shots.iter().enumerate() {
        let seg_path = tmp_dir.path().join(format!("shot_{i:03}.mkv"));
        let seg_str = seg_path.to_string_lossy().to_string();
        extract(&source, &seg_str, s.start.as_secs_f64(), s.duration.as_secs_f64()).await?;
        segment_paths.push((i, seg_path, seg_str, s.clone()));
    }

    let mut set = tokio::task::JoinSet::new();
    for (i, seg_path, seg_str, s) in segment_paths {
        let sem = semaphore.clone();
        let shot_cfg = viser_pertitle::Config {
            encoding: cfg.encoding.clone(),
            ladder_opts: cfg.ladder_opts.clone(),
            checkpoint_path: String::new(),
            vmaf_model: cfg.vmaf_model.clone(),
            opt_metric: cfg.opt_metric,
            allow_hdr: cfg.allow_hdr,
            hdr_scoring: cfg.hdr_scoring,
        };
        let sender = sender.clone();
        let shots_len = shots.len();

        set.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed unexpectedly");
            let shot_analysis = viser_pertitle::analyze(&seg_str, shot_cfg, None).await?;
            let _ = std::fs::remove_file(&seg_path);
            sender.send(Progress { shot_done: i + 1, shot_total: shots_len, shot_index: i });
            Ok::<_, anyhow::Error>((
                i,
                ShotResult { shot: s, points: shot_analysis.points, hull: shot_analysis.hull },
                shot_analysis.trial_count,
            ))
        });
    }

    let mut shot_results: Vec<Option<ShotResult>> = vec![None; shots.len()];
    let mut total_trials = 0;
    while let Some(res) = set.join_next().await {
        let (i, result, trials) = res??;
        shot_results[i] = Some(result);
        total_trials += trials;
    }
    let shot_results: Vec<ShotResult> = shot_results.into_iter().flatten().collect();

    Ok(Result {
        source: source.to_string(),
        metric: cfg.opt_metric,
        shots: shot_results,
        duration: start.elapsed(),
        shot_count: shots.len(),
        trial_count: total_trials,
        assignments: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let cfg = Config::default();
        assert!(!cfg.allow_hdr);
        assert!(cfg.vmaf_model.is_empty());
        assert_eq!(cfg.encoding.crf_values, vec![22, 26, 30, 34, 38]);
    }

    #[test]
    fn test_config_can_set_hdr_and_vmaf_model() {
        let cfg = Config { vmaf_model: "vmaf_v0.6.1".into(), allow_hdr: true, ..Config::default() };
        assert!(cfg.allow_hdr);
        assert_eq!(cfg.vmaf_model, "vmaf_v0.6.1");
    }
}
