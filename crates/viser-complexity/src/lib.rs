use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use tokio::process::Command;
use viser_ffmpeg::{ffmpeg_path, probe};

/// Classified scene type based on spatial/temporal complexity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneClass {
    /// Black / fade / freeze frame (near-zero motion and detail)
    Black,
    /// Static / talking-heads (low spatial, low temporal)
    Static,
    /// Detailed / landscape (high spatial, low temporal)
    Detailed,
    /// Motion / action (low spatial, high temporal)
    Motion,
    /// Complex / crowd / nature (high spatial, high temporal)
    Complex,
}

impl fmt::Display for SceneClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneClass::Black => write!(f, "black"),
            SceneClass::Static => write!(f, "static"),
            SceneClass::Detailed => write!(f, "detailed"),
            SceneClass::Motion => write!(f, "motion"),
            SceneClass::Complex => write!(f, "complex"),
        }
    }
}

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
    pub scene_class: SceneClass,
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

            let scene_class = classify_scene(avg_s, avg_t, avg_dct);

            segments.push(SegmentComplexity {
                start: seg_start,
                end: seg_end,
                duration: seg_end - seg_start,
                avg_spatial: avg_s,
                avg_temporal: avg_t,
                max_spatial: max_val(&spatial),
                max_temporal: max_val(&temporal),
                score: compute_score_with_dct(avg_s, avg_t, avg_dct),
                scene_class,
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

/// Classify a segment by its spatial/temporal complexity profile.
pub fn classify_scene(spatial: f64, temporal: f64, dct_energy: f64) -> SceneClass {
    let s = spatial;
    let t = temporal;
    let d = dct_energy;

    // Black / fade: near-zero motion, entropy, and detail
    if t < 1.0 && s <= 0.3 && d < 5.0 {
        return SceneClass::Black;
    }

    // Motion / action: high inter-frame difference, moderate detail
    if t > 8.0 && s < 0.65 {
        return SceneClass::Motion;
    }

    // Complex / crowd / nature: high texture + high motion
    if s > 0.7 && t > 5.0 {
        return SceneClass::Complex;
    }

    // Detailed / landscape: high entropy, low motion
    if s >= 0.65 && t <= 5.0 {
        return SceneClass::Detailed;
    }

    // Static / talking-heads: low motion, moderate entropy
    SceneClass::Static
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

/// Content type classification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentType {
    /// Natural video content (film, sports, etc.)
    Natural,
    /// Screen content (slides, code, UI, screencasts)
    Screen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenContentDetection {
    pub content_type: ContentType,
    pub confidence: f64, // 0-100
    pub reason: String,
}

/// Detects whether a video is screen content based on complexity profile heuristics.
///
/// Screen content signatures:
/// - Very high spatial complexity (sharp edges, text)
/// - Very low temporal complexity (static or near-static)
/// - High fraction of frames with minimal temporal change
pub fn detect_screen_content(profile: &Profile) -> ScreenContentDetection {
    if profile.frames.is_empty() {
        return ScreenContentDetection {
            content_type: ContentType::Natural,
            confidence: 0.0,
            reason: "no frames analyzed".into(),
        };
    }

    // Screen content heuristics
    let static_fraction = profile.frames.iter().filter(|f| f.temporal < 1.5).count() as f64
        / profile.frames.len() as f64;

    let has_sharp_edges = profile.avg_spatial > 0.75;
    let is_mostly_static = static_fraction > 0.8;
    let high_dct_low_temporal = profile.avg_temporal < 2.0
        && (profile.segments.iter().any(|s| s.avg_spatial > 0.7) || profile.avg_spatial > 0.7);

    let score = if has_sharp_edges && is_mostly_static {
        // Classic slide/UI: sharp edges, barely moves
        90.0
    } else if high_dct_low_temporal {
        // Code/screenshot: high DCT energy but low motion
        70.0
    } else if is_mostly_static && profile.avg_spatial > 0.6 {
        // Mostly static with moderate edges
        50.0
    } else if static_fraction > 0.6 && profile.avg_temporal < 3.0 {
        // Leaning static
        30.0
    } else {
        0.0
    };

    let content_type = if score >= 50.0 { ContentType::Screen } else { ContentType::Natural };
    let reason = if score >= 90.0 {
        format!(
            "sharp edges (spatial={:.2}) + mostly static ({:.0}% frames) — classic screen content",
            profile.avg_spatial,
            static_fraction * 100.0
        )
    } else if score >= 70.0 {
        format!(
            "high spatial/DCT energy (spatial={:.2}) with low temporal ({:.1}) — likely screen content",
            profile.avg_spatial, profile.avg_temporal
        )
    } else if score >= 50.0 {
        format!(
            "mostly static ({:.0}% frames) with moderate spatial ({:.2}) — possible screen content",
            static_fraction * 100.0,
            profile.avg_spatial
        )
    } else if score >= 30.0 {
        format!("leaning static ({:.0}% frames)", static_fraction * 100.0)
    } else {
        "natural video content detected".into()
    };

    ScreenContentDetection { content_type, confidence: score, reason }
}

#[cfg(test)]
mod screen_tests {
    use super::*;

    fn mk_frame(pts_secs: f64, spatial: f64, temporal: f64, dct: f64) -> FrameComplexity {
        FrameComplexity {
            pts: Duration::from_secs_f64(pts_secs),
            spatial,
            temporal,
            dct_energy: dct,
        }
    }

    #[test]
    fn test_detect_screen_content_slides() {
        // All frames have sharp edges, no motion
        let frames: Vec<_> =
            (0..100).map(|i| mk_frame(i as f64 * 0.04, 0.85, 0.2, 100.0)).collect();
        let profile = Profile {
            frames: frames.clone(),
            segments: vec![],
            avg_spatial: 0.85,
            avg_temporal: 0.2,
            overall_score: 0.0,
        };
        let detection = detect_screen_content(&profile);
        assert_eq!(detection.content_type, ContentType::Screen);
        assert!(detection.confidence >= 90.0);
    }

    #[test]
    fn test_detect_screen_content_natural_video() {
        let frames: Vec<_> = (0..100).map(|i| mk_frame(i as f64 * 0.04, 0.5, 10.0, 50.0)).collect();
        let profile = Profile {
            frames,
            segments: vec![],
            avg_spatial: 0.5,
            avg_temporal: 10.0,
            overall_score: 0.0,
        };
        let detection = detect_screen_content(&profile);
        assert_eq!(detection.content_type, ContentType::Natural);
        assert_eq!(detection.confidence, 0.0);
    }

    #[test]
    fn test_detect_screen_content_empty() {
        let profile = Profile {
            frames: vec![],
            segments: vec![],
            avg_spatial: 0.0,
            avg_temporal: 0.0,
            overall_score: 0.0,
        };
        let detection = detect_screen_content(&profile);
        assert_eq!(detection.content_type, ContentType::Natural);
        assert_eq!(detection.confidence, 0.0);
    }

    #[test]
    fn test_detect_screen_content_code_capture() {
        // High spatial, low temporal, moderate DCT
        let frames: Vec<_> = (0..100).map(|i| mk_frame(i as f64 * 0.04, 0.78, 1.5, 80.0)).collect();
        let profile = Profile {
            frames: frames.clone(),
            segments: vec![],
            avg_spatial: 0.78,
            avg_temporal: 1.5,
            overall_score: 0.0,
        };
        let detection = detect_screen_content(&profile);
        assert_eq!(detection.content_type, ContentType::Screen);
        assert!(detection.confidence >= 70.0, "expected >= 70, got {}", detection.confidence);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pts_secs: f64, spatial: f64, temporal: f64, dct: f64) -> FrameComplexity {
        FrameComplexity {
            pts: Duration::from_secs_f64(pts_secs),
            spatial,
            temporal,
            dct_energy: dct,
        }
    }

    #[test]
    fn test_mean_empty() {
        assert!((mean(&[]) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_mean_single() {
        assert!((mean(&[42.0]) - 42.0).abs() < 1e-9);
    }

    #[test]
    fn test_mean_multiple() {
        assert!((mean(&[1.0, 2.0, 3.0]) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_max_val_empty_negative_inf() {
        assert!(max_val(&[]).is_infinite() && max_val(&[]).is_sign_negative());
    }

    #[test]
    fn test_max_val() {
        assert!((max_val(&[1.0, 5.0, 3.0]) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_compute_score_bounds() {
        let s = compute_score(0.5, 0.0);
        assert!(s >= 0.0 && s <= 100.0);
    }

    #[test]
    fn test_compute_score_zero_input() {
        let s = compute_score(0.0, 0.0);
        assert!(s >= 0.0);
    }

    #[test]
    fn test_compute_score_high_input() {
        let s = compute_score(1.0, 30.0); // temporal 30*3.33=99.9, spatial (1-0.5)*200=100
        assert!(s <= 100.0);
        assert!(s > 50.0);
    }

    #[test]
    fn test_compute_score_with_dct() {
        let s = compute_score_with_dct(0.5, 0.0, 0.0);
        assert!(s >= 0.0);
    }

    #[test]
    fn test_parse_complexity_output_empty() {
        let frames = parse_complexity_output("");
        assert!(frames.is_empty());
    }

    #[test]
    fn test_parse_complexity_output_basic() {
        let output = "\
frame: 1 pts_time:0.000
lavfi.entropy.normalized_entropy.normal.Y=0.6
lavfi.signalstats.YDIF=2.5
lavfi.signalstats.YHIGH=100.0
lavfi.signalstats.YLOW=30.0
frame: 2 pts_time:1.000
lavfi.entropy.normalized_entropy.normal.Y=0.7
lavfi.signalstats.YDIF=3.0
lavfi.signalstats.YHIGH=120.0
lavfi.signalstats.YLOW=40.0
";
        let frames = parse_complexity_output(output);
        assert_eq!(frames.len(), 2);

        assert!((frames[0].spatial - 0.6).abs() < 1e-9);
        assert!((frames[0].temporal - 2.5).abs() < 1e-9);
        assert!((frames[0].dct_energy - 70.0).abs() < 1e-9); // 100 - 30

        assert!((frames[1].spatial - 0.7).abs() < 1e-9);
        assert!((frames[1].temporal - 3.0).abs() < 1e-9);
        assert!((frames[1].dct_energy - 80.0).abs() < 1e-9); // 120 - 40
    }

    #[test]
    fn test_parse_complexity_output_handles_partial_data() {
        let output = "\
frame: 1 pts_time:0.000
lavfi.entropy.normalized_entropy.normal.Y=0.5
frame: 2 pts_time:1.000
lavfi.signalstats.YDIF=1.0
";
        let frames = parse_complexity_output(output);
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn test_parse_complexity_output_negative_dct() {
        // If YLOW > YHIGH, dct_energy should clamp to 0
        let output = "\
frame: 1 pts_time:0.000
lavfi.signalstats.YHIGH=30.0
lavfi.signalstats.YLOW=50.0
";
        let frames = parse_complexity_output(output);
        assert!((frames[0].dct_energy - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_aggregate_segments_single_segment() {
        let frames = vec![
            frame(0.0, 0.5, 1.0, 10.0),
            frame(0.5, 0.6, 2.0, 20.0),
            frame(1.0, 0.7, 3.0, 30.0),
        ];
        let segs = aggregate_segments(&frames, Duration::from_secs(2), Duration::from_secs(2));
        assert_eq!(segs.len(), 1);
        assert!((segs[0].avg_spatial - 0.6).abs() < 0.01);
        assert!((segs[0].avg_temporal - 2.0).abs() < 0.01);
        assert!((segs[0].max_spatial - 0.7).abs() < 1e-9);
        assert_eq!(segs[0].start, Duration::ZERO);
        assert_eq!(segs[0].end, Duration::from_secs(2));
    }

    #[test]
    fn test_aggregate_segments_multiple() {
        let frames = vec![
            frame(0.0, 0.4, 1.0, 5.0),
            frame(0.5, 0.5, 1.5, 6.0),
            frame(1.0, 0.6, 2.0, 7.0),
            frame(1.5, 0.7, 2.5, 8.0),
            frame(2.0, 0.8, 3.0, 9.0),
            frame(2.5, 0.9, 3.5, 10.0),
        ];
        let segs = aggregate_segments(&frames, Duration::from_secs(3), Duration::from_secs(1));
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].start, Duration::from_secs(0));
        assert_eq!(segs[1].start, Duration::from_secs(1));
        assert_eq!(segs[2].start, Duration::from_secs(2));
    }

    #[test]
    fn test_aggregate_segments_empty_bucket() {
        // Evenly spaced frames with a gap
        let frames = vec![frame(0.0, 0.5, 1.0, 5.0), frame(3.0, 0.8, 3.0, 10.0)];
        let segs = aggregate_segments(&frames, Duration::from_secs(4), Duration::from_secs(2));
        assert_eq!(segs.len(), 2); // seg 0 has frame[0], seg 1 has frame[1]
    }

    #[test]
    fn test_classify_black() {
        assert_eq!(classify_scene(0.2, 0.5, 2.0), SceneClass::Black);
    }

    #[test]
    fn test_classify_static() {
        assert_eq!(classify_scene(0.4, 1.5, 10.0), SceneClass::Static);
    }

    #[test]
    fn test_classify_detailed() {
        assert_eq!(classify_scene(0.7, 3.0, 50.0), SceneClass::Detailed);
    }

    #[test]
    fn test_classify_motion() {
        assert_eq!(classify_scene(0.4, 10.0, 30.0), SceneClass::Motion);
    }

    #[test]
    fn test_classify_complex() {
        assert_eq!(classify_scene(0.8, 6.0, 60.0), SceneClass::Complex);
    }

    #[test]
    fn test_classify_edges() {
        // Boundary conditions
        assert_eq!(classify_scene(0.65, 5.0, 30.0), SceneClass::Detailed);
        assert_eq!(classify_scene(0.6, 9.0, 20.0), SceneClass::Motion);
    }

    #[test]
    fn test_scene_class_display() {
        assert_eq!(SceneClass::Black.to_string(), "black");
        assert_eq!(SceneClass::Static.to_string(), "static");
        assert_eq!(SceneClass::Detailed.to_string(), "detailed");
        assert_eq!(SceneClass::Motion.to_string(), "motion");
        assert_eq!(SceneClass::Complex.to_string(), "complex");
    }

    #[test]
    fn test_segment_has_scene_class() {
        let frames = vec![FrameComplexity {
            pts: Duration::from_secs_f64(0.0),
            spatial: 0.2,
            temporal: 0.5,
            dct_energy: 2.0,
        }];
        let segs = aggregate_segments(&frames, Duration::from_secs(2), Duration::from_secs(2));
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].scene_class, SceneClass::Black);
    }
    fn test_analyze_opts_default() {
        let opts = AnalyzeOpts::default();
        assert_eq!(opts.segment_duration, Duration::from_secs(2));
        assert_eq!(opts.subsample, 1);
    }
}
