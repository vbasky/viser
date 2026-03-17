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
    let min_q = a_quality.iter().copied().reduce(f64::min).unwrap()
        .max(b_quality.iter().copied().reduce(f64::min).unwrap());
    let max_q = a_quality.iter().copied().reduce(f64::max).unwrap()
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

#[derive(Debug, Clone)]
pub struct BdRateError(pub String);

impl std::fmt::Display for BdRateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bdrate: {}", self.0)
    }
}

impl std::error::Error for BdRateError {}

fn extract_rd(points: &[Point]) -> (Vec<f64>, Vec<f64>) {
    let mut pairs: Vec<(f64, f64)> = points.iter()
        .map(|p| (p.vmaf, p.bitrate.log10()))
        .collect();
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
        for j in 0..4 {
            mat[i][j] = sums[i + j];
        }
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
