//! NIQE no-reference quality assessment with the utlive `modelparameters.mat`
//! multivariate Gaussian pristine-scene model.

use serde::Deserialize;

use crate::aggd::{estimate_aggd_niqe, gamma, gaussian_blur_replicate, gaussian_kernel_7x7};

#[derive(Debug, Deserialize)]
struct NiqeModel {
    mu: Vec<f64>,
    cov: Vec<Vec<f64>>,
}

static MODEL_JSON: &str = include_str!("../models/niqe_model.json");

fn load_model() -> NiqeModel {
    serde_json::from_str(MODEL_JSON).expect("embedded NIQE model must parse")
}

const BLOCK_SIZE: usize = 96;
const FEAT_PER_BLOCK: usize = 18;

/// Scores a grayscale frame. Lower is better (naturalness distance).
pub fn score_frame(gray: &[u8], w: usize, h: usize) -> f64 {
    if w < BLOCK_SIZE || h < BLOCK_SIZE {
        return f64::NAN;
    }
    let model = load_model();
    let mut im: Vec<f64> = gray.iter().map(|&v| f64::from(v)).collect();
    let mut cw = w;
    let mut ch = h;
    cw -= cw % BLOCK_SIZE;
    ch -= ch % BLOCK_SIZE;
    im.truncate(ch * cw);

    let kernel = gaussian_kernel_7x7();
    let mut all_features: Vec<Vec<f64>> = Vec::new();

    for scale in 0..2 {
        let block = BLOCK_SIZE >> scale;
        if block == 0 || cw < block || ch < block {
            break;
        }
        if scale > 0 {
            im = downsample_half(&im, cw, ch);
            cw /= 2;
            ch /= 2;
        }

        let mu = gaussian_blur_replicate(&im, cw, ch, &kernel);
        let im_sq: Vec<f64> = im.iter().map(|v| v * v).collect();
        let blurred_sq = gaussian_blur_replicate(&im_sq, cw, ch, &kernel);
        let mut sigma: Vec<f64> = blurred_sq
            .iter()
            .zip(mu.iter())
            .map(|(&e2, &m)| (e2 - m * m).max(0.0).sqrt())
            .collect();
        for s in &mut sigma {
            *s += 1.0;
        }

        let mscn: Vec<f64> =
            im.iter().zip(mu.iter()).zip(sigma.iter()).map(|((&p, &m), &s)| (p - m) / s).collect();

        let rows = ch / block;
        let cols = cw / block;
        for by in 0..rows {
            for bx in 0..cols {
                let mut patch = Vec::with_capacity(block * block);
                for y in 0..block {
                    for x in 0..block {
                        let px = bx * block + x;
                        let py = by * block + y;
                        patch.push(mscn[py * cw + px]);
                    }
                }
                all_features.push(compute_block_features(&patch));
            }
        }
    }

    if all_features.is_empty() {
        return f64::NAN;
    }

    let dim = all_features[0].len();
    let n = all_features.len() as f64;
    let mut mu_dist = vec![0.0; dim];
    for feat in &all_features {
        for (m, &v) in mu_dist.iter_mut().zip(feat.iter()) {
            *m += v;
        }
    }
    for m in &mut mu_dist {
        *m /= n;
    }

    let mut cov_dist = vec![vec![0.0; dim]; dim];
    for feat in &all_features {
        for i in 0..dim {
            for j in 0..dim {
                cov_dist[i][j] += (feat[i] - mu_dist[i]) * (feat[j] - mu_dist[j]);
            }
        }
    }
    for row in &mut cov_dist {
        for v in row {
            *v /= n;
        }
    }

    let mu_pris = &model.mu;
    let cov_pris = &model.cov;
    let mut cov_mix = vec![vec![0.0; dim]; dim];
    for i in 0..dim {
        for j in 0..dim {
            cov_mix[i][j] = (cov_pris[i][j] + cov_dist[i][j]) / 2.0;
        }
    }

    let inv = pseudo_inverse(&cov_mix);
    let mut diff = vec![0.0; dim];
    for i in 0..dim {
        diff[i] = mu_pris[i] - mu_dist[i];
    }
    let mut quality = 0.0;
    for i in 0..dim {
        for j in 0..dim {
            quality += diff[i] * inv[i][j] * diff[j];
        }
    }
    quality.sqrt()
}

fn compute_block_features(patch: &[f64]) -> Vec<f64> {
    let (alpha, betal, betar) = estimate_aggd_niqe(patch);
    let mut feat = vec![alpha, (betal + betar) / 2.0];
    let side = (patch.len() as f64).sqrt() as usize;
    let shifts = [(0, 1), (1, 0), (1, 1), (1, -1)];
    for (dx, dy) in shifts {
        let mut pair = Vec::with_capacity(patch.len());
        for y in 0..side {
            for x in 0..side {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                let a = patch[y * side + x];
                let b = if nx >= 0 && nx < side as i32 && ny >= 0 && ny < side as i32 {
                    patch[ny as usize * side + nx as usize]
                } else {
                    0.0
                };
                pair.push(a * b);
            }
        }
        let (alpha, betal, betar) = estimate_aggd_niqe(&pair);
        let mean_param = (betar - betal) * (gamma(2.0 / alpha) / gamma(1.0 / alpha));
        feat.extend([alpha, mean_param, betal, betar]);
    }
    while feat.len() < FEAT_PER_BLOCK {
        feat.push(0.0);
    }
    feat.truncate(FEAT_PER_BLOCK);
    feat
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

fn pseudo_inverse(m: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = m.len();
    let mut aug = vec![vec![0.0; 2 * n]; n];
    for i in 0..n {
        for j in 0..n {
            aug[i][j] = m[i][j];
        }
        aug[i][n + i] = 1.0;
    }
    for col in 0..n {
        let mut pivot = col;
        for row in col + 1..n {
            if aug[row][col].abs() > aug[pivot][col].abs() {
                pivot = row;
            }
        }
        if aug[pivot][col].abs() < 1e-12 {
            continue;
        }
        aug.swap(pivot, col);
        let div = aug[col][col];
        for j in 0..2 * n {
            aug[col][j] /= div;
        }
        for row in 0..n {
            if row == col {
                continue;
            }
            let factor = aug[row][col];
            for j in 0..2 * n {
                aug[row][j] -= factor * aug[col][j];
            }
        }
    }
    let mut inv = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            inv[i][j] = aug[i][n + j];
        }
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niqe_model_loads() {
        let m = load_model();
        assert_eq!(m.mu.len(), 36);
        assert_eq!(m.cov.len(), 36);
    }

    #[test]
    fn niqe_scores_finite_for_test_pattern() {
        let w = 192;
        let h = 192;
        let gray: Vec<u8> = (0..w * h).map(|i| ((i * 37) % 256) as u8).collect();
        let score = score_frame(&gray, w, h);
        assert!(score.is_finite());
        assert!(score > 0.0);
    }
}
