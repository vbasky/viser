use crate::{ShotResult, TrellisAssignment};

#[derive(Debug, Clone)]
pub struct TrellisOpts {
    pub target_bitrate: f64,
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
