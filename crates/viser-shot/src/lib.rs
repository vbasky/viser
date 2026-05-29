use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;
use viser_ffmpeg::{ffmpeg_path, probe};

/// A detected shot with start and end timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    pub index: i32,
    pub start: Duration,
    pub end: Duration,
    pub duration: Duration,
    pub score: f64, // scene change score at boundary (0-100)
}

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
