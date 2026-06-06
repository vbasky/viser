//! Bitrate ladder selection with crossover enforcement.
//!
//! Picks the best N rungs from a convex hull (Pareto frontier) using greedy
//! VMAF-target selection, while enforcing resolution crossovers and bitrate/quality
//! constraints. Also provides pre-built fixed ladders (Netflix, Apple HLS) for baseline
//! comparison.
//!
//! Part of the `viser` video-encoding-optimizer workspace.

mod fixed;

pub use fixed::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::Resolution;
use viser_hull::{Hull, Point};

/// One level in a bitrate ladder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rung {
    /// The hull point selected for this rung.
    #[serde(flatten)]
    pub point: Point,
    /// Rung number, with 0 being the lowest quality.
    pub index: i32, // rung number (0 = lowest quality)
}

/// Ordered set of rungs from lowest to highest quality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ladder {
    /// Rungs ordered by ascending bitrate.
    pub rungs: Vec<Rung>,
}

/// Constraints and target count controlling ladder selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opts {
    /// Target number of rungs to select (e.g. 6).
    pub num_rungs: i32, // target number of rungs (e.g., 6)
    /// Minimum bitrate in kbps; candidates below this are dropped.
    pub min_bitrate: f64, // minimum bitrate in kbps
    /// Maximum bitrate in kbps; candidates above this (minus audio) are dropped.
    pub max_bitrate: f64, // maximum bitrate in kbps
    /// Minimum acceptable VMAF quality; candidates below this are dropped.
    pub min_vmaf: f64, // minimum acceptable quality
    /// Maximum VMAF quality target, capping the top of the target range.
    pub max_vmaf: f64, // maximum quality target
    /// Audio bitrate overhead (kbps) reserved within the delivery budget.
    pub audio_bitrate_kbps: f64, // audio overhead in delivery budget
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            num_rungs: 6,
            min_bitrate: 200.0,
            max_bitrate: 8000.0,
            min_vmaf: 40.0,
            max_vmaf: 97.0,
            audio_bitrate_kbps: 0.0,
        }
    }
}

/// Picks the best N rungs from the convex hull to form a bitrate ladder.
pub fn select(h: &Hull, opts: &Opts) -> Ladder {
    if h.points.is_empty() || opts.num_rungs <= 0 {
        return Ladder { rungs: vec![] };
    }

    // Build crossover map
    let crossover_min = build_crossover_map(h);

    // Filter hull points by constraints + crossover enforcement
    let mut candidates: Vec<Point> = Vec::new();
    for p in &h.points {
        if p.bitrate < opts.min_bitrate || p.bitrate > opts.max_bitrate - opts.audio_bitrate_kbps {
            continue;
        }
        if p.vmaf < opts.min_vmaf {
            continue;
        }
        if let Some(&min_br) = crossover_min.get(&p.resolution) {
            if p.bitrate < min_br {
                continue;
            }
        }
        candidates.push(p.clone());
    }

    if candidates.is_empty() {
        return Ladder { rungs: vec![] };
    }

    if candidates.len() <= opts.num_rungs as usize {
        return to_ladder(candidates);
    }

    // Determine VMAF range from candidates
    let min_q = candidates.first().map(|p| p.vmaf).unwrap_or(0.0);
    let mut max_q = candidates.last().map(|p| p.vmaf).unwrap_or(100.0);
    if opts.max_vmaf > 0.0 && max_q > opts.max_vmaf {
        max_q = opts.max_vmaf;
    }
    let min_q = min_q.min(max_q);

    // Generate evenly-spaced quality targets
    let num = opts.num_rungs as usize;
    let targets: Vec<f64> = if num == 1 {
        vec![(min_q + max_q) / 2.0]
    } else {
        let step = (max_q - min_q) / (num - 1) as f64;
        (0..num).map(|i| min_q + step * i as f64).collect()
    };

    // Greedy selection: for each target, find closest unused candidate
    let mut used = vec![false; candidates.len()];
    let mut selected = Vec::new();

    for target in &targets {
        let mut best_idx = None;
        let mut best_dist = f64::MAX;

        for (i, p) in candidates.iter().enumerate() {
            if used[i] {
                continue;
            }
            let dist = (p.vmaf - target).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            used[idx] = true;
            selected.push(candidates[idx].clone());
        }
    }

    to_ladder(selected)
}

fn build_crossover_map(h: &Hull) -> std::collections::HashMap<Resolution, f64> {
    let mut crossovers = std::collections::HashMap::new();
    for co in h.crossovers() {
        crossovers.insert(co.to, co.bitrate);
    }
    crossovers
}

fn to_ladder(mut points: Vec<Point>) -> Ladder {
    points.sort_by(|a, b| a.bitrate.partial_cmp(&b.bitrate).unwrap());
    let rungs =
        points.into_iter().enumerate().map(|(i, p)| Rung { point: p, index: i as i32 }).collect();
    Ladder { rungs }
}

impl Ladder {
    /// Returns the (lowest, highest) bitrate in kbps, or `(0.0, 0.0)` if empty.
    pub fn bitrate_range(&self) -> (f64, f64) {
        if self.rungs.is_empty() {
            return (0.0, 0.0);
        }
        (self.rungs.first().unwrap().point.bitrate, self.rungs.last().unwrap().point.bitrate)
    }

    /// Returns the (lowest, highest) VMAF quality, or `(0.0, 0.0)` if empty.
    pub fn quality_range(&self) -> (f64, f64) {
        if self.rungs.is_empty() {
            return (0.0, 0.0);
        }
        (self.rungs.first().unwrap().point.vmaf, self.rungs.last().unwrap().point.vmaf)
    }

    /// Percent bitrate savings of the top rung versus a fixed top-rung bitrate (kbps).
    ///
    /// Returns 0.0 if the ladder is empty or the top rung is not cheaper.
    pub fn savings(&self, fixed_bitrate: f64) -> f64 {
        if self.rungs.is_empty() || fixed_bitrate <= 0.0 {
            return 0.0;
        }
        let top = &self.rungs.last().unwrap().point;
        if top.bitrate >= fixed_bitrate {
            return 0.0;
        }
        (1.0 - top.bitrate / fixed_bitrate) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::{Codec, Resolution};
    use viser_hull::{Hull, Point};

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

    fn hull_for(points: Vec<Point>) -> Hull {
        viser_hull::compute_upper(&points)
    }

    #[test]
    fn test_select_empty_hull() {
        let h = Hull { points: vec![] };
        let ladder = select(&h, &Opts::default());
        assert!(ladder.rungs.is_empty());
    }

    #[test]
    fn test_select_zero_rungs() {
        let h = hull_for(vec![point(500.0, 80.0), point(1000.0, 90.0)]);
        let ladder = select(&h, &Opts { num_rungs: 0, ..Opts::default() });
        assert!(ladder.rungs.is_empty());
    }

    #[test]
    fn test_select_fewer_candidates_than_rungs() {
        let h = hull_for(vec![point(500.0, 80.0), point(1000.0, 90.0)]);
        let ladder = select(&h, &Opts { num_rungs: 6, ..Opts::default() });
        assert!(!ladder.rungs.is_empty());
        assert!(ladder.rungs.len() <= 2);
    }

    #[test]
    fn test_select_filters_outside_bitrate_range() {
        let h = hull_for(vec![
            point(100.0, 50.0),
            point(500.0, 80.0),
            point(1000.0, 90.0),
            point(10000.0, 98.0),
        ]);
        let opts =
            Opts { num_rungs: 4, min_bitrate: 200.0, max_bitrate: 5000.0, ..Opts::default() };
        let ladder = select(&h, &opts);
        for rung in &ladder.rungs {
            assert!(rung.point.bitrate >= 200.0);
            assert!(rung.point.bitrate <= 5000.0);
        }
    }

    #[test]
    fn test_select_filters_below_min_vmaf() {
        let h = hull_for(vec![
            point(200.0, 30.0),
            point(500.0, 60.0),
            point(1000.0, 85.0),
            point(2000.0, 95.0),
        ]);
        let opts = Opts { num_rungs: 4, min_vmaf: 50.0, ..Opts::default() };
        let ladder = select(&h, &opts);
        for rung in &ladder.rungs {
            assert!(rung.point.vmaf >= 50.0);
        }
    }

    #[test]
    fn test_select_output_sorted() {
        let h = hull_for(vec![
            point(500.0, 70.0),
            point(1000.0, 85.0),
            point(2000.0, 93.0),
            point(5000.0, 98.0),
        ]);
        let ladder = select(&h, &Opts::default());
        assert!(ladder.rungs.windows(2).all(|w| w[0].point.bitrate <= w[1].point.bitrate));
    }

    #[test]
    fn test_select_rung_indices() {
        let h = hull_for(vec![point(500.0, 70.0), point(1000.0, 85.0), point(2000.0, 93.0)]);
        let ladder = select(&h, &Opts { num_rungs: 3, ..Opts::default() });
        for (i, rung) in ladder.rungs.iter().enumerate() {
            assert_eq!(rung.index as usize, i);
        }
    }

    #[test]
    fn test_bitrate_range_empty() {
        let ladder = Ladder { rungs: vec![] };
        assert_eq!(ladder.bitrate_range(), (0.0, 0.0));
    }

    #[test]
    fn test_bitrate_range() {
        let rungs = vec![
            Rung { point: point(500.0, 70.0), index: 0 },
            Rung { point: point(2000.0, 93.0), index: 1 },
        ];
        let ladder = Ladder { rungs };
        assert_eq!(ladder.bitrate_range(), (500.0, 2000.0));
    }

    #[test]
    fn test_quality_range_empty() {
        let ladder = Ladder { rungs: vec![] };
        assert_eq!(ladder.quality_range(), (0.0, 0.0));
    }

    #[test]
    fn test_quality_range() {
        let rungs = vec![
            Rung { point: point(500.0, 70.0), index: 0 },
            Rung { point: point(2000.0, 93.0), index: 1 },
        ];
        let ladder = Ladder { rungs };
        assert_eq!(ladder.quality_range(), (70.0, 93.0));
    }

    #[test]
    fn test_savings_empty() {
        let ladder = Ladder { rungs: vec![] };
        assert_eq!(ladder.savings(8000.0), 0.0);
    }

    #[test]
    fn test_savings_zero_fixed() {
        let rungs = vec![Rung { point: point(2000.0, 93.0), index: 0 }];
        let ladder = Ladder { rungs };
        assert_eq!(ladder.savings(0.0), 0.0);
    }

    #[test]
    fn test_savings_no_savings() {
        let rungs = vec![Rung { point: point(8000.0, 93.0), index: 0 }];
        let ladder = Ladder { rungs };
        assert_eq!(ladder.savings(8000.0), 0.0);
    }

    #[test]
    fn test_savings_calculated() {
        let rungs = vec![Rung { point: point(4000.0, 93.0), index: 0 }];
        let ladder = Ladder { rungs };
        let s = ladder.savings(8000.0);
        assert!((s - 50.0).abs() < 1e-9);
    }

    #[test]
    fn test_netflix_old_ladder() {
        let ladder = netflix_old();
        assert_eq!(ladder.name, "Netflix Fixed (2015)");
        assert_eq!(ladder.rungs.len(), 10);
        assert!((ladder.total_bitrate() - 20170.0).abs() < 1e-9);
        assert!((ladder.top_bitrate() - 5800.0).abs() < 1e-9);
    }

    #[test]
    fn test_apple_hls_ladder() {
        let ladder = apple_hls();
        assert_eq!(ladder.name, "Apple HLS (2024)");
        assert_eq!(ladder.rungs.len(), 9);
        assert!((ladder.total_bitrate() - 25640.0).abs() < 1e-9);
        assert!((ladder.top_bitrate() - 7800.0).abs() < 1e-9);
    }

    #[test]
    fn test_select_respects_max_vmaf() {
        let h = hull_for(vec![
            point(500.0, 70.0),
            point(1000.0, 85.0),
            point(2000.0, 90.0),
            point(3000.0, 93.0),
            point(5000.0, 98.0),
        ]);
        let opts = Opts { num_rungs: 3, max_vmaf: 90.0, ..Opts::default() };
        let ladder = select(&h, &opts);
        // max_vmaf caps quality target range so targets are [70, 80, 90]
        // without max_vmaf, targets would reach higher, changing selection
        assert!(!ladder.rungs.is_empty());
        // The highest vmaf candidate closest to target=90.0 is 90.0 itself
        assert!(ladder.rungs.last().unwrap().point.vmaf <= 90.0 + 1e-9);
    }

    #[test]
    fn test_opts_default() {
        let opts = Opts::default();
        assert_eq!(opts.num_rungs, 6);
        assert!((opts.min_bitrate - 200.0).abs() < 1e-9);
        assert!((opts.max_bitrate - 8000.0).abs() < 1e-9);
        assert!((opts.min_vmaf - 40.0).abs() < 1e-9);
        assert!((opts.max_vmaf - 97.0).abs() < 1e-9);
    }
}
