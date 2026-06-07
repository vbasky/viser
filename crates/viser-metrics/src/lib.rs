//! Metric-vs-metric correlation and divergence analysis for the `viser`
//! video-encoding-optimizer workspace.
//!
//! Where [`viser_quality`] *computes* each metric, this crate *compares the
//! metrics against each other*: how strongly PSNR, SSIM, VMAF, SSIMULACRA2 and
//! butteraugli agree on the same content — via Pearson, Spearman (SROCC) and
//! Kendall tau-b (KROCC) — and which samples they most *disagree* about
//! (divergence detection).
//!
//! A "sample" is whatever the aligned series share: per-frame scores within one
//! clip, or one aggregate score per encode across a ladder. The core functions
//! are metric-agnostic and operate on `&[f64]`; [`series_from_frames`] is a
//! convenience bridge from [`viser_quality::FrameResult`].
//!
//! ```
//! use viser_metrics::{MetricSeries, correlation_matrix};
//! let series = vec![
//!     MetricSeries::new("vmaf", vec![80.0, 85.0, 90.0], true),
//!     MetricSeries::new("psnr", vec![37.0, 38.0, 40.0], true),
//! ];
//! let m = correlation_matrix(&series);
//! assert!((m.spearman[0][1] - 1.0).abs() < 1e-9); // perfectly monotonic
//! ```

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// One metric's values across an aligned set of samples (frames or encodes).
#[derive(Debug, Clone)]
pub struct MetricSeries {
    /// Human-readable label, e.g. `"vmaf"`.
    pub label: String,
    /// One value per sample. All series compared together must be the same length.
    pub values: Vec<f64>,
    /// Whether a higher value means better quality (`false` for butteraugli).
    pub higher_is_better: bool,
}

impl MetricSeries {
    /// Construct a series.
    pub fn new(label: impl Into<String>, values: Vec<f64>, higher_is_better: bool) -> Self {
        Self { label: label.into(), values, higher_is_better }
    }
}

/// Pearson product-moment correlation of two equal-length series.
///
/// Returns `0.0` when the inputs are mismatched, empty, or constant.
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len();
    if n == 0 || n != y.len() {
        return 0.0;
    }
    let nf = n as f64;
    let mx = x.iter().sum::<f64>() / nf;
    let my = y.iter().sum::<f64>() / nf;
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..n {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx <= 0.0 || vy <= 0.0 {
        return 0.0;
    }
    cov / (vx.sqrt() * vy.sqrt())
}

/// Spearman rank correlation (SROCC) — Pearson on tie-averaged ranks.
pub fn spearman(x: &[f64], y: &[f64]) -> f64 {
    if x.len() != y.len() || x.is_empty() {
        return 0.0;
    }
    pearson(&ranks(x), &ranks(y))
}

/// Kendall tau-b rank correlation (KROCC), corrected for ties.
pub fn kendall_tau(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len();
    if n < 2 || n != y.len() {
        return 0.0;
    }
    let mut concordant = 0i64;
    let mut discordant = 0i64;
    let mut ties_x = 0i64;
    let mut ties_y = 0i64;
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = x[i] - x[j];
            let dy = y[i] - y[j];
            let tx = dx == 0.0;
            let ty = dy == 0.0;
            if tx {
                ties_x += 1;
            }
            if ty {
                ties_y += 1;
            }
            if !tx && !ty {
                if (dx > 0.0) == (dy > 0.0) {
                    concordant += 1;
                } else {
                    discordant += 1;
                }
            }
        }
    }
    let n0 = (n * (n - 1) / 2) as f64;
    let denom = ((n0 - ties_x as f64) * (n0 - ties_y as f64)).sqrt();
    if denom <= 0.0 {
        return 0.0;
    }
    (concordant - discordant) as f64 / denom
}

/// Tie-averaged 1-based ranks of `values`.
fn ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap_or(Ordering::Equal));
    let mut out = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && values[idx[j + 1]] == values[idx[i]] {
            j += 1;
        }
        // Average of the 1-based ranks i+1..=j+1.
        let avg = ((i + j) as f64) / 2.0 + 1.0;
        for k in i..=j {
            out[idx[k]] = avg;
        }
        i = j + 1;
    }
    out
}

/// Pairwise correlation matrices across a set of labelled metric series.
///
/// Each matrix is `k x k` where `k = series.len()`, indexed by series order;
/// the diagonal is `1.0` for non-constant series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationMatrix {
    /// Series labels, in row/column order.
    pub labels: Vec<String>,
    /// Pearson correlation coefficients.
    pub pearson: Vec<Vec<f64>>,
    /// Spearman (SROCC) rank correlation coefficients.
    pub spearman: Vec<Vec<f64>>,
    /// Kendall tau-b (KROCC) rank correlation coefficients.
    pub kendall: Vec<Vec<f64>>,
}

/// Compute pairwise Pearson/Spearman/Kendall correlation across all series.
pub fn correlation_matrix(series: &[MetricSeries]) -> CorrelationMatrix {
    let k = series.len();
    let labels = series.iter().map(|s| s.label.clone()).collect();
    let mut pearson_m = vec![vec![0.0; k]; k];
    let mut spearman_m = vec![vec![0.0; k]; k];
    let mut kendall_m = vec![vec![0.0; k]; k];
    for i in 0..k {
        for j in 0..k {
            pearson_m[i][j] = pearson(&series[i].values, &series[j].values);
            spearman_m[i][j] = spearman(&series[i].values, &series[j].values);
            kendall_m[i][j] = kendall_tau(&series[i].values, &series[j].values);
        }
    }
    CorrelationMatrix { labels, pearson: pearson_m, spearman: spearman_m, kendall: kendall_m }
}

/// A sample the metrics disagree about, with its per-metric normalized quality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Divergence {
    /// Index of the sample (frame or encode) within the input series.
    pub index: usize,
    /// Spread of normalized quality across metrics (`0.0` = full agreement, `1.0` = max).
    pub spread: f64,
    /// Per-metric normalized quality in `[0, 1]`, aligned with the input series order.
    pub normalized: Vec<f64>,
}

/// Rank samples by how much the metrics disagree about them, worst (largest spread) first.
///
/// Each series is min-max normalized to `[0, 1]` and inverted when
/// `higher_is_better` is false, so all metrics point "up" before comparison.
/// Samples where one metric says "great" and another says "poor" surface at the top.
/// Returns an empty vec if the series are misaligned or empty.
pub fn divergences(series: &[MetricSeries]) -> Vec<Divergence> {
    if series.len() < 2 {
        return Vec::new();
    }
    let n = series[0].values.len();
    if n == 0 || series.iter().any(|s| s.values.len() != n) {
        return Vec::new();
    }
    let normalized: Vec<Vec<f64>> = series.iter().map(minmax_normalized).collect();
    let mut out = Vec::with_capacity(n);
    for idx in 0..n {
        let vals: Vec<f64> = normalized.iter().map(|s| s[idx]).collect();
        let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        out.push(Divergence { index: idx, spread: max - min, normalized: vals });
    }
    out.sort_by(|a, b| b.spread.partial_cmp(&a.spread).unwrap_or(Ordering::Equal));
    out
}

/// Min-max normalize a series to `[0, 1]`, inverting when lower-is-better so
/// that higher always means better quality. Constant series map to `0.5`.
fn minmax_normalized(series: &MetricSeries) -> Vec<f64> {
    let min = series.values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = series.values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    series
        .values
        .iter()
        .map(|&v| {
            let q = if range > 0.0 { (v - min) / range } else { 0.5 };
            if series.higher_is_better { q } else { 1.0 - q }
        })
        .collect()
}

/// Build per-metric series from per-frame quality results.
///
/// Includes VMAF, PSNR and SSIM (always present per frame) plus MS-SSIM, VIF,
/// CAMBI, SSIMULACRA2, butteraugli and XPSNR when they carry non-constant
/// per-frame data. CAMBI and butteraugli are marked lower-is-better.
pub fn series_from_frames(frames: &[viser_quality::FrameResult]) -> Vec<MetricSeries> {
    let mut series = vec![
        MetricSeries::new("vmaf", frames.iter().map(|f| f.vmaf).collect(), true),
        MetricSeries::new("psnr", frames.iter().map(|f| f.psnr).collect(), true),
        MetricSeries::new("ssim", frames.iter().map(|f| f.ssim).collect(), true),
    ];
    // Optional metrics: included only when present (any non-zero value).
    let mut push_if_present = |label: &str, vals: Vec<f64>, higher: bool| {
        if vals.iter().any(|v| *v != 0.0) {
            series.push(MetricSeries::new(label, vals, higher));
        }
    };
    push_if_present("ms_ssim", frames.iter().map(|f| f.ms_ssim).collect(), true);
    push_if_present("vif", frames.iter().map(|f| f.vif).collect(), true);
    push_if_present("cambi", frames.iter().map(|f| f.cambi).collect(), false);
    push_if_present("xpsnr", frames.iter().map(|f| f.xpsnr).collect(), true);
    push_if_present("ssimulacra2", frames.iter().map(|f| f.ssimulacra2).collect(), true);
    push_if_present("butteraugli", frames.iter().map(|f| f.butteraugli).collect(), false);
    series
}

impl CorrelationMatrix {
    /// Render the Spearman (SROCC) matrix as a Markdown table.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("| metric |");
        for label in &self.labels {
            out.push_str(&format!(" {label} |"));
        }
        out.push_str("\n|---|");
        for _ in &self.labels {
            out.push_str("---|");
        }
        out.push('\n');
        for (i, label) in self.labels.iter().enumerate() {
            out.push_str(&format!("| {label} |"));
            for j in 0..self.labels.len() {
                out.push_str(&format!(" {:.3} |", self.spearman[i][j]));
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearson_perfect_positive_and_negative() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let up = [2.0, 4.0, 6.0, 8.0];
        let down = [8.0, 6.0, 4.0, 2.0];
        assert!((pearson(&x, &up) - 1.0).abs() < 1e-9);
        assert!((pearson(&x, &down) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn pearson_guards() {
        assert_eq!(pearson(&[], &[]), 0.0);
        assert_eq!(pearson(&[1.0, 2.0], &[1.0]), 0.0);
        assert_eq!(pearson(&[5.0, 5.0, 5.0], &[1.0, 2.0, 3.0]), 0.0); // constant
    }

    #[test]
    fn spearman_monotonic_nonlinear() {
        // monotonic but very non-linear: Spearman = 1, Pearson < 1
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [1.0, 4.0, 9.0, 16.0, 25.0];
        assert!((spearman(&x, &y) - 1.0).abs() < 1e-9);
        assert!(pearson(&x, &y) < 1.0);
    }

    #[test]
    fn spearman_handles_ties() {
        let x = [1.0, 2.0, 2.0, 3.0];
        let y = [10.0, 20.0, 20.0, 30.0];
        assert!((spearman(&x, &y) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn kendall_perfect_and_reversed() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let up = [1.0, 2.0, 3.0, 4.0];
        let down = [4.0, 3.0, 2.0, 1.0];
        assert!((kendall_tau(&x, &up) - 1.0).abs() < 1e-9);
        assert!((kendall_tau(&x, &down) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn correlation_matrix_diagonal_is_one() {
        let series = vec![
            MetricSeries::new("a", vec![1.0, 2.0, 3.0], true),
            MetricSeries::new("b", vec![3.0, 1.0, 2.0], true),
        ];
        let m = correlation_matrix(&series);
        assert!((m.pearson[0][0] - 1.0).abs() < 1e-9);
        assert!((m.spearman[1][1] - 1.0).abs() < 1e-9);
        // symmetric
        assert!((m.spearman[0][1] - m.spearman[1][0]).abs() < 1e-9);
    }

    #[test]
    fn divergence_flags_disagreement() {
        // sample 1 is great by metric A but poor by metric B
        let series = vec![
            MetricSeries::new("a", vec![0.0, 100.0, 50.0], true),
            MetricSeries::new("b", vec![0.0, 0.0, 50.0], true),
        ];
        let d = divergences(&series);
        assert_eq!(d.len(), 3);
        // the top divergence is index 1 (1.0 vs 0.0 normalized)
        assert_eq!(d[0].index, 1);
        assert!((d[0].spread - 1.0).abs() < 1e-9);
    }

    #[test]
    fn divergence_respects_polarity() {
        // butteraugli (lower better) agreeing with vmaf (higher better)
        let series = vec![
            MetricSeries::new("vmaf", vec![100.0, 50.0, 0.0], true),
            MetricSeries::new("butteraugli", vec![0.0, 1.0, 2.0], false),
        ];
        // after inverting butteraugli both rank the samples identically → near-zero spread
        let d = divergences(&series);
        assert!(d.iter().all(|x| x.spread < 1e-9));
    }

    #[test]
    fn divergence_guards() {
        assert!(divergences(&[]).is_empty());
        assert!(divergences(&[MetricSeries::new("a", vec![1.0], true)]).is_empty());
        let misaligned = vec![
            MetricSeries::new("a", vec![1.0, 2.0], true),
            MetricSeries::new("b", vec![1.0], true),
        ];
        assert!(divergences(&misaligned).is_empty());
    }

    #[test]
    fn series_from_frames_skips_empty_metrics() {
        use viser_quality::FrameResult;
        let frames = vec![
            FrameResult { frame_num: 0, vmaf: 80.0, psnr: 37.0, ssim: 0.9, ..Default::default() },
            FrameResult { frame_num: 1, vmaf: 90.0, psnr: 40.0, ssim: 0.95, ..Default::default() },
        ];
        let series = series_from_frames(&frames);
        // vmaf/psnr/ssim present; all optional metrics all-zero → skipped
        assert_eq!(series.len(), 3);
        assert_eq!(series[0].label, "vmaf");
    }

    #[test]
    fn markdown_render() {
        let series = vec![
            MetricSeries::new("vmaf", vec![1.0, 2.0, 3.0], true),
            MetricSeries::new("psnr", vec![1.0, 2.0, 3.0], true),
        ];
        let md = correlation_matrix(&series).to_markdown();
        assert!(md.contains("| vmaf |"));
        assert!(md.contains("1.000"));
    }
}
