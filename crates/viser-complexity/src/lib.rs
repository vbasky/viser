use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;
use viser_ffmpeg::{ffmpeg_path, probe};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameComplexity {
    pub pts: Duration,
    pub spatial: f64,    // normalized entropy (0-1)
    pub temporal: f64,   // inter-frame luma difference (0-255)
    pub dct_energy: f64, // average DCT coefficient energy
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentComplexity {
    pub start: Duration,
    pub end: Duration,
    pub duration: Duration,
    pub avg_spatial: f64,
    pub avg_temporal: f64,
    pub max_spatial: f64,
    pub max_temporal: f64,
    pub score: f64, // combined 0-100 complexity score
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub frames: Vec<FrameComplexity>,
    pub segments: Vec<SegmentComplexity>,
    pub avg_spatial: f64,
    pub avg_temporal: f64,
    pub overall_score: f64,
}

#[derive(Debug, Clone)]
pub struct AnalyzeOpts {
    pub segment_duration: Duration,
    pub subsample: i32,
}

impl Default for AnalyzeOpts {
    fn default() -> Self {
        Self { segment_duration: Duration::from_secs(2), subsample: 1 }
    }
}

/// Extracts per-frame complexity metrics and aggregates them into segments.
pub async fn analyze(path: &str, opts: AnalyzeOpts) -> anyhow::Result<Profile> {
    let seg_dur = if opts.segment_duration.is_zero() {
        Duration::from_secs(2)
    } else {
        opts.segment_duration
    };
    let subsample = if opts.subsample <= 0 { 1 } else { opts.subsample };

    let probe_result = probe(path).await?;
    let total_duration = Duration::from_secs_f64(probe_result.format.duration);

    let select_filter =
        if subsample > 1 { format!("select='not(mod(n\\,{subsample}))',") } else { String::new() };

    let filter = format!("{select_filter}entropy,signalstats,metadata=mode=print:file=-");
    let args = ["-i", path, "-vf", &filter, "-f", "null", "-"];

    let output = Command::new(ffmpeg_path())
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("complexity analysis failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let frames = parse_complexity_output(&stdout);

    if frames.is_empty() {
        anyhow::bail!("no frames analyzed");
    }

    let segments = aggregate_segments(&frames, total_duration, seg_dur);

    let n = frames.len() as f64;
    let avg_spatial: f64 = frames.iter().map(|f| f.spatial).sum::<f64>() / n;
    let avg_temporal: f64 = frames.iter().map(|f| f.temporal).sum::<f64>() / n;
    let overall_score = compute_score(avg_spatial, avg_temporal);

    Ok(Profile { frames, segments, avg_spatial, avg_temporal, overall_score })
}

fn parse_complexity_output(output: &str) -> Vec<FrameComplexity> {
    let mut frames = Vec::new();
    let mut current =
        FrameComplexity { pts: Duration::ZERO, spatial: 0.0, temporal: 0.0, dct_energy: 0.0 };
    let mut has_pts = false;

    for line in output.lines() {
        if line.starts_with("frame:") {
            if has_pts {
                frames.push(current.clone());
            }
            current = FrameComplexity {
                pts: Duration::ZERO,
                spatial: 0.0,
                temporal: 0.0,
                dct_energy: 0.0,
            };
            has_pts = false;

            if let Some(pts_time) = extract_field(line, "pts_time:") {
                if let Ok(seconds) = pts_time.parse::<f64>() {
                    current.pts = Duration::from_secs_f64(seconds);
                    has_pts = true;
                }
            }
            continue;
        }

        if let Some(val) = line.strip_prefix("lavfi.entropy.normalized_entropy.normal.Y=") {
            current.spatial = val.parse().unwrap_or(0.0);
        }
        if let Some(val) = line.strip_prefix("lavfi.signalstats.YDIF=") {
            current.temporal = val.parse().unwrap_or(0.0);
        }
        if let Some(val) = line.strip_prefix("lavfi.signalstats.YHIGH=") {
            current.dct_energy = val.parse().unwrap_or(0.0);
        }
        if let Some(val) = line.strip_prefix("lavfi.signalstats.YLOW=") {
            let y_low: f64 = val.parse().unwrap_or(0.0);
            current.dct_energy -= y_low;
            if current.dct_energy < 0.0 {
                current.dct_energy = 0.0;
            }
        }
    }

    if has_pts {
        frames.push(current);
    }

    frames
}

fn extract_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let rest = rest.trim_start();
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn aggregate_segments(
    frames: &[FrameComplexity],
    total_duration: Duration,
    seg_duration: Duration,
) -> Vec<SegmentComplexity> {
    let mut segments = Vec::new();
    let mut seg_start = Duration::ZERO;

    while seg_start < total_duration {
        let seg_end = (seg_start + seg_duration).min(total_duration);

        let seg_frames: Vec<&FrameComplexity> =
            frames.iter().filter(|f| f.pts >= seg_start && f.pts < seg_end).collect();

        if !seg_frames.is_empty() {
            let spatial: Vec<f64> = seg_frames.iter().map(|f| f.spatial).collect();
            let temporal: Vec<f64> = seg_frames.iter().map(|f| f.temporal).collect();
            let dct: Vec<f64> = seg_frames.iter().map(|f| f.dct_energy).collect();

            let avg_dct = mean(&dct);
            let avg_s = mean(&spatial);
            let avg_t = mean(&temporal);

            segments.push(SegmentComplexity {
                start: seg_start,
                end: seg_end,
                duration: seg_end - seg_start,
                avg_spatial: avg_s,
                avg_temporal: avg_t,
                max_spatial: max_val(&spatial),
                max_temporal: max_val(&temporal),
                score: compute_score_with_dct(avg_s, avg_t, avg_dct),
            });
        }

        seg_start = seg_end;
    }

    segments
}

fn compute_score(spatial: f64, temporal: f64) -> f64 {
    let spatial_norm = ((spatial - 0.5) * 200.0).clamp(0.0, 100.0);
    let temporal_norm = (temporal * 3.33).min(100.0);
    spatial_norm * 0.6 + temporal_norm * 0.4
}

fn compute_score_with_dct(spatial: f64, temporal: f64, dct_energy: f64) -> f64 {
    let spatial_norm = ((spatial - 0.5) * 200.0).clamp(0.0, 100.0);
    let temporal_norm = (temporal * 3.33).min(100.0);
    let dct_norm = (dct_energy * 0.5).min(100.0);
    spatial_norm * 0.4 + dct_norm * 0.3 + temporal_norm * 0.3
}

fn mean(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.iter().sum::<f64>() / vals.len() as f64
}

fn max_val(vals: &[f64]) -> f64 {
    vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}
