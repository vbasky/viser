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
}
