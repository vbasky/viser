//! Bitrate ladder selection with crossover enforcement.
//!
//! Picks the best N rungs from a convex hull (Pareto frontier) using greedy
//! VMAF-target selection, while enforcing resolution crossovers and bitrate/quality
//! constraints. Also provides pre-built fixed ladders (Netflix, Apple HLS) for baseline
//! comparison.
//!
//! Part of the `viser` video-encoding-optimizer workspace.

mod blend;
mod fixed;
pub mod manifest;

pub use blend::*;
pub use fixed::*;
pub use manifest::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::Resolution;
use viser_hull::{Hull, Point};

/// Storage and CDN delivery cost parameters for cost-aware ladder optimization.
///
/// When `CostOpts` is set on `Opts`, the ladder is pruned after initial
/// selection so that the total monthly cost (storage + delivery) stays within
/// `max_monthly_cost` — removing the rungs with the worst incremental quality
/// per dollar first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostOpts {
    /// Storage cost per gigabyte per month (e.g., 0.023 for S3 Standard).
    pub storage_cost_per_gb_month: f64,
    /// CDN delivery cost per gigabyte transferred (e.g., 0.08 for CloudFront).
    pub cdn_cost_per_gb: f64,
    /// Expected monthly viewer-hours for this content (e.g., 10_000).
    pub viewing_hours_per_month: f64,
    /// Maximum acceptable total monthly cost in dollars. When set to a value
    /// > 0, the ladder is pruned to stay within this budget by dropping rungs
    /// with the worst marginal quality-per-dollar ratio.
    pub max_monthly_cost: f64,
}

impl Default for CostOpts {
    fn default() -> Self {
        Self {
            storage_cost_per_gb_month: 0.023,
            cdn_cost_per_gb: 0.08,
            viewing_hours_per_month: 0.0,
            max_monthly_cost: 0.0,
        }
    }
}

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

/// ABR (adaptive bitrate) configuration for client-aware ladder selection.
///
/// When `target_bitrates` is set, the ladder rungs are anchored to those bitrate
/// targets instead of evenly-spaced VMAF quality targets. This lets the resulting
/// ladder align with common ABR client bandwidths or a content-delivery strategy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AbrOpts {
    /// Target bitrates in kbps for each rung. When `Some`, the ladder is built by
    /// selecting the hull point closest to each target bitrate (crossover constraints
    /// and VMAF bounds still apply).
    pub target_bitrates: Option<Vec<f64>>,
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
    /// ABR-aware selection options. When `abr.target_bitrates` is set, the ladder is
    /// built by matching those bitrate targets instead of evenly-spaced VMAF targets.
    #[serde(default)]
    pub abr: AbrOpts,
    /// Cost-aware optimization options. When `cost.max_monthly_cost > 0`, the
    /// ladder is pruned to stay within the budget by removing the least
    /// cost-effective rungs.
    #[serde(default)]
    pub cost: CostOpts,
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
            abr: AbrOpts::default(),
            cost: CostOpts::default(),
        }
    }
}

/// Generates logarithmically-spaced bitrate targets between `min` and `max`.
///
/// Each rung is ~(`max`/`min`)^(1/(`n`-1)) times the previous one, so the spacing
/// is denser at low bitrates where ABR clients are most sensitive and wider at
/// high bitrates where large jumps are less noticeable.
///
/// Returns an empty vec when `n == 0`, `min <= 0`, or `max <= min`.
pub fn logarithmic_bitrates(min: f64, max: f64, n: usize) -> Vec<f64> {
    if n == 0 || min <= 0.0 || max <= min {
        return vec![];
    }
    if n == 1 {
        return vec![(min + max) / 2.0];
    }
    let ratio = (max / min).powf(1.0 / (n - 1) as f64);
    (0..n).map(|i| (min * ratio.powi(i as i32)).round()).collect()
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
        if let Some(&min_br) = crossover_min.get(&p.resolution)
            && p.bitrate < min_br
        {
            continue;
        }
        candidates.push(p.clone());
    }

    if candidates.is_empty() {
        return Ladder { rungs: vec![] };
    }

    // ── Build initial ladder ──
    let initial = if let Some(bitrate_targets) = &opts.abr.target_bitrates
        && !bitrate_targets.is_empty()
    {
        // ABR bitrate-target mode
        let mut used = vec![false; candidates.len()];
        let mut selected = Vec::new();
        for target in bitrate_targets {
            let mut best_idx = None;
            let mut best_dist = f64::MAX;
            for (i, p) in candidates.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let dist = (p.bitrate - target).abs();
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
    } else if candidates.len() <= opts.num_rungs as usize {
        // VMAF-target mode: fewer candidates than rungs → take all
        to_ladder(candidates)
    } else {
        // VMAF-target mode: greedy selection
        let min_q = candidates.first().map(|p| p.vmaf).unwrap_or(0.0);
        let mut max_q = candidates.last().map(|p| p.vmaf).unwrap_or(100.0);
        if opts.max_vmaf > 0.0 && max_q > opts.max_vmaf {
            max_q = opts.max_vmaf;
        }
        let min_q = min_q.min(max_q);

        let num = opts.num_rungs as usize;
        let targets: Vec<f64> = if num == 1 {
            vec![(min_q + max_q) / 2.0]
        } else {
            let step = (max_q - min_q) / (num - 1) as f64;
            (0..num).map(|i| min_q + step * i as f64).collect()
        };

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
    };

    // ── Cost-aware pruning (applied to any ladder, regardless of selection path) ──
    if initial.rungs.len() > 1
        && opts.cost.max_monthly_cost > 0.0
        && opts.cost.viewing_hours_per_month > 0.0
    {
        let mut pruned = initial;
        cost_prune(&mut pruned, &opts.cost, h);
        pruned
    } else {
        initial
    }
}

fn cost_prune(ladder: &mut Ladder, cost: &CostOpts, _hull: &Hull) {
    let duration_secs = 120.0;

    loop {
        let current_cost = ladder.monthly_cost(cost, duration_secs);
        if current_cost <= cost.max_monthly_cost || ladder.rungs.len() <= 1 {
            break;
        }

        // Find the rung to drop: the one with the highest cost per VMAF point
        // (worst value). For the lowest rung, we consider its VMAF gap to the
        // next rung. For the highest rung, we consider its VMAF gap from the
        // previous rung. For middle rungs, we average both gaps.
        let mut worst_idx = 0;
        let mut worst_ratio = f64::MIN;

        for i in 0..ladder.rungs.len() {
            let rung = &ladder.rungs[i].point;
            // Estimate file size in GB for this rung.
            let file_gb = rung_bitrate_to_gb(rung.bitrate, duration_secs);
            let monthly_storage = file_gb * cost.storage_cost_per_gb_month;

            // Estimate delivery cost: assume viewing_hours_per_month evenly
            // split among remaining rungs for simplicity.
            let viewing_share = cost.viewing_hours_per_month / ladder.rungs.len() as f64 * 3600.0; // seconds
            let delivery_gb = rung_bitrate_to_gb(rung.bitrate, viewing_share);
            let monthly_delivery = delivery_gb * cost.cdn_cost_per_gb;

            let marginal_cost = monthly_storage + monthly_delivery;

            // Marginal VMAF: how much quality do we lose by dropping this rung?
            let marginal_vmaf = if i == 0 {
                // Lowest rung: VMAF gap to the next rung up.
                ladder.rungs[1].point.vmaf - rung.vmaf
            } else if i == ladder.rungs.len() - 1 {
                // Highest rung: VMAF gap from the previous rung.
                rung.vmaf - ladder.rungs[i - 1].point.vmaf
            } else {
                // Middle rung: average of both gaps.
                let gap_down = rung.vmaf - ladder.rungs[i - 1].point.vmaf;
                let gap_up = ladder.rungs[i + 1].point.vmaf - rung.vmaf;
                (gap_down + gap_up) / 2.0
            };

            if marginal_cost > 0.0 && marginal_vmaf > 0.0 {
                let ratio = marginal_cost / marginal_vmaf;
                if ratio > worst_ratio {
                    worst_ratio = ratio;
                    worst_idx = i;
                }
            }
        }

        ladder.rungs.remove(worst_idx);
    }

    // Re-index after pruning.
    for (i, rung) in ladder.rungs.iter_mut().enumerate() {
        rung.index = i as i32;
    }
}

/// Converts a bitrate (kbps) over a duration (seconds) to gigabytes.
fn rung_bitrate_to_gb(bitrate_kbps: f64, duration_secs: f64) -> f64 {
    bitrate_kbps * duration_secs / 8.0 / 1024.0 / 1024.0
}

fn build_crossover_map(h: &Hull) -> std::collections::HashMap<Resolution, f64> {
    let mut crossovers = std::collections::HashMap::new();
    for co in h.crossovers() {
        crossovers.insert(co.to, co.bitrate);
    }
    crossovers
}

fn to_ladder(mut points: Vec<Point>) -> Ladder {
    points.sort_by(|a, b| a.bitrate.total_cmp(&b.bitrate));
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
        (
            self.rungs.first().expect("non-empty after check").point.bitrate,
            self.rungs.last().expect("non-empty after check").point.bitrate,
        )
    }

    /// Returns the (lowest, highest) VMAF quality, or `(0.0, 0.0)` if empty.
    pub fn quality_range(&self) -> (f64, f64) {
        if self.rungs.is_empty() {
            return (0.0, 0.0);
        }
        (
            self.rungs.first().expect("non-empty after check").point.vmaf,
            self.rungs.last().expect("non-empty after check").point.vmaf,
        )
    }

    /// Percent bitrate savings of the top rung versus a fixed top-rung bitrate (kbps).
    ///
    /// Returns 0.0 if the ladder is empty or the top rung is not cheaper.
    pub fn savings(&self, fixed_bitrate: f64) -> f64 {
        if self.rungs.is_empty() || fixed_bitrate <= 0.0 {
            return 0.0;
        }
        let top = &self.rungs.last().expect("non-empty after check").point;
        if top.bitrate >= fixed_bitrate {
            return 0.0;
        }
        (1.0 - top.bitrate / fixed_bitrate) * 100.0
    }

    /// Estimated total monthly cost in dollars: storage for all rungs plus CDN
    /// delivery for the expected viewing hours (assumes all viewing is at the
    /// highest rung as a conservative estimate).
    ///
    /// Returns 0.0 when the ladder is empty.
    pub fn monthly_cost(&self, cost: &CostOpts, duration_secs: f64) -> f64 {
        if self.rungs.is_empty() || cost.viewing_hours_per_month <= 0.0 {
            return 0.0;
        }
        let storage = self.monthly_storage_cost(cost.storage_cost_per_gb_month, duration_secs);
        let delivery = self.monthly_delivery_cost(cost, duration_secs);
        storage + delivery
    }

    /// Monthly storage cost for all rungs combined.
    pub fn monthly_storage_cost(&self, cost_per_gb: f64, duration_secs: f64) -> f64 {
        self.rungs
            .iter()
            .map(|r| rung_bitrate_to_gb(r.point.bitrate, duration_secs) * cost_per_gb)
            .sum()
    }

    /// Monthly CDN delivery cost. A conservative estimate that assumes all
    /// viewing hours are served from the highest rung.
    pub fn monthly_delivery_cost(&self, cost: &CostOpts, duration_secs: f64) -> f64 {
        let Some(top) = self.rungs.last() else {
            return 0.0;
        };
        // Total viewing seconds per month.
        let viewing_secs = cost.viewing_hours_per_month * 3600.0;
        // How many full-duration streams fit in the viewing budget.
        let streams = viewing_secs / duration_secs.max(1.0);
        let data_per_stream_gb = rung_bitrate_to_gb(top.point.bitrate, duration_secs);
        data_per_stream_gb * streams * cost.cdn_cost_per_gb
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

    // ── Property-based tests ──
    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_point() -> impl Strategy<Value = Point> {
            let res = prop_oneof![
                Just(Resolution::new(640, 360)),
                Just(Resolution::new(1280, 720)),
                Just(Resolution::new(1920, 1080)),
            ];
            (0.0f64..10000.0f64, 0.0f64..100.0f64, res, 0i32..51i32).prop_map(
                |(bitrate, vmaf, res, crf)| Point {
                    resolution: res,
                    codec: Codec::X264,
                    crf,
                    bitrate,
                    vmaf,
                    psnr: 0.0,
                    ssim: 0.0,
                },
            )
        }

        fn arb_opts() -> impl Strategy<Value = Opts> {
            (
                1i32..12i32,
                0.0f64..2000.0f64,
                2000.0f64..20000.0f64,
                0.0f64..60.0f64,
                70.0f64..100.0f64,
                0.0f64..500.0f64,
            )
                .prop_map(|(num_rungs, min_br, max_br, min_q, max_q, audio)| Opts {
                    num_rungs,
                    min_bitrate: min_br,
                    max_bitrate: max_br.max(min_br + 100.0),
                    min_vmaf: min_q,
                    max_vmaf: max_q.max(min_q + 1.0),
                    audio_bitrate_kbps: audio,
                    abr: AbrOpts::default(),
                    cost: CostOpts::default(),
                })
        }

        proptest! {
            /// Invariant: ladder rungs are sorted by bitrate ascending.
            #[test]
            fn ladder_rungs_sorted_by_bitrate(
                points in proptest::collection::vec(arb_point(), 0..60),
                opts in arb_opts(),
            ) {
                let hull = viser_hull::compute_upper(&points);
                let ladder = select(&hull, &opts);
                for w in ladder.rungs.windows(2) {
                    assert!(w[0].point.bitrate <= w[1].point.bitrate,
                        "ladder not sorted: {} kbps before {} kbps",
                        w[0].point.bitrate, w[1].point.bitrate);
                }
            }

            /// Invariant: ladder never has more rungs than requested.
            #[test]
            fn ladder_rung_count_within_limit(
                points in proptest::collection::vec(arb_point(), 0..60),
                opts in arb_opts(),
            ) {
                let hull = viser_hull::compute_upper(&points);
                let ladder = select(&hull, &opts);
                assert!(ladder.rungs.len() <= opts.num_rungs as usize,
                    "got {} rungs, asked for {}", ladder.rungs.len(), opts.num_rungs);
            }

            /// Invariant: all rungs are within the specified bitrate bounds.
            #[test]
            fn ladder_rungs_within_bitrate_bounds(
                points in proptest::collection::vec(arb_point(), 0..60),
                opts in arb_opts(),
            ) {
                let hull = viser_hull::compute_upper(&points);
                let ladder = select(&hull, &opts);
                let effective_max = opts.max_bitrate - opts.audio_bitrate_kbps;
                for rung in &ladder.rungs {
                    assert!(rung.point.bitrate >= opts.min_bitrate - 1e-9,
                        "rung bitrate {:.1} below min {:.1}",
                        rung.point.bitrate, opts.min_bitrate);
                    assert!(rung.point.bitrate <= effective_max + 1e-9,
                        "rung bitrate {:.1} above max {:.1} (effective after audio {:.1})",
                        rung.point.bitrate, effective_max, opts.max_bitrate);
                }
            }

            /// Invariant: all rungs are within VMAF bounds.
            #[test]
            fn ladder_rungs_within_vmaf_bounds(
                points in proptest::collection::vec(arb_point(), 0..60),
                opts in arb_opts(),
            ) {
                let hull = viser_hull::compute_upper(&points);
                let ladder = select(&hull, &opts);
                for rung in &ladder.rungs {
                    assert!(rung.point.vmaf >= opts.min_vmaf - 1e-9,
                        "rung vmaf {:.1} below min {:.1}", rung.point.vmaf, opts.min_vmaf);
                }
            }

            /// Invariant: rung indices form a contiguous sequence 0..N-1.
            #[test]
            fn ladder_rung_indices_contiguous(
                points in proptest::collection::vec(arb_point(), 0..60),
                opts in arb_opts(),
            ) {
                let hull = viser_hull::compute_upper(&points);
                let ladder = select(&hull, &opts);
                let indices: Vec<i32> = ladder.rungs.iter().map(|r| r.index).collect();
                let expected: Vec<i32> = (0..ladder.rungs.len() as i32).collect();
                assert_eq!(indices, expected, "rung indices not contiguous");
            }

            /// Invariant: bitrate_range first <= last (after sorting the rungs).
            #[test]
            fn ladder_bitrate_range_ordered(
                points in proptest::collection::vec(arb_point(), 1..60),
            ) {
                let mut sorted = points;
                sorted.sort_by(|a, b| a.bitrate.partial_cmp(&b.bitrate).unwrap());
                let ladder = Ladder {
                    rungs: sorted.iter().enumerate().map(|(i, p)| Rung {
                        point: p.clone(), index: i as i32
                    }).collect()
                };
                let (lo, hi) = ladder.bitrate_range();
                assert!(lo <= hi, "bitrate range out of order: {lo} > {hi}");
            }
            #[test]
            fn ladder_savings_bounded(
                points in proptest::collection::vec(arb_point(), 1..60),
                fixed_bitrate in 1000.0f64..20000.0f64,
            ) {
                let ladder = Ladder {
                    rungs: points.iter().enumerate().map(|(i, p)| Rung {
                        point: p.clone(), index: i as i32
                    }).collect()
                };
                let s = ladder.savings(fixed_bitrate);
                assert!(s >= 0.0, "savings negative: {s}");
                assert!(s <= 100.0, "savings > 100%: {s}");
            }

        }
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
        assert!(opts.abr.target_bitrates.is_none());
    }

    // ── ABR integration ──

    #[test]
    fn test_select_bitrate_targets_selects_closest() {
        let h = hull_for(vec![
            point(500.0, 70.0),
            point(1200.0, 85.0),
            point(2500.0, 93.0),
            point(5000.0, 97.0),
        ]);
        let opts = Opts {
            num_rungs: 4,
            abr: AbrOpts { target_bitrates: Some(vec![400.0, 1000.0, 2000.0, 4000.0]) },
            ..Opts::default()
        };
        let ladder = select(&h, &opts);
        assert_eq!(ladder.rungs.len(), 4);
        // Each rung should be near its target bitrate
        let brs: Vec<f64> = ladder.rungs.iter().map(|r| r.point.bitrate).collect();
        assert!((brs[0] - 500.0).abs() < 1.0, "rung 0: {}", brs[0]); // closest to 400
        assert!((brs[1] - 1200.0).abs() < 1.0, "rung 1: {}", brs[1]); // closest to 1000
        assert!((brs[2] - 2500.0).abs() < 1.0, "rung 2: {}", brs[2]); // closest to 2000
        assert!((brs[3] - 5000.0).abs() < 1.0, "rung 3: {}", brs[3]); // closest to 4000
    }

    #[test]
    fn test_select_bitrate_targets_respects_constraints() {
        // Bitrate targets should still respect min_bitrate, max_bitrate, min_vmaf.
        let h = hull_for(vec![
            point(100.0, 30.0),
            point(500.0, 70.0),
            point(1000.0, 85.0),
            point(2000.0, 93.0),
            point(10000.0, 99.0),
        ]);
        let opts = Opts {
            num_rungs: 5,
            min_bitrate: 200.0,
            max_bitrate: 5000.0,
            min_vmaf: 50.0,
            abr: AbrOpts { target_bitrates: Some(vec![150.0, 500.0, 1000.0, 3000.0, 9000.0]) },
            ..Opts::default()
        };
        let ladder = select(&h, &opts);
        for rung in &ladder.rungs {
            assert!(
                rung.point.bitrate >= 200.0 - 1e-9,
                "bitrate {:.1} below min",
                rung.point.bitrate
            );
            assert!(rung.point.vmaf >= 50.0 - 1e-9, "vmaf {:.1} below min", rung.point.vmaf);
        }
        // The 100 kbps and 10000 kbps points should be excluded by constraints.
        // The 9000 target will still match a 5000 kbps point (or 2000).
        assert!(
            ladder.rungs.iter().all(|r| r.point.bitrate < 6000.0),
            "high bitrate points should be excluded"
        );
    }

    #[test]
    fn test_select_bitrate_targets_deduplicates() {
        let h = hull_for(vec![point(500.0, 70.0), point(1000.0, 85.0), point(2000.0, 93.0)]);
        let opts = Opts {
            num_rungs: 4,
            abr: AbrOpts {
                // Two targets near 1000 and two near 2000
                target_bitrates: Some(vec![500.0, 600.0, 2000.0, 2100.0]),
            },
            ..Opts::default()
        };
        let ladder = select(&h, &opts);
        // Only 3 candidates available; even with 4 targets we get at most 3 rungs
        assert_eq!(ladder.rungs.len(), 3);
    }

    #[test]
    fn test_select_bitrate_targets_empty_returns_empty() {
        let h = hull_for(vec![point(500.0, 70.0)]);
        let opts = Opts {
            num_rungs: 3,
            abr: AbrOpts { target_bitrates: Some(vec![]) },
            ..Opts::default()
        };
        let ladder = select(&h, &opts);
        // Empty target list falls through to VMAF-based mode (num_rungs used)
        assert!(ladder.rungs.len() <= 3);
    }

    #[test]
    fn test_select_bitrate_targets_more_targets_than_candidates() {
        let h = hull_for(vec![point(500.0, 70.0), point(1000.0, 85.0)]);
        let opts = Opts {
            num_rungs: 6,
            abr: AbrOpts {
                target_bitrates: Some(vec![300.0, 600.0, 900.0, 1200.0, 1500.0, 2000.0]),
            },
            ..Opts::default()
        };
        let ladder = select(&h, &opts);
        assert!(ladder.rungs.len() <= 2);
    }

    // ── Logarithmic bitrates ──

    #[test]
    fn test_logarithmic_bitrates_n1() {
        let brs = logarithmic_bitrates(200.0, 8000.0, 1);
        assert_eq!(brs.len(), 1);
        assert!((brs[0] - 4100.0).abs() < 1.0);
    }

    #[test]
    fn test_logarithmic_bitrates_n6() {
        let brs = logarithmic_bitrates(200.0, 8000.0, 6);
        assert_eq!(brs.len(), 6);
        // Should be monotonically increasing
        for w in brs.windows(2) {
            assert!(w[0] < w[1], "not increasing: {} >= {}", w[0], w[1]);
        }
        // First should be near min, last near max
        assert!((brs[0] - 200.0).abs() < 10.0);
        assert!((brs[5] - 8000.0).abs() < 10.0);
    }

    #[test]
    fn test_logarithmic_bitrates_zero_n() {
        assert!(logarithmic_bitrates(200.0, 8000.0, 0).is_empty());
    }

    #[test]
    fn test_logarithmic_bitrates_min_non_positive() {
        assert!(logarithmic_bitrates(0.0, 8000.0, 6).is_empty());
        assert!(logarithmic_bitrates(-100.0, 8000.0, 6).is_empty());
    }

    #[test]
    fn test_logarithmic_bitrates_min_equals_max() {
        assert!(logarithmic_bitrates(5000.0, 5000.0, 6).is_empty());
    }

    // ── Cost calculations ──

    #[test]
    fn test_rung_bitrate_to_gb() {
        // 1000 kbps for 1 hour = 1000 * 3600 / 8 / 1024 / 1024 ≈ 0.429 GB
        let gb = rung_bitrate_to_gb(1000.0, 3600.0);
        assert!((gb - 0.429).abs() < 0.001, "got {gb}");
    }

    #[test]
    fn test_monthly_storage_cost() {
        let ladder = Ladder {
            rungs: vec![
                Rung { point: point(500.0, 70.0), index: 0 },
                Rung { point: point(2000.0, 90.0), index: 1 },
            ],
        };
        let cost = CostOpts { storage_cost_per_gb_month: 0.023, ..CostOpts::default() };
        let storage = ladder.monthly_storage_cost(cost.storage_cost_per_gb_month, 3600.0);
        // ~0.429 GB per rung * 0.023 * 2 rungs ≈ 0.0197
        assert!(storage > 0.0 && storage < 1.0, "storage cost {storage} out of range");
    }

    #[test]
    fn test_monthly_cost_empty_ladder() {
        let ladder = Ladder { rungs: vec![] };
        let cost = CostOpts { viewing_hours_per_month: 1000.0, ..CostOpts::default() };
        assert_eq!(ladder.monthly_cost(&cost, 120.0), 0.0);
    }

    #[test]
    fn test_monthly_cost_zero_viewing() {
        let ladder = Ladder { rungs: vec![Rung { point: point(5000.0, 95.0), index: 0 }] };
        // With zero viewing hours, cost should be 0 (delivery is 0).
        let cost = CostOpts { viewing_hours_per_month: 0.0, ..CostOpts::default() };
        assert_eq!(ladder.monthly_cost(&cost, 120.0), 0.0);
    }

    #[test]
    fn test_cost_prune_directly_reduces_rungs() {
        let mut ladder = Ladder {
            rungs: vec![
                Rung { point: point(200.0, 50.0), index: 0 },
                Rung { point: point(500.0, 70.0), index: 1 },
                Rung { point: point(1000.0, 85.0), index: 2 },
                Rung { point: point(2000.0, 93.0), index: 3 },
                Rung { point: point(4000.0, 97.0), index: 4 },
                Rung { point: point(8000.0, 99.0), index: 5 },
            ],
        };
        let cost = CostOpts {
            storage_cost_per_gb_month: 0.023,
            cdn_cost_per_gb: 0.08,
            viewing_hours_per_month: 10_000.0,
            max_monthly_cost: 500.0,
        };
        let initial_cost = ladder.monthly_cost(&cost, 120.0);
        assert!(initial_cost > 500.0, "expected cost > 500, got {initial_cost}");
        assert_eq!(ladder.rungs.len(), 6);

        // Prune
        let hull = hull_for(vec![]);
        cost_prune(&mut ladder, &cost, &hull);

        let pruned_cost = ladder.monthly_cost(&cost, 120.0);
        assert!(ladder.rungs.len() < 6, "expected pruning: got {} rungs", ladder.rungs.len());
        assert!(pruned_cost <= 500.0 + 10.0, "cost ${pruned_cost:.2} exceeds budget");
    }

    #[test]
    fn test_cost_prune_reduces_rungs_when_budget_is_small() {
        let hull = hull_for(vec![
            point(200.0, 50.0),
            point(500.0, 70.0),
            point(1000.0, 85.0),
            point(2000.0, 93.0),
            point(4000.0, 97.0),
            point(8000.0, 99.0),
        ]);
        // With 10k viewing hours at $0.08/GB CDN, the top rungs cost
        // significantly more than the lower ones. A $500 budget should
        // cause some (not all) rungs to be pruned.
        let opts = Opts {
            num_rungs: 6,
            cost: CostOpts {
                storage_cost_per_gb_month: 0.023,
                cdn_cost_per_gb: 0.08,
                viewing_hours_per_month: 10_000.0,
                max_monthly_cost: 500.0,
            },
            ..Opts::default()
        };
        let ladder = select(&hull, &opts);
        assert!(ladder.rungs.len() < 6, "expected cost pruning: got {} rungs", ladder.rungs.len());
        let cost = ladder.monthly_cost(&opts.cost, 120.0);
        assert!(cost <= 500.0 + 10.0, "cost ${cost:.2} exceeds budget");
    }

    #[test]
    fn test_cost_prune_no_budget_keeps_all_rungs() {
        let hull = hull_for(vec![
            point(200.0, 50.0),
            point(500.0, 70.0),
            point(1000.0, 85.0),
            point(2000.0, 93.0),
        ]);
        let opts = Opts {
            num_rungs: 4,
            cost: CostOpts {
                max_monthly_cost: 0.0, // no budget = no pruning
                viewing_hours_per_month: 100_000.0,
                ..CostOpts::default()
            },
            ..Opts::default()
        };
        let ladder = select(&hull, &opts);
        assert_eq!(ladder.rungs.len(), 4, "no pruning expected");
    }
}
