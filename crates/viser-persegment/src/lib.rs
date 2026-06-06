//! Segment-level CRF adaptation for the `viser` video-encoding-optimizer workspace.
//!
//! Analyzes spatial/temporal complexity across the source, then for each short (typically
//! 1-second) segment runs a closed-loop binary search over CRF, encoding and measuring VMAF
//! until the target quality is met within tolerance. The entry point `adapt` returns
//! per-segment CRF choices along with overall bitrate/VMAF averages and the complexity profile.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use viser_complexity::{self, AnalyzeOpts, Profile};
use viser_ffmpeg::{self, Codec, EncodeJob, Resolution};
use viser_quality::{self, MeasureOpts, Metric};

/// Config for segment-level CRF adaptation.
#[derive(Debug, Clone)]
pub struct Config {
    /// Target VMAF score each segment's CRF search aims to achieve.
    pub target_vmaf: f64,
    /// Acceptable absolute VMAF deviation from the target before stopping the search.
    pub tolerance: f64,
    /// Lowest (highest-quality) CRF the search will consider.
    pub min_crf: i32,
    /// Highest (lowest-quality) CRF the search will consider.
    pub max_crf: i32,
    /// Codec used for segment encodes.
    pub codec: Codec,
    /// Output resolution; `None` keeps the source resolution.
    pub resolution: Option<Resolution>,
    /// Encoder preset (e.g. "medium").
    pub preset: String,
    /// Length of each adapted segment.
    pub segment_duration: Duration,
    /// Maximum binary-search iterations per segment.
    pub max_iterations: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_vmaf: 93.0,
            tolerance: 2.0,
            min_crf: 15,
            max_crf: 45,
            codec: Codec::X264,
            resolution: None,
            preset: "medium".into(),
            segment_duration: Duration::from_secs(1),
            max_iterations: 5,
        }
    }
}

/// Adaptation result for a single segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentResult {
    /// Start timestamp of the segment within the source.
    pub start: Duration,
    /// End timestamp of the segment within the source.
    pub end: Duration,
    /// Final CRF selected for the segment.
    pub crf: i32,
    /// Measured bitrate at the selected CRF.
    pub bitrate: f64,
    /// Measured VMAF at the selected CRF.
    pub vmaf: f64,
    /// Complexity score of the segment from the complexity analysis.
    pub complexity: f64,
    /// Number of search iterations performed for the segment.
    pub iterations: i32,
}

/// Complete output of a segment-level CRF adaptation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    /// Path to the analyzed source video.
    pub source: String,
    /// Per-segment adaptation results in time order.
    pub segments: Vec<SegmentResult>,
    /// Duration-weighted average bitrate across all segments.
    pub avg_bitrate: f64,
    /// Duration-weighted average VMAF across all segments.
    pub avg_vmaf: f64,
    /// The target VMAF the adaptation aimed for.
    pub target_vmaf: f64,
    /// Wall-clock duration of the adaptation.
    pub duration: Duration,
    /// Complexity profile computed for the source.
    pub complexity_profile: Profile,
}

/// Runs segment-level adaptation.
pub async fn adapt(source: &str, cfg: Config) -> anyhow::Result<Result> {
    let start = Instant::now();

    // Step 1: Analyze complexity
    let profile = viser_complexity::analyze(
        source,
        AnalyzeOpts { segment_duration: cfg.segment_duration, subsample: 2 },
    )
    .await?;

    // Step 2: Map complexity to initial CRF
    let segments: Vec<SegmentResult> = profile
        .segments
        .iter()
        .map(|seg| SegmentResult {
            start: seg.start,
            end: seg.end,
            crf: complexity_to_crf(seg.score, cfg.min_crf, cfg.max_crf),
            bitrate: 0.0,
            vmaf: 0.0,
            complexity: seg.score,
            iterations: 0,
        })
        .collect();

    // Step 3: Temp directory
    let tmp_dir = tempfile::Builder::new().prefix("viser-persegment-").tempdir()?;

    // Step 4: Encode and verify each segment in parallel
    let parallel = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallel));
    let source = Arc::new(source.to_string());

    let mut handles = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let sem = semaphore.clone();
        let source = source.clone();
        let seg = seg.clone();
        let cfg_shared = Config {
            min_crf: cfg.min_crf,
            max_crf: cfg.max_crf,
            codec: cfg.codec,
            preset: cfg.preset.clone(),
            resolution: cfg.resolution,
            target_vmaf: cfg.target_vmaf,
            tolerance: cfg.tolerance,
            max_iterations: cfg.max_iterations,
            segment_duration: Duration::from_secs(0),
        };
        let tmp_dir_path = tmp_dir.path().to_path_buf();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            let mut crf_low = cfg_shared.min_crf;
            let mut crf_high = cfg_shared.max_crf;
            let mut seg = seg;

            for iter in 0..cfg_shared.max_iterations {
                seg.iterations = iter + 1;

                let seg_source = tmp_dir_path.join(format!("seg_{i:03}_src.mkv"));
                let seg_encoded = tmp_dir_path.join(format!("seg_{i:03}_crf{}.mp4", seg.crf));

                let dur_secs = (seg.end - seg.start).as_secs_f64();
                viser_ffmpeg::extract(
                    &source,
                    &seg_source.to_string_lossy(),
                    seg.start.as_secs_f64(),
                    dur_secs,
                )
                .await?;

                let job = EncodeJob {
                    input: seg_source.to_string_lossy().to_string(),
                    output: seg_encoded.to_string_lossy().to_string(),
                    codec: cfg_shared.codec,
                    crf: seg.crf,
                    preset: cfg_shared.preset.clone(),
                    resolution: cfg_shared.resolution,
                    rate_control: viser_ffmpeg::RateControlMode::Crf,
                    target_bitrate: 0.0,
                    max_bitrate: 0.0,
                    bufsize: 0.0,
                    extra_args: vec![],
                };

                let enc_result = viser_ffmpeg::encode(job, None).await?;
                seg.bitrate = enc_result.bitrate;

                let q_result = viser_quality::measure(
                    &seg_source.to_string_lossy(),
                    &seg_encoded.to_string_lossy(),
                    MeasureOpts { metrics: vec![Metric::Vmaf], subsample: 5, ..Default::default() },
                )
                .await?;
                seg.vmaf = q_result.vmaf;

                let _ = std::fs::remove_file(&seg_encoded);
                let _ = std::fs::remove_file(&seg_source);

                if (seg.vmaf - cfg_shared.target_vmaf).abs() <= cfg_shared.tolerance {
                    break;
                }

                if seg.vmaf > cfg_shared.target_vmaf + cfg_shared.tolerance {
                    crf_low = seg.crf;
                } else {
                    crf_high = seg.crf;
                }
                seg.crf = (crf_low + crf_high) / 2;

                if crf_high - crf_low <= 1 {
                    break;
                }
            }

            Ok::<_, anyhow::Error>((i, seg))
        }));
    }

    let mut seg_results: Vec<Option<SegmentResult>> = vec![None; segments.len()];
    for h in handles {
        let (i, seg) = h.await??;
        seg_results[i] = Some(seg);
    }
    let segments: Vec<SegmentResult> = seg_results.into_iter().flatten().collect();

    // Compute averages
    let mut total_bitrate = 0.0;
    let mut total_vmaf = 0.0;
    let mut total_dur = 0.0;
    for seg in &segments {
        let dur = (seg.end - seg.start).as_secs_f64();
        total_bitrate += seg.bitrate * dur;
        total_vmaf += seg.vmaf * dur;
        total_dur += dur;
    }

    Ok(Result {
        source: source.to_string(),
        segments,
        avg_bitrate: total_bitrate / total_dur,
        avg_vmaf: total_vmaf / total_dur,
        target_vmaf: cfg.target_vmaf,
        duration: start.elapsed(),
        complexity_profile: profile,
    })
}

fn complexity_to_crf(score: f64, min_crf: i32, max_crf: i32) -> i32 {
    let crf = max_crf as f64 - (score / 100.0) * (max_crf - min_crf) as f64;
    crf.round().clamp(min_crf as f64, max_crf as f64) as i32
}
