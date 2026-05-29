use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use viser_complexity::{self, AnalyzeOpts, Profile};
use viser_ffmpeg::{self, Codec, EncodeJob, Resolution};
use viser_quality::{self, MeasureOpts, Metric};

/// Config for segment-level CRF adaptation.
#[derive(Debug, Clone)]
pub struct Config {
    pub target_vmaf: f64,
    pub tolerance: f64,
    pub min_crf: i32,
    pub max_crf: i32,
    pub codec: Codec,
    pub resolution: Option<Resolution>,
    pub preset: String,
    pub segment_duration: Duration,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentResult {
    pub start: Duration,
    pub end: Duration,
    pub crf: i32,
    pub bitrate: f64,
    pub vmaf: f64,
    pub complexity: f64,
    pub iterations: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    pub source: String,
    pub segments: Vec<SegmentResult>,
    pub avg_bitrate: f64,
    pub avg_vmaf: f64,
    pub target_vmaf: f64,
    pub duration: Duration,
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
    let mut segments: Vec<SegmentResult> = profile
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

    // Step 4: Encode and verify each segment (closed loop with binary search)
    for (i, seg) in segments.iter_mut().enumerate() {
        let mut crf_low = cfg.min_crf;
        let mut crf_high = cfg.max_crf;

        for iter in 0..cfg.max_iterations {
            seg.iterations = iter + 1;

            let seg_source = tmp_dir.path().join(format!("seg_{i:03}_src.mkv"));
            let seg_encoded = tmp_dir.path().join(format!("seg_{i:03}_crf{}.mp4", seg.crf));

            let dur_secs = (seg.end - seg.start).as_secs_f64();
            viser_ffmpeg::extract(
                source,
                &seg_source.to_string_lossy(),
                seg.start.as_secs_f64(),
                dur_secs,
            )
            .await?;

            let job = EncodeJob {
                input: seg_source.to_string_lossy().to_string(),
                output: seg_encoded.to_string_lossy().to_string(),
                codec: cfg.codec,
                crf: seg.crf,
                preset: cfg.preset.clone(),
                resolution: cfg.resolution,
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

            if (seg.vmaf - cfg.target_vmaf).abs() <= cfg.tolerance {
                break;
            }

            if seg.vmaf > cfg.target_vmaf + cfg.tolerance {
                crf_low = seg.crf;
            } else {
                crf_high = seg.crf;
            }
            seg.crf = (crf_low + crf_high) / 2;

            if crf_high - crf_low <= 1 {
                break;
            }
        }
    }

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
