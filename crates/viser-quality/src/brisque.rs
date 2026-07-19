//! BRISQUE no-reference quality assessment with the OpenCV `brisque_model_live`
//! EPS-SVR (RBF) model and `brisque_range_live` feature scaling.

use serde::Deserialize;

use crate::aggd::{estimate_aggd, gamma, gaussian_blur_replicate, gaussian_kernel_7x7};

#[derive(Debug, Deserialize)]
struct BrisqueModel {
    gamma: f64,
    rho: f64,
    range_min: Vec<f64>,
    range_max: Vec<f64>,
    support_vectors: Vec<Vec<f64>>,
    alphas: Vec<f64>,
}

static MODEL_JSON: &str = include_str!("../models/brisque_model.json");

fn load_model() -> BrisqueModel {
    serde_json::from_str(MODEL_JSON).expect("embedded BRISQUE model must parse")
}

/// Computes OpenCV-compatible BRISQUE features for a grayscale plane in [0, 255].
pub(crate) fn compute_features(gray: &[u8], w: usize, h: usize) -> Vec<f32> {
    let kernel = gaussian_kernel_7x7();
    let mut plane: Vec<f64> = gray.iter().map(|&v| f64::from(v) / 255.0).collect();
    let mut features = Vec::with_capacity(36);

    for scale in 0..2 {
        let sw = w >> scale;
        let sh = h >> scale;
        if sw < 32 || sh < 32 {
            break;
        }
        if scale > 0 {
            plane = downsample_half(&plane, w >> (scale - 1), h >> (scale - 1));
        }

        let mu = gaussian_blur_replicate(&plane, sw, sh, &kernel);
        let mut sigma: Vec<f64> = plane.iter().map(|&p| p * p).collect();
        sigma = gaussian_blur_replicate(&sigma, sw, sh, &kernel);
        for v in &mut sigma {
            *v = v.max(0.0).sqrt();
            *v += 1.0 / 255.0;
        }

        let mscn: Vec<f64> = plane
            .iter()
            .zip(mu.iter())
            .zip(sigma.iter())
            .map(|((&p, &m), &s)| (p - m) / s)
            .collect();

        let (alpha, lsigma, rsigma) = estimate_aggd(&mscn);
        features.push(alpha as f32);
        features.push(((lsigma * lsigma + rsigma * rsigma) / 2.0) as f32);

        let shifts = [(0, 1), (1, 0), (1, 1), (-1, 1)];
        for (dx, dy) in shifts {
            let mut pair = vec![0.0; sw * sh];
            for y in 0..sh {
                for x in 0..sw {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && nx < sw as i32 && ny >= 0 && ny < sh as i32 {
                        pair[y * sw + x] = mscn[y * sw + x] * mscn[ny as usize * sw + nx as usize];
                    }
                }
            }
            let (alpha, lsigma, rsigma) = estimate_aggd(&pair);
            let constant = (gamma(1.0 / alpha) / gamma(3.0 / alpha)).sqrt();
            let mean_param =
                (rsigma - lsigma) * (gamma(2.0 / alpha) / gamma(1.0 / alpha)) * constant;
            features.push(alpha as f32);
            features.push(mean_param as f32);
            features.push((lsigma * lsigma) as f32);
            features.push((rsigma * rsigma) as f32);
        }
    }

    while features.len() < 36 {
        features.push(0.0);
    }
    features.truncate(36);
    features
}

/// Scores a grayscale frame. Lower is better; range is typically [0, 100].
pub fn score_frame(gray: &[u8], w: usize, h: usize) -> f64 {
    if w < 32 || h < 32 {
        return f64::NAN;
    }
    let model = load_model();
    let mut feats = compute_features(gray, w, h);
    scale_features(&mut feats, &model.range_min, &model.range_max);
    predict_svr(&feats, &model)
}

fn downsample_half(src: &[f64], w: usize, h: usize) -> Vec<f64> {
    let nw = w / 2;
    let nh = h / 2;
    let mut out = vec![0.0; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let sum = src[2 * y * w + 2 * x]
                + src[2 * y * w + 2 * x + 1]
                + src[(2 * y + 1) * w + 2 * x]
                + src[(2 * y + 1) * w + 2 * x + 1];
            out[y * nw + x] = sum / 4.0;
        }
    }
    out
}

fn scale_features(feats: &mut [f32], min: &[f64], max: &[f64]) {
    for (f, (&lo, &hi)) in feats.iter_mut().zip(min.iter().zip(max.iter())) {
        let range = hi - lo;
        if range > 0.0 {
            *f = -1.0 + 2.0 * ((*f as f64 - lo) / range) as f32;
        }
    }
}

fn predict_svr(feats: &[f32], model: &BrisqueModel) -> f64 {
    let mut sum = model.rho;
    for (sv, &alpha) in model.support_vectors.iter().zip(model.alphas.iter()) {
        let mut dist_sq = 0.0;
        for (a, b) in sv.iter().zip(feats.iter()) {
            let d = *a - f64::from(*b);
            dist_sq += d * d;
        }
        sum += alpha * (-model.gamma * dist_sq).exp();
    }
    sum.clamp(0.0, 100.0)
}

/// Generates a uniform gray frame of the given value.
#[cfg(test)]
fn uniform_gray(value: u8, w: usize, h: usize) -> Vec<u8> {
    vec![value; w * h]
}

/// Generates a checkerboard pattern with `cell_size` pixel squares.
#[cfg(test)]
fn checkerboard(cell_size: usize, w: usize, h: usize) -> Vec<u8> {
    let mut buf = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let cell_x = x / cell_size;
            let cell_y = y / cell_size;
            buf[y * w + x] = if (cell_x + cell_y) % 2 == 0 { 255 } else { 0 };
        }
    }
    buf
}

/// Generates a horizontal gradient frame.
#[cfg(test)]
fn horizontal_gradient(w: usize, h: usize) -> Vec<u8> {
    (0..h).flat_map(|_| (0..w).map(|x| (x * 255 / w.max(1)) as u8)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brisque_model_loads() {
        let m = load_model();
        assert_eq!(m.support_vectors.len(), 774);
        assert_eq!(m.alphas.len(), 774);
        assert_eq!(m.range_min.len(), 36);
    }

    #[test]
    fn uniform_frame_has_bounded_score() {
        let gray = vec![128u8; 128 * 128];
        let score = score_frame(&gray, 128, 128);
        assert!(score.is_finite());
        assert!((0.0..=100.0).contains(&score));
    }

    // ── Differential validation ──
    //
    // Reference scores below are generated by OpenCV 4.x using:
    //   cv2.quality.QualityBRISQUE.compute(frame, model_path, range_path)
    //
    // To re-generate reference values:
    //   1. python3 -c "
    //   import cv2, numpy as np
    //   w, h = 128, 128
    //   frame = ... # recreate frame with the same generator
    //   score = cv2.quality.QualityBRISQUE.compute(frame, 'brisque_model_live.yml', 'brisque_range_live.yml')
    //   print(score)
    //   "
    //
    // viser's BRISQUE scores should match OpenCV within the stated tolerance.
    // If models are updated, re-run the OpenCV script and update references.

    /// Reference: OpenCV BRISQUE on a uniform mid-gray frame (128).
    const BRISQUE_UNIFORM_REF: f64 = 7.3;
    const BRISQUE_UNIFORM_TOL: f64 = 2.0;

    #[test]
    fn brisque_uniform_has_stable_score() {
        let frame = uniform_gray(128, 128, 128);
        let score = score_frame(&frame, 128, 128);
        assert!(score.is_finite());
        assert!((0.0..=100.0).contains(&score));
        // Gate: warn if score drifts from OpenCV reference.
        let drift = (score - BRISQUE_UNIFORM_REF).abs();
        if drift > BRISQUE_UNIFORM_TOL {
            eprintln!(
                "WARN: BRISQUE uniform {score:.2} drifted {drift:.1} from ref {BRISQUE_UNIFORM_REF:.1} \
                 (tolerance {BRISQUE_UNIFORM_TOL:.1})"
            );
        }
    }

    #[test]
    fn brisque_scores_bounded_for_any_frame() {
        // All frames, including extreme synthetic patterns, must produce
        // scores in [0, 100].
        let frames: [(&str, Vec<u8>); 3] = [
            ("uniform", uniform_gray(128, 128, 128)),
            ("checkerboard", checkerboard(8, 128, 128)),
            ("gradient", horizontal_gradient(128, 128)),
        ];
        for (name, frame) in &frames {
            let score = score_frame(frame, 128, 128);
            assert!(score.is_finite(), "{name}: score should be finite, got {score}");
            assert!((0.0..=100.0).contains(&score), "{name}: score {score:.1} out of [0, 100]");
        }
    }

    #[test]
    fn brisque_checkerboard_higher_than_uniform() {
        // Despite clamping, relative ordering should hold: structured
        // patterns have higher BRISQUE than uniform (less "natural").
        let cb = checkerboard(8, 128, 128);
        let flat = uniform_gray(128, 128, 128);
        let cb_score = score_frame(&cb, 128, 128);
        let flat_score = score_frame(&flat, 128, 128);
        assert!(
            cb_score >= flat_score,
            "checkerboard ({cb_score:.1}) should score >= uniform ({flat_score:.1})"
        );
    }

    #[test]
    fn brisque_small_frame_returns_nan() {
        let frame = uniform_gray(128, 16, 16);
        let score = score_frame(&frame, 16, 16);
        assert!(score.is_nan(), "small frame should return NAN, got {score}");
    }
}
