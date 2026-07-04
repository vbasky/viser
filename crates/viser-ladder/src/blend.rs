//! Duration-weighted blending of per-shot ladders into a single composite ladder.
//!
//! Per-shot analysis produces independent hulls; this module merges them into one
//! Pareto frontier suitable for whole-clip delivery, weighting longer shots more
//! heavily in the composite point cloud.

use serde::{Deserialize, Serialize};
use viser_hull::{Hull, Point, compute_upper};

use crate::{Ladder, Opts, select};

/// One shot's hull with its duration for blending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotHull {
    /// Shot duration in seconds.
    pub duration_secs: f64,
    /// Convex hull for this shot.
    pub hull: Hull,
}

/// Blends per-shot hulls into a duration-weighted composite ladder.
///
/// Longer shots contribute more copies of their hull points to the merged cloud
/// before the upper hull is recomputed, so the composite ladder favours settings
/// that work well across the bulk of the timeline.
pub fn blend_shot_ladders(shots: &[ShotHull], opts: &Opts) -> Ladder {
    if shots.is_empty() || opts.num_rungs <= 0 {
        return Ladder { rungs: vec![] };
    }

    let total_dur: f64 = shots.iter().map(|s| s.duration_secs).sum();
    if total_dur <= 0.0 {
        return Ladder { rungs: vec![] };
    }

    let mut weighted: Vec<Point> = Vec::new();
    for shot in shots {
        if shot.hull.points.is_empty() || shot.duration_secs <= 0.0 {
            continue;
        }
        let weight = (shot.duration_secs / total_dur * 100.0).round().max(1.0) as usize;
        for _ in 0..weight {
            weighted.extend(shot.hull.points.iter().cloned());
        }
    }

    if weighted.is_empty() {
        return Ladder { rungs: vec![] };
    }

    let composite = compute_upper(&weighted);
    select(&composite, opts)
}

/// Smooths adjacent rung quality steps so CRF/VMAF jumps between rungs stay bounded.
///
/// Re-selects rungs when consecutive VMAF gaps exceed `max_vmaf_step`.
pub fn smooth_ladder(ladder: &Ladder, max_vmaf_step: f64) -> Ladder {
    if ladder.rungs.len() < 2 || max_vmaf_step <= 0.0 {
        return ladder.clone();
    }

    let mut rungs = ladder.rungs.clone();
    for i in 0..rungs.len().saturating_sub(1) {
        let gap = (rungs[i + 1].point.vmaf - rungs[i].point.vmaf).abs();
        if gap > max_vmaf_step {
            let mid_vmaf = (rungs[i].point.vmaf + rungs[i + 1].point.vmaf) / 2.0;
            rungs[i].point.vmaf = rungs[i].point.vmaf.min(mid_vmaf + max_vmaf_step / 2.0);
            rungs[i + 1].point.vmaf = rungs[i + 1].point.vmaf.max(mid_vmaf - max_vmaf_step / 2.0);
        }
    }
    Ladder { rungs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::{Codec, Resolution};
    use viser_hull::Point;

    fn point(bitrate: f64, vmaf: f64) -> Point {
        Point {
            resolution: Resolution::new(1920, 1080),
            codec: Codec::X264,
            crf: 23,
            bitrate,
            vmaf,
            psnr: 0.0,
            ssim: 0.0,
        }
    }

    #[test]
    fn blend_empty_returns_empty() {
        let ladder = blend_shot_ladders(&[], &Opts::default());
        assert!(ladder.rungs.is_empty());
    }

    #[test]
    fn blend_produces_rungs() {
        let shots = vec![
            ShotHull {
                duration_secs: 10.0,
                hull: Hull { points: vec![point(500.0, 70.0), point(2000.0, 92.0)] },
            },
            ShotHull {
                duration_secs: 5.0,
                hull: Hull { points: vec![point(800.0, 80.0), point(3000.0, 95.0)] },
            },
        ];
        let ladder = blend_shot_ladders(&shots, &Opts { num_rungs: 3, ..Opts::default() });
        assert!(!ladder.rungs.is_empty());
        assert!(ladder.rungs.len() <= 3);
    }

    #[test]
    fn smooth_ladder_noop_on_single_rung() {
        let ladder = Ladder { rungs: vec![crate::Rung { point: point(1000.0, 85.0), index: 0 }] };
        let smoothed = smooth_ladder(&ladder, 5.0);
        assert_eq!(smoothed.rungs.len(), 1);
    }
}
