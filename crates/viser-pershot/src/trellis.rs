use crate::{ShotResult, TrellisAssignment};

/// Options for Trellis bit allocation across shots.
#[derive(Debug, Clone)]
pub struct TrellisOpts {
    /// Target duration-weighted average bitrate to hit across all shots.
    pub target_bitrate: f64,
    /// Relative tolerance for the achieved bitrate; defaults to 0.05 if <= 0.
    pub tolerance: f64,
}

impl Default for TrellisOpts {
    fn default() -> Self {
        Self { target_bitrate: 0.0, tolerance: 0.05 }
    }
}

/// Finds the optimal encoding parameters for each shot using the
/// constant-slope principle from Lagrangian optimization.
pub fn trellis_optimize(shot_results: &[ShotResult], opts: &TrellisOpts) -> Vec<TrellisAssignment> {
    if shot_results.is_empty() || opts.target_bitrate <= 0.0 {
        return vec![];
    }

    let tolerance = if opts.tolerance <= 0.0 { 0.05 } else { opts.tolerance };

    let total_duration: f64 = shot_results.iter().map(|sr| sr.shot.duration.as_secs_f64()).sum();

    if total_duration <= 0.0 {
        return vec![];
    }

    // Binary search for optimal lambda
    let mut lambda_low = 0.0;
    let mut lambda_high = 1.0;

    // Find initial upper bound
    loop {
        let bitrate = total_bitrate_at_lambda(shot_results, total_duration, lambda_high);
        if bitrate <= opts.target_bitrate {
            break;
        }
        lambda_high *= 2.0;
        if lambda_high > 1e6 {
            break;
        }
    }

    let mut best_assignments = vec![];

    for _ in 0..50 {
        let lambda = (lambda_low + lambda_high) / 2.0;
        let assignments = assign_at_lambda(shot_results, lambda);
        let bitrate = weighted_bitrate(&assignments, shot_results, total_duration);

        if ((bitrate - opts.target_bitrate) / opts.target_bitrate).abs() < tolerance {
            return assignments;
        }

        best_assignments = assignments;

        if bitrate > opts.target_bitrate {
            lambda_low = lambda;
        } else {
            lambda_high = lambda;
        }
    }

    best_assignments
}

fn assign_at_lambda(shot_results: &[ShotResult], lambda: f64) -> Vec<TrellisAssignment> {
    shot_results
        .iter()
        .enumerate()
        .map(|(i, sr)| {
            if sr.hull.points.is_empty() {
                return TrellisAssignment {
                    shot_index: i,
                    resolution: viser_ffmpeg::RES_1080P,
                    codec: viser_ffmpeg::Codec::X264,
                    crf: 23,
                    bitrate: 0.0,
                    vmaf: 0.0,
                };
            }

            let mut best_idx = 0;
            let mut best_value = f64::NEG_INFINITY;

            for (j, p) in sr.hull.points.iter().enumerate() {
                let value = p.vmaf - lambda * p.bitrate;
                if value > best_value {
                    best_value = value;
                    best_idx = j;
                }
            }

            let p = &sr.hull.points[best_idx];
            TrellisAssignment {
                shot_index: i,
                resolution: p.resolution,
                codec: p.codec,
                crf: p.crf,
                bitrate: p.bitrate,
                vmaf: p.vmaf,
            }
        })
        .collect()
}

fn weighted_bitrate(
    assignments: &[TrellisAssignment],
    shot_results: &[ShotResult],
    total_duration: f64,
) -> f64 {
    assignments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let weight = shot_results[i].shot.duration.as_secs_f64() / total_duration;
            a.bitrate * weight
        })
        .sum()
}

fn total_bitrate_at_lambda(shot_results: &[ShotResult], total_duration: f64, lambda: f64) -> f64 {
    let assignments = assign_at_lambda(shot_results, lambda);
    weighted_bitrate(&assignments, shot_results, total_duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use viser_ffmpeg::{Codec, RES_720P, RES_1080P, Resolution};
    use viser_hull::{Hull, Point};
    use viser_shot::Shot;

    fn shot(dur_secs: f64, idx: i32) -> Shot {
        Shot {
            index: idx,
            start: Duration::ZERO,
            end: Duration::from_secs_f64(dur_secs),
            duration: Duration::from_secs_f64(dur_secs),
            score: 0.0,
        }
    }

    fn point(bitrate: f64, vmaf: f64, res: Resolution) -> Point {
        Point { resolution: res, codec: Codec::X264, crf: 23, bitrate, vmaf, psnr: 0.0, ssim: 0.0 }
    }

    fn sr(points: Vec<Point>, shot_idx: i32, shot_dur: f64) -> ShotResult {
        ShotResult { shot: shot(shot_dur, shot_idx), hull: Hull { points }, points: vec![] }
    }

    #[test]
    fn test_trellis_empty_shots() {
        let r = trellis_optimize(&[], &TrellisOpts { target_bitrate: 1000.0, tolerance: 0.05 });
        assert!(r.is_empty());
    }

    #[test]
    fn test_trellis_zero_target() {
        let s = sr(vec![point(500.0, 80.0, RES_1080P)], 0, 1.0);
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 0.0, tolerance: 0.05 });
        assert!(r.is_empty());
    }

    #[test]
    fn test_trellis_single_shot_single_point() {
        let s = sr(vec![point(1000.0, 90.0, RES_1080P)], 0, 1.0);
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 1000.0, tolerance: 0.05 });
        assert_eq!(r.len(), 1);
        assert!((r[0].bitrate - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn test_trellis_single_shot_picks_best_for_lambda() {
        let s = sr(
            vec![
                point(500.0, 70.0, RES_720P),
                point(1000.0, 85.0, RES_1080P),
                point(2000.0, 93.0, RES_1080P),
            ],
            0,
            1.0,
        );
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 1000.0, tolerance: 0.01 });
        assert_eq!(r.len(), 1);
        assert!((r[0].bitrate - 1000.0).abs() < 100.0);
    }

    #[test]
    fn test_trellis_two_shots_equal_duration() {
        // Shot 0: complex (needs 2000kbps for 90 VMAF)
        // Shot 1: simple (needs 500kbps for 90 VMAF)
        let s0 = sr(
            vec![
                point(1000.0, 80.0, RES_1080P),
                point(2000.0, 90.0, RES_1080P),
                point(4000.0, 95.0, RES_1080P),
            ],
            0,
            1.0,
        );
        let s1 = sr(
            vec![
                point(300.0, 80.0, RES_1080P),
                point(500.0, 90.0, RES_1080P),
                point(800.0, 95.0, RES_1080P),
            ],
            1,
            1.0,
        );
        let r =
            trellis_optimize(&[s0, s1], &TrellisOpts { target_bitrate: 1500.0, tolerance: 0.05 });
        assert_eq!(r.len(), 2);
        // Simple shot should get less bitrate
        assert!(r[0].bitrate > r[1].bitrate);
    }

    #[test]
    fn test_trellis_respects_duration_weighting() {
        // Shot 0: 9s of simple content
        // Shot 1: 1s of complex content
        let s0 = sr(vec![point(500.0, 85.0, RES_1080P), point(1000.0, 93.0, RES_1080P)], 0, 9.0);
        let s1 = sr(vec![point(2000.0, 80.0, RES_1080P), point(4000.0, 93.0, RES_1080P)], 1, 1.0);
        let r =
            trellis_optimize(&[s0, s1], &TrellisOpts { target_bitrate: 600.0, tolerance: 0.05 });
        assert_eq!(r.len(), 2);
        // Weighted average should be dominated by the 9s shot
        let weighted = (r[0].bitrate * 9.0 + r[1].bitrate * 1.0) / 10.0;
        assert!((weighted - 600.0).abs() < 300.0);
    }

    #[test]
    fn test_trellis_all_identical_shots() {
        let s = sr(
            vec![
                point(500.0, 70.0, RES_720P),
                point(1000.0, 85.0, RES_1080P),
                point(2000.0, 93.0, RES_1080P),
            ],
            0,
            1.0,
        );
        let r = trellis_optimize(
            &[s.clone(), s.clone(), s],
            &TrellisOpts { target_bitrate: 1000.0, tolerance: 0.05 },
        );
        assert_eq!(r.len(), 3);
        // All should pick the same point (same benefit per bit)
        assert!((r[0].bitrate - r[1].bitrate).abs() < 1e-9);
        assert!((r[0].bitrate - r[2].bitrate).abs() < 1e-9);
    }

    #[test]
    fn test_trellis_empty_hull_shot() {
        let s0 = sr(vec![point(1000.0, 85.0, RES_1080P)], 0, 1.0);
        let s1 = ShotResult { shot: shot(1.0, 1), hull: Hull { points: vec![] }, points: vec![] };
        let r =
            trellis_optimize(&[s0, s1], &TrellisOpts { target_bitrate: 1000.0, tolerance: 0.05 });
        assert_eq!(r.len(), 2);
        // Empty hull shot falls back to default (1080p x264 CRF23, bitrate 0)
        assert!((r[0].bitrate - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn test_trellis_tight_tolerance() {
        let s = sr(
            vec![
                point(100.0, 50.0, RES_1080P),
                point(300.0, 70.0, RES_1080P),
                point(800.0, 85.0, RES_1080P),
                point(2000.0, 93.0, RES_1080P),
                point(5000.0, 97.0, RES_1080P),
            ]
            .into_iter()
            .map(|p| Point { ..p })
            .collect(),
            0,
            1.0,
        );
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 800.0, tolerance: 0.001 });
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn test_trellis_zero_tolerance_falls_back_to_default() {
        let s = sr(vec![point(1000.0, 85.0, RES_1080P)], 0, 1.0);
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 1000.0, tolerance: 0.0 });
        // Zero tolerance should fall back to default 0.05
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn test_trellis_large_lambda_search_bounds() {
        // Target bitrate is impossibly low — lambda should grow but hit the 1e6 cap
        let s = sr(vec![point(5000.0, 95.0, RES_1080P), point(10000.0, 98.0, RES_1080P)], 0, 1.0);
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 100.0, tolerance: 0.01 });
        // Should converge to best_assignments even if never hits tolerance
        assert_eq!(r.len(), 1);
        assert!(r[0].bitrate > 0.0);
    }

    #[test]
    fn test_trellis_target_zero_with_all_zero() {
        // All bitrates are zero (failed encodes), but target is also zero
        let s = sr(vec![point(0.0, 0.0, RES_1080P)], 0, 1.0);
        let r = trellis_optimize(&[s], &TrellisOpts { target_bitrate: 0.0, tolerance: 0.05 });
        assert!(r.is_empty());
    }
}
