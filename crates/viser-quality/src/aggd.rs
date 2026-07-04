//! Asymmetric Generalized Gaussian Distribution (AGGD) fitting helpers
//! shared by NIQE and BRISQUE feature extraction.

/// Lanczos approximation of Γ(x) for x > 0.
pub(crate) fn gamma(x: f64) -> f64 {
    if x <= 0.0 || !x.is_finite() {
        return f64::NAN;
    }
    const G: f64 = 7.0;
    const COEFF: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.5203681218851,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    if x < 0.5 {
        return std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma(1.0 - x));
    }
    let x = x - 1.0;
    let mut a = COEFF[0];
    let mut t = x + G;
    for &c in &COEFF[1..] {
        a += c / t;
        t += 1.0;
    }
    let tmp = x + G + 0.5;
    (2.0 * std::f64::consts::PI).sqrt() * tmp.powf(x + 0.5) * (-tmp).exp() * a
}

/// 7×7 Gaussian kernel with σ = 7/6 (OpenCV BRISQUE/NIQE default).
pub(crate) fn gaussian_kernel_7x7() -> [[f32; 7]; 7] {
    let sigma = 7.0_f32 / 6.0;
    let mut k = [[0.0_f32; 7]; 7];
    let mut sum = 0.0_f32;
    for y in 0..7 {
        for x in 0..7 {
            let dx = x as f32 - 3.0;
            let dy = y as f32 - 3.0;
            let v = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp();
            k[y][x] = v;
            sum += v;
        }
    }
    for row in &mut k {
        for v in row {
            *v /= sum;
        }
    }
    k
}

/// Separable Gaussian blur with edge replication (OpenCV BORDER_REPLICATE).
pub(crate) fn gaussian_blur_replicate(
    src: &[f64],
    w: usize,
    h: usize,
    kernel: &[[f32; 7]; 7],
) -> Vec<f64> {
    let mut tmp = vec![0.0; w * h];
    let mut out = vec![0.0; w * h];
    let r = 3usize;

    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for kx in 0..7 {
                let sx = x.saturating_add(kx).saturating_sub(r).min(w - 1);
                sum += src[y * w + sx] as f32 * kernel[3][kx];
            }
            tmp[y * w + x] = sum as f64;
        }
    }

    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for ky in 0..7 {
                let sy = y.saturating_add(ky).saturating_sub(r).min(h - 1);
                sum += tmp[sy * w + x] as f32 * kernel[ky][3];
            }
            out[y * w + x] = sum as f64;
        }
    }
    out
}

/// AGGD shape/scale parameters from a coefficient vector (OpenCV `AGGDfit`).
pub(crate) fn estimate_aggd(coeffs: &[f64]) -> (f64, f64, f64) {
    let mut pos_sq = 0.0;
    let mut neg_sq = 0.0;
    let mut abs_sum = 0.0;
    let mut pos_count = 0usize;
    let mut neg_count = 0usize;

    for &v in coeffs {
        if v > 0.0 {
            pos_count += 1;
            pos_sq += v * v;
            abs_sum += v;
        } else if v < 0.0 {
            neg_count += 1;
            neg_sq += v * v;
            abs_sum -= v;
        }
    }

    let left_sigma = if neg_count > 0 { (neg_sq / neg_count as f64).sqrt() } else { 0.0 };
    let right_sigma = if pos_count > 0 { (pos_sq / pos_count as f64).sqrt() } else { 0.0 };
    let total = coeffs.len() as f64;
    let gamma_hat = if right_sigma > 0.0 { left_sigma / right_sigma } else { 1.0 };
    let r_hat =
        if total > 0.0 { (abs_sum / total).powi(2) / ((neg_sq + pos_sq) / total) } else { 0.0 };
    let r_hat_norm =
        r_hat * (gamma_hat.powi(3) + 1.0) * (gamma_hat + 1.0) / (gamma_hat.powi(2) + 1.0).powi(2);

    let mut prev_gamma = 0.2;
    let mut prev_diff = 1e10;
    let mut alpha = 0.2;
    let mut gam = 0.2;
    while gam < 10.0 {
        let r_gam = gamma(2.0 / gam).powi(2) / (gamma(1.0 / gam) * gamma(3.0 / gam));
        let diff = (r_gam - r_hat_norm).abs();
        if diff > prev_diff {
            break;
        }
        prev_diff = diff;
        prev_gamma = gam;
        alpha = gam;
        gam += 0.001;
    }
    let _ = prev_gamma;
    (alpha, left_sigma, right_sigma)
}

/// NIQE/MATLAB AGGD parameter estimation (`estimateaggdparam`).
pub(crate) fn estimate_aggd_niqe(coeffs: &[f64]) -> (f64, f64, f64) {
    let (alpha, left_sigma, right_sigma) = estimate_aggd(coeffs);
    let betal = left_sigma * (gamma(1.0 / alpha) / gamma(3.0 / alpha)).sqrt();
    let betar = right_sigma * (gamma(1.0 / alpha) / gamma(3.0 / alpha)).sqrt();
    (alpha, betal, betar)
}
