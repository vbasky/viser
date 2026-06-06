//! Shot/scene boundary detection for the `viser` video-encoding-optimizer workspace.
//!
//! Wraps FFmpeg's `scdet` filter to find scene changes in a video, merges shots
//! shorter than a configurable minimum, and returns each shot's start/end
//! timestamps and change score. See `detect` for the entry point.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;
use viser_ffmpeg::{ffmpeg_path, probe};

/// A detected shot with start and end timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    /// Zero-based shot index within the video.
    pub index: i32,
    /// Start timestamp of the shot.
    pub start: Duration,
    /// End timestamp of the shot.
    pub end: Duration,
    /// Length of the shot (`end - start`).
    pub duration: Duration,
    /// Scene-change score at this shot's starting boundary (0-100); `0` for the first shot.
    pub score: f64, // scene change score at boundary (0-100)
}

/// Parameters controlling `detect`.
#[derive(Debug, Clone)]
pub struct DetectOpts {
    /// Threshold for scene change detection (0-100). Lower = more sensitive.
    pub threshold: f64,
    /// Minimum shot duration. Shots shorter than this are merged.
    pub min_duration: Duration,
}

impl Default for DetectOpts {
    fn default() -> Self {
        Self { threshold: 10.0, min_duration: Duration::from_millis(500) }
    }
}

/// Finds shot boundaries using FFmpeg's scdet filter.
pub async fn detect(path: &str, opts: DetectOpts) -> anyhow::Result<Vec<Shot>> {
    let threshold = if opts.threshold <= 0.0 { 10.0 } else { opts.threshold };
    let min_duration =
        if opts.min_duration.is_zero() { Duration::from_millis(500) } else { opts.min_duration };

    let probe_result = probe(path).await?;
    let total_duration = Duration::from_secs_f64(probe_result.format.duration);

    let filter = format!("scdet=t={threshold:.1},metadata=mode=print:file=-");
    let args = ["-i", path, "-vf", &filter, "-f", "null", "-"];

    let output = Command::new(ffmpeg_path())
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let boundaries = parse_scdet_output(&stdout);
    let shots = build_shots(&boundaries, total_duration, min_duration);

    Ok(shots)
}

struct SceneChange {
    pts: Duration,
    score: f64,
}

fn parse_scdet_output(output: &str) -> Vec<SceneChange> {
    let mut changes = Vec::new();
    let mut current_pts = Duration::ZERO;
    let mut has_pts = false;

    for line in output.lines() {
        if line.starts_with("frame:") {
            if let Some(pts_time) = extract_field(line, "pts_time:") {
                if let Ok(seconds) = pts_time.parse::<f64>() {
                    current_pts = Duration::from_secs_f64(seconds);
                    has_pts = true;
                }
            }
            continue;
        }

        if let Some(score_str) = line.strip_prefix("lavfi.scd.score=") {
            if let Ok(score) = score_str.parse::<f64>() {
                if score > 0.0 && has_pts {
                    changes.push(SceneChange { pts: current_pts, score });
                }
            }
        }
    }

    changes.sort_by(|a, b| a.pts.cmp(&b.pts));
    changes
}

fn extract_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let rest = rest.trim_start();
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn build_shots(
    boundaries: &[SceneChange],
    total_duration: Duration,
    min_duration: Duration,
) -> Vec<Shot> {
    if boundaries.is_empty() {
        return vec![Shot {
            index: 0,
            start: Duration::ZERO,
            end: total_duration,
            duration: total_duration,
            score: 0.0,
        }];
    }

    let mut shots = Vec::new();
    let mut prev_end = Duration::ZERO;

    for sc in boundaries {
        if sc.pts <= prev_end {
            continue;
        }

        let s = Shot {
            index: shots.len() as i32,
            start: prev_end,
            end: sc.pts,
            duration: sc.pts.saturating_sub(prev_end),
            score: sc.score,
        };

        if s.duration < min_duration && !shots.is_empty() {
            let last: &mut Shot = shots.last_mut().unwrap();
            last.end = sc.pts;
            last.duration = sc.pts.saturating_sub(last.start);
        } else {
            shots.push(s);
        }

        prev_end = sc.pts;
    }

    // Final shot from last boundary to end
    if prev_end < total_duration {
        let s = Shot {
            index: shots.len() as i32,
            start: prev_end,
            end: total_duration,
            duration: total_duration.saturating_sub(prev_end),
            score: 0.0,
        };
        if s.duration < min_duration && !shots.is_empty() {
            let last: &mut Shot = shots.last_mut().unwrap();
            last.end = total_duration;
            last.duration = total_duration.saturating_sub(last.start);
        } else {
            shots.push(s);
        }
    }

    // Re-index
    for (i, shot) in shots.iter_mut().enumerate() {
        shot.index = i as i32;
    }

    shots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_field_basic() {
        let line = "frame: 123 pts_time: 5.500 foo: bar";
        assert_eq!(extract_field(line, "pts_time:"), Some("5.500"));
    }

    #[test]
    fn test_extract_field_not_found() {
        let line = "frame: 123 foo: bar";
        assert_eq!(extract_field(line, "pts_time:"), None);
    }

    #[test]
    fn test_extract_field_end_of_string() {
        let line = "key: value";
        assert_eq!(extract_field(line, "key:"), Some("value"));
    }

    #[test]
    fn test_parse_scdet_output_empty() {
        let result = parse_scdet_output("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_scdet_output_no_changes() {
        let output = "\
frame: 1 pts_time:1.000
frame: 2 pts_time:2.000
";
        let result = parse_scdet_output(output);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_scdet_output_with_score() {
        let output = "\
frame: 1 pts_time:1.000
frame: 2 pts_time:2.000
lavfi.scd.score=15.5
frame: 3 pts_time:3.000
";
        let result = parse_scdet_output(output);
        assert_eq!(result.len(), 1);
        assert!((result[0].score - 15.5).abs() < 1e-9);
    }

    #[test]
    fn test_parse_scdet_output_multiple() {
        let output = "\
frame: 1 pts_time:1.000
lavfi.scd.score=12.0
frame: 2 pts_time:2.000
frame: 3 pts_time:3.000
lavfi.scd.score=45.5
frame: 4 pts_time:4.000
";
        let result = parse_scdet_output(output);
        assert_eq!(result.len(), 2);
        assert!((result[0].score - 12.0).abs() < 1e-9);
        assert!((result[1].score - 45.5).abs() < 1e-9);
    }

    #[test]
    fn test_parse_scdet_output_ignores_zero_score() {
        let output = "\
frame: 1 pts_time:1.000
lavfi.scd.score=0.0
frame: 2 pts_time:2.000
lavfi.scd.score=25.0
";
        let result = parse_scdet_output(output);
        assert_eq!(result.len(), 1);
        assert!((result[0].score - 25.0).abs() < 1e-9);
    }

    #[test]
    fn test_build_shots_no_boundaries() {
        let shots = build_shots(&[], Duration::from_secs(10), Duration::from_millis(500));
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].start, Duration::ZERO);
        assert_eq!(shots[0].end, Duration::from_secs(10));
        assert_eq!(shots[0].index, 0);
    }

    #[test]
    fn test_build_shots_single_boundary() {
        let boundaries = vec![super::SceneChange { pts: Duration::from_secs(4), score: 20.0 }];
        let shots = build_shots(&boundaries, Duration::from_secs(10), Duration::from_millis(500));
        assert_eq!(shots.len(), 2);
        assert_eq!(shots[0].start, Duration::ZERO);
        assert_eq!(shots[0].end, Duration::from_secs(4));
        assert_eq!(shots[0].score, 20.0);
        assert_eq!(shots[1].start, Duration::from_secs(4));
        assert_eq!(shots[1].end, Duration::from_secs(10));
        assert_eq!(shots[1].score, 0.0);
    }

    #[test]
    fn test_build_shots_merges_short_shots() {
        let boundaries = vec![super::SceneChange { pts: Duration::from_millis(200), score: 15.0 }];
        let shots = build_shots(&boundaries, Duration::from_secs(10), Duration::from_millis(500));
        // First shot can't be merged (nothing to merge into), so we get 2 shots
        assert_eq!(shots.len(), 2);
        assert_eq!(shots[0].start, Duration::ZERO);
        assert_eq!(shots[0].end, Duration::from_millis(200));
    }

    #[test]
    fn test_build_shots_merges_short_middle_shot() {
        // Boundary at 200ms creates a short first shot (unmergeable).
        // Boundary at 300ms would create a 100ms shot between 200ms-300ms,
        // which gets merged into the previous shot.
        let boundaries = vec![
            super::SceneChange { pts: Duration::from_millis(200), score: 15.0 },
            super::SceneChange { pts: Duration::from_millis(300), score: 20.0 },
        ];
        let shots = build_shots(&boundaries, Duration::from_secs(10), Duration::from_millis(500));
        // Shot1 [0, 200ms) = 200ms (first, can't merge)
        // Shot2 [200ms, 300ms) = 100ms (short, merges into shot1) -> shot1 becomes [0, 300ms)
        // Final [300ms, 10s) = 9.7s
        assert_eq!(shots.len(), 2);
        assert_eq!(shots[0].start, Duration::ZERO);
        assert_eq!(shots[0].end, Duration::from_millis(300));
        assert_eq!(shots[1].start, Duration::from_millis(300));
    }

    #[test]
    fn test_build_shots_reindexes() {
        let boundaries = vec![
            super::SceneChange { pts: Duration::from_secs(2), score: 10.0 },
            super::SceneChange { pts: Duration::from_secs(5), score: 20.0 },
        ];
        let shots = build_shots(&boundaries, Duration::from_secs(10), Duration::from_millis(500));
        for (i, s) in shots.iter().enumerate() {
            assert_eq!(s.index as usize, i);
        }
        assert_eq!(shots.len(), 3);
    }

    #[test]
    fn test_shot_serde_roundtrip() {
        let shot = Shot {
            index: 2,
            start: Duration::from_secs(5),
            end: Duration::from_secs(10),
            duration: Duration::from_secs(5),
            score: 42.5,
        };
        let json = serde_json::to_string(&shot).unwrap();
        let back: Shot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.index, 2);
        assert!((back.score - 42.5).abs() < 1e-9);
        assert_eq!(back.start.as_secs(), 5);
    }

    #[test]
    fn test_detect_opts_default() {
        let opts = DetectOpts::default();
        assert!((opts.threshold - 10.0).abs() < 1e-9);
        assert_eq!(opts.min_duration, Duration::from_millis(500));
    }

    #[test]
    fn test_build_shots_skips_same_pts() {
        let boundaries = vec![
            super::SceneChange { pts: Duration::from_secs(5), score: 10.0 },
            super::SceneChange { pts: Duration::from_secs(5), score: 15.0 },
        ];
        let shots = build_shots(&boundaries, Duration::from_secs(10), Duration::from_millis(500));
        assert_eq!(shots.len(), 2); // second boundary at same PTS is skipped
    }
}
