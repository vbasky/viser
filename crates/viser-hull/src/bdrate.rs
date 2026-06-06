use crate::Point;

/// Computes the Bjontegaard Delta Rate between two R-D curves.
///
/// Returns negative values when B is more efficient (needs less bitrate).
/// For example, -30.0 means B achieves the same quality at 30% lower bitrate.
///
/// Both curves must have at least 4 points for cubic interpolation.
pub fn bd_rate(curve_a: &[Point], curve_b: &[Point]) -> Result<f64, BdRateError> {
    if curve_a.len() < 4 || curve_b.len() < 4 {
        return Err(BdRateError("need at least 4 points per curve".into()));
    }

    let (a_rate, a_quality) = extract_rd(curve_a);
    let (b_rate, b_quality) = extract_rd(curve_b);

    // Overlapping quality range between the two curves
    let min_q = a_quality
        .iter()
        .copied()
        .reduce(f64::min)
        .unwrap()
        .max(b_quality.iter().copied().reduce(f64::min).unwrap());
    let max_q = a_quality
        .iter()
        .copied()
        .reduce(f64::max)
        .unwrap()
        .min(b_quality.iter().copied().reduce(f64::max).unwrap());

    if min_q >= max_q {
        return Err(BdRateError("no overlapping quality range between curves".into()));
    }

    let poly_a = fit_cubic(&a_quality, &a_rate);
    let poly_b = fit_cubic(&b_quality, &b_rate);

    let integral_a = integrate_cubic(&poly_a, min_q, max_q);
    let integral_b = integrate_cubic(&poly_b, min_q, max_q);

    let avg_diff = (integral_b - integral_a) / (max_q - min_q);
    let bdrate = (10.0_f64.powf(avg_diff) - 1.0) * 100.0;

    Ok(bdrate)
}

/// Error returned by `bd_rate` when curves are too short or have no overlapping quality range.
#[derive(Debug, Clone)]
pub struct BdRateError(pub String);

impl std::fmt::Display for BdRateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bdrate: {}", self.0)
    }
}

impl std::error::Error for BdRateError {}

fn extract_rd(points: &[Point]) -> (Vec<f64>, Vec<f64>) {
    let mut pairs: Vec<(f64, f64)> = points.iter().map(|p| (p.vmaf, p.bitrate.log10())).collect();
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let quality: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    let log_rate: Vec<f64> = pairs.iter().map(|p| p.1).collect();
    (log_rate, quality)
}

type Cubic = [f64; 4];

fn fit_cubic(x: &[f64], y: &[f64]) -> Cubic {
    let n = x.len();
    let mut sums = [0.0_f64; 7];
    let mut rhs = [0.0_f64; 4];

    for i in 0..n {
        let xi = x[i];
        let yi = y[i];
        let mut xp = 1.0;
        for j in 0..7 {
            sums[j] += xp;
            if j < 4 {
                rhs[j] += yi * xp;
            }
            xp *= xi;
        }
    }

    let mut mat = [[0.0_f64; 5]; 4];
    for i in 0..4 {
        mat[i][..4].copy_from_slice(&sums[i..(4 + i)]);
        mat[i][4] = rhs[i];
    }

    // Gaussian elimination with partial pivoting
    for col in 0..4 {
        let mut max_val = mat[col][col].abs();
        let mut max_row = col;
        for row in (col + 1)..4 {
            if mat[row][col].abs() > max_val {
                max_val = mat[row][col].abs();
                max_row = row;
            }
        }
        mat.swap(col, max_row);

        if mat[col][col].abs() < 1e-12 {
            return [0.0; 4];
        }

        for row in (col + 1)..4 {
            let factor = mat[row][col] / mat[col][col];
            for j in col..5 {
                mat[row][j] -= factor * mat[col][j];
            }
        }
    }

    // Back substitution
    let mut coeff = [0.0_f64; 4];
    for i in (0..4).rev() {
        coeff[i] = mat[i][4];
        for j in (i + 1)..4 {
            coeff[i] -= mat[i][j] * coeff[j];
        }
        coeff[i] /= mat[i][i];
    }

    coeff
}

fn integrate_cubic(c: &Cubic, a: f64, b: f64) -> f64 {
    let antideriv = |x: f64| -> f64 {
        c[0] * x + c[1] * x * x / 2.0 + c[2] * x * x * x / 3.0 + c[3] * x * x * x * x / 4.0
    };
    antideriv(b) - antideriv(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point;
    use viser_ffmpeg::{Codec, Resolution};

    fn pt(bitrate: f64, vmaf: f64) -> Point {
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
    fn test_bd_rate_exact_4_points_minimum() {
        let a = vec![pt(100.0, 70.0), pt(300.0, 80.0), pt(800.0, 90.0), pt(2000.0, 95.0)];
        let b = vec![pt(80.0, 70.0), pt(250.0, 80.0), pt(700.0, 90.0), pt(1800.0, 95.0)];
        let r = bd_rate(&a, &b).unwrap();
        assert!(r < 0.0, "B is more efficient, expected negative BD-rate, got {r}");
    }

    #[test]
    fn test_bd_rate_overlapping_range() {
        // Curves share [80, 90] quality range
        let a = vec![pt(200.0, 80.0), pt(500.0, 85.0), pt(1200.0, 90.0), pt(3000.0, 95.0)];
        let b = vec![pt(150.0, 80.0), pt(400.0, 85.0), pt(1000.0, 90.0), pt(2500.0, 95.0)];
        assert!(bd_rate(&a, &b).is_ok());
    }

    #[test]
    fn test_bd_rate_sorted_input() {
        // Already sorted by VMAF
        let a = vec![pt(100.0, 70.0), pt(300.0, 75.0), pt(800.0, 85.0), pt(2000.0, 90.0)];
        let b = a.clone();
        let r = bd_rate(&a, &b).unwrap();
        assert!(r.abs() < 1.0, "identical sorted curves should yield ~0");
    }

    #[test]
    fn test_bd_rate_unsorted_input() {
        // Unsorted by VMAF — should still work (extract_rd sorts)
        let a = vec![pt(2000.0, 90.0), pt(100.0, 70.0), pt(800.0, 85.0), pt(300.0, 75.0)];
        let b = vec![pt(300.0, 75.0), pt(2000.0, 90.0), pt(800.0, 85.0), pt(100.0, 70.0)];
        assert!(bd_rate(&a, &b).is_ok());
    }

    #[test]
    fn test_bd_rate_singular_matrix() {
        // All points at the same VMAF quality — polynomial fit will get a zero pivot
        let a = vec![pt(100.0, 80.0), pt(300.0, 80.0), pt(800.0, 80.0), pt(2000.0, 80.0)];
        let b = vec![pt(100.0, 80.0), pt(300.0, 80.0), pt(800.0, 80.0), pt(2000.0, 80.0)];
        // fit_cubic returns [0;4] for singular matrices, integration gives 0
        // but the overlapping range check may still pass
        let r = bd_rate(&a, &b);
        // Should either error or compute near-zero
        // (singular matrix may error on overlapping range)
        if let Ok(v) = r {
            assert!(v.abs() < 1e-6, "singular matrix should give ~0, got {v}");
        }
    }

    #[test]
    fn test_extract_rd_sorts_by_quality() {
        let points = vec![pt(2000.0, 90.0), pt(100.0, 70.0), pt(800.0, 85.0), pt(300.0, 75.0)];
        let (log_rates, qualities) = extract_rd(&points);
        assert!(
            qualities.windows(2).all(|w| w[0] <= w[1]),
            "qualities should be sorted ascending, got {qualities:?}"
        );
        assert_eq!(qualities.len(), 4);
        assert_eq!(log_rates.len(), 4);
    }

    #[test]
    fn test_fit_cubic_perfect_linear() {
        // y = 0.5 + 1.0*x (perfectly fit by cubic)
        let x = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let y: Vec<f64> = x.iter().map(|xi| 0.5 + 1.0 * xi).collect();
        let coeff = fit_cubic(&x, &y);
        assert!(coeff[0].abs() > 0.0, "constant term should be non-zero");
        assert!((coeff[1] - 1.0).abs() < 0.01, "linear term should be ~1.0, got {}", coeff[1]);
    }

    #[test]
    fn test_integrate_cubic_linear() {
        // c[0]x integrated from 0 to 2 = c[0]*2
        let c = [1.0_f64, 0.0, 0.0, 0.0];
        let result = integrate_cubic(&c, 0.0, 2.0);
        assert!((result - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_integrate_cubic_quadratic() {
        // c[1]*x^2/2 integrated from 0 to 2 = 3*4/2 = 6
        let c = [0.0_f64, 3.0, 0.0, 0.0];
        let result = integrate_cubic(&c, 0.0, 2.0);
        assert!((result - 6.0).abs() < 1e-9);
    }

    #[test]
    fn test_bd_rate_error_display() {
        let err = BdRateError("test error".into());
        assert_eq!(format!("{err}"), "bdrate: test error");
    }
}
