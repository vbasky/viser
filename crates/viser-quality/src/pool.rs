//! Pooling strategies for reducing per-frame metric scores to summary statistics.
//!
//! Per-frame quality varies a lot within a clip, and the arithmetic mean hides
//! the worst moments that dominate perceived quality. [`PooledStats`] captures
//! the whole distribution — mean, harmonic mean, spread, and low percentiles —
//! so callers can pool with whatever strategy fits their use case (e.g. the
//! harmonic mean Netflix uses for VMAF, or a low percentile for worst-case QoE).

use serde::{Deserialize, Serialize};

/// A strategy for reducing a series of per-frame scores to a single value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStrategy {
    /// Arithmetic mean.
    Mean,
    /// Harmonic mean — penalises low outliers; the convention Netflix uses for VMAF.
    HarmonicMean,
    /// Minimum (single worst frame for higher-is-better metrics).
    Min,
    /// Maximum (single best frame for higher-is-better metrics).
    Max,
    /// 1st percentile — worst-1% pooling, the part of the clip viewers notice most.
    P1,
    /// 5th percentile.
    P5,
    /// 10th percentile.
    P10,
    /// Median (50th percentile).
    Median,
}

impl PoolStrategy {
    /// Apply this strategy to `values`, returning `0.0` for an empty slice.
    pub fn apply(self, values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        match self {
            PoolStrategy::Mean => mean(values),
            PoolStrategy::HarmonicMean => harmonic_mean(values),
            PoolStrategy::Min
            | PoolStrategy::Max
            | PoolStrategy::P1
            | PoolStrategy::P5
            | PoolStrategy::P10
            | PoolStrategy::Median => {
                let mut sorted = values.to_vec();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                match self {
                    PoolStrategy::Min => sorted[0],
                    PoolStrategy::Max => sorted[sorted.len() - 1],
                    PoolStrategy::P1 => percentile_sorted(&sorted, 1.0),
                    PoolStrategy::P5 => percentile_sorted(&sorted, 5.0),
                    PoolStrategy::P10 => percentile_sorted(&sorted, 10.0),
                    PoolStrategy::Median => percentile_sorted(&sorted, 50.0),
                    PoolStrategy::Mean | PoolStrategy::HarmonicMean => unreachable!(),
                }
            }
        }
    }
}

/// The full distribution of a per-frame metric series.
///
/// All fields are `0.0`/`0` for an empty input. Percentiles are
/// linearly interpolated; `std_dev` is the population standard deviation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PooledStats {
    /// Arithmetic mean.
    pub mean: f64,
    /// Harmonic mean over the positive values (`0.0` if none are positive).
    pub harmonic_mean: f64,
    /// Smallest value.
    pub min: f64,
    /// Largest value.
    pub max: f64,
    /// 1st percentile.
    pub p1: f64,
    /// 5th percentile.
    pub p5: f64,
    /// 10th percentile.
    pub p10: f64,
    /// Median (50th percentile).
    pub median: f64,
    /// Population standard deviation.
    pub std_dev: f64,
    /// Number of frames pooled.
    pub count: usize,
}

impl PooledStats {
    /// Compute every summary statistic from a per-frame series in one pass over a sort.
    pub fn from_values(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Self {
            mean: mean(values),
            harmonic_mean: harmonic_mean(values),
            min: sorted[0],
            max: sorted[sorted.len() - 1],
            p1: percentile_sorted(&sorted, 1.0),
            p5: percentile_sorted(&sorted, 5.0),
            p10: percentile_sorted(&sorted, 10.0),
            median: percentile_sorted(&sorted, 50.0),
            std_dev: std_dev(values),
            count: values.len(),
        }
    }

    /// Read back the value a given [`PoolStrategy`] would produce.
    pub fn get(&self, strategy: PoolStrategy) -> f64 {
        match strategy {
            PoolStrategy::Mean => self.mean,
            PoolStrategy::HarmonicMean => self.harmonic_mean,
            PoolStrategy::Min => self.min,
            PoolStrategy::Max => self.max,
            PoolStrategy::P1 => self.p1,
            PoolStrategy::P5 => self.p5,
            PoolStrategy::P10 => self.p10,
            PoolStrategy::Median => self.median,
        }
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn harmonic_mean(values: &[f64]) -> f64 {
    let positive: Vec<f64> = values.iter().copied().filter(|x| *x > 0.0).collect();
    if positive.is_empty() {
        return 0.0;
    }
    let denom: f64 = positive.iter().map(|x| 1.0 / x).sum();
    positive.len() as f64 / denom
}

fn std_dev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean(values);
    let var = values
        .iter()
        .map(|x| {
            let d = x - m;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64;
    var.sqrt()
}

/// Linearly-interpolated percentile `p` in `[0, 100]` over an already-sorted slice.
fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    match sorted.len() {
        0 => 0.0,
        1 => sorted[0],
        n => {
            let rank = (p / 100.0) * (n - 1) as f64;
            let lo = rank.floor() as usize;
            let hi = rank.ceil() as usize;
            let frac = rank - lo as f64;
            sorted[lo] + (sorted[hi] - sorted[lo]) * frac
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        let s = PooledStats::from_values(&[]);
        assert_eq!(s, PooledStats::default());
        assert_eq!(PoolStrategy::Mean.apply(&[]), 0.0);
        assert_eq!(PoolStrategy::P1.apply(&[]), 0.0);
    }

    #[test]
    fn basic_stats() {
        let v = [10.0, 20.0, 30.0, 40.0, 50.0];
        let s = PooledStats::from_values(&v);
        assert!((s.mean - 30.0).abs() < 1e-9);
        assert!((s.min - 10.0).abs() < 1e-9);
        assert!((s.max - 50.0).abs() < 1e-9);
        assert!((s.median - 30.0).abs() < 1e-9);
        assert_eq!(s.count, 5);
    }

    #[test]
    fn harmonic_mean_penalises_low_outliers() {
        let v = [100.0, 100.0, 1.0];
        let s = PooledStats::from_values(&v);
        // harmonic mean is dragged far below the arithmetic mean by the low frame
        assert!(s.harmonic_mean < s.mean);
        assert!(s.harmonic_mean < 5.0);
    }

    #[test]
    fn harmonic_mean_ignores_nonpositive() {
        // zeros/negatives are skipped rather than producing inf/NaN
        assert_eq!(harmonic_mean(&[0.0, 0.0]), 0.0);
        let s = PooledStats::from_values(&[0.0, 2.0]);
        assert!((s.harmonic_mean - 2.0).abs() < 1e-9);
    }

    #[test]
    fn percentiles_interpolate() {
        let v: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        let s = PooledStats::from_values(&v);
        // p1 over 1..=100 (linear interp on n-1) ≈ 1.99
        assert!((s.p1 - 1.99).abs() < 1e-6);
        assert!((s.p10 - 10.9).abs() < 1e-6);
        assert!((s.median - 50.5).abs() < 1e-6);
    }

    #[test]
    fn strategy_get_matches_apply() {
        let v = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        let s = PooledStats::from_values(&v);
        for strat in [
            PoolStrategy::Mean,
            PoolStrategy::HarmonicMean,
            PoolStrategy::Min,
            PoolStrategy::Max,
            PoolStrategy::P1,
            PoolStrategy::P5,
            PoolStrategy::P10,
            PoolStrategy::Median,
        ] {
            assert!((s.get(strat) - strat.apply(&v)).abs() < 1e-9, "mismatch for {strat:?}");
        }
    }

    #[test]
    fn single_value() {
        let s = PooledStats::from_values(&[42.0]);
        assert_eq!(s.mean, 42.0);
        assert_eq!(s.p1, 42.0);
        assert_eq!(s.std_dev, 0.0);
        assert_eq!(s.count, 1);
    }
}
