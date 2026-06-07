//! No-reference (single-input) quality signals computed on decoded luma frames.
//!
//! Unlike the reference-based metrics, these need no pristine source — they
//! describe artefacts in the encode itself, useful for gating ingest where no
//! reference exists. The signal math here is pure Rust and deterministic; only
//! frame decode shells out to FFmpeg (a `gray8` rawvideo pipe).
//!
//! Three model-free signals are computed (no trained models, unlike
//! NIQE/BRISQUE):
//! - **sharpness** — variance of the Laplacian (higher = sharper/more detail);
//! - **blockiness** — extra gradient at 8×8 block boundaries vs. interior
//!   (lower = fewer blocking artefacts);
//! - **noise** — Immerkær's fast noise-standard-deviation estimate (lower is
//!   cleaner).

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use viser_ffmpeg::{ProbeCache, ffmpeg_path};

use crate::PooledStats;

/// Options controlling a [`measure_noref`] call.
#[derive(Debug, Clone, Default)]
pub struct NoRefOpts {
    /// Analyse every `stride`-th frame; `0` or `1` analyses every frame.
    pub stride: usize,
    /// Optional probe cache to avoid a redundant probe.
    pub probe_cache: Option<ProbeCache>,
}

/// Pooled no-reference signals for one input.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoRefResult {
    /// Mean variance-of-Laplacian (sharpness; higher is sharper).
    pub sharpness: f64,
    /// Mean 8×8 blockiness (lower is better).
    pub blockiness: f64,
    /// Mean noise standard-deviation estimate (lower is cleaner).
    pub noise: f64,
    /// Sharpness distribution across frames.
    pub sharpness_pooled: PooledStats,
    /// Blockiness distribution across frames.
    pub blockiness_pooled: PooledStats,
    /// Noise distribution across frames.
    pub noise_pooled: PooledStats,
    /// Number of frames analysed.
    pub frames: usize,
}

/// Variance of the 4-neighbour Laplacian response over the interior pixels.
///
/// A common focus/sharpness measure: blurry frames have a low-variance Laplacian.
pub fn variance_of_laplacian(frame: &[u8], w: usize, h: usize) -> f64 {
    if w < 3 || h < 3 {
        return 0.0;
    }
    let at = |x: usize, y: usize| frame[y * w + x] as f64;
    let mut sum = 0.0;
    let mut sum_sq = 0.0;
    let mut n = 0.0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let lap = 4.0 * at(x, y) - at(x - 1, y) - at(x + 1, y) - at(x, y - 1) - at(x, y + 1);
            sum += lap;
            sum_sq += lap * lap;
            n += 1.0;
        }
    }
    if n == 0.0 {
        return 0.0;
    }
    let mean = sum / n;
    (sum_sq / n) - mean * mean
}

/// 8×8 blockiness: mean absolute gradient across block boundaries minus the mean
/// absolute gradient at interior positions. `0.0` when there is no extra energy
/// at the block grid (i.e. no blocking artefacts). Clamped to be non-negative.
pub fn blockiness(frame: &[u8], w: usize, h: usize) -> f64 {
    if w < 9 || h < 9 {
        return 0.0;
    }
    let at = |x: usize, y: usize| frame[y * w + x] as f64;

    let (mut bnd, mut bnd_n, mut int, mut int_n) = (0.0, 0.0, 0.0, 0.0);
    // Horizontal gradients (column boundaries).
    for y in 0..h {
        for x in 1..w {
            let d = (at(x, y) - at(x - 1, y)).abs();
            if x % 8 == 0 {
                bnd += d;
                bnd_n += 1.0;
            } else {
                int += d;
                int_n += 1.0;
            }
        }
    }
    // Vertical gradients (row boundaries).
    for y in 1..h {
        for x in 0..w {
            let d = (at(x, y) - at(x, y - 1)).abs();
            if y % 8 == 0 {
                bnd += d;
                bnd_n += 1.0;
            } else {
                int += d;
                int_n += 1.0;
            }
        }
    }
    if bnd_n == 0.0 || int_n == 0.0 {
        return 0.0;
    }
    (bnd / bnd_n - int / int_n).max(0.0)
}

/// Immerkær's fast noise standard-deviation estimate.
///
/// Convolves with the Laplacian-of-Laplacian mask `[[1,-2,1],[-2,4,-2],[1,-2,1]]`,
/// which suppresses smooth structure and edges, leaving noise.
pub fn noise_sigma(frame: &[u8], w: usize, h: usize) -> f64 {
    if w < 3 || h < 3 {
        return 0.0;
    }
    let at = |x: usize, y: usize| frame[y * w + x] as f64;
    let mut sum_abs = 0.0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let v = at(x - 1, y - 1) - 2.0 * at(x, y - 1) + at(x + 1, y - 1) - 2.0 * at(x - 1, y)
                + 4.0 * at(x, y)
                - 2.0 * at(x + 1, y)
                + at(x - 1, y + 1)
                - 2.0 * at(x, y + 1)
                + at(x + 1, y + 1);
            sum_abs += v.abs();
        }
    }
    let count = ((w - 2) * (h - 2)) as f64;
    // sigma = sqrt(pi/2) / (6 * N) * sum|response|
    (std::f64::consts::PI / 2.0).sqrt() / (6.0 * count) * sum_abs
}

/// Resolve a video's luma dimensions, using the probe cache when present.
async fn luma_dims(input: &str, opts: &NoRefOpts) -> anyhow::Result<(usize, usize)> {
    let info = if let Some(ref cache) = opts.probe_cache {
        cache.probe(input).await?
    } else {
        viser_ffmpeg::probe(input).await?
    };
    let v = info.video_stream().ok_or_else(|| anyhow::anyhow!("no video stream in {input}"))?;
    if v.width <= 0 || v.height <= 0 {
        anyhow::bail!("invalid dimensions for {input}");
    }
    Ok((v.width as usize, v.height as usize))
}

/// Compute the no-reference signals over a clip, streaming `gray8` frames from
/// FFmpeg one at a time (bounded memory regardless of clip length).
pub async fn measure_noref(input: &str, opts: &NoRefOpts) -> anyhow::Result<NoRefResult> {
    let (w, h) = luma_dims(input, opts).await?;
    let frame_size = w * h;
    let stride = opts.stride.max(1);

    let mut child = Command::new(ffmpeg_path())
        .args(["-i", input, "-vf", "format=gray", "-f", "rawvideo", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no ffmpeg stdout"))?;

    let mut buf = vec![0u8; frame_size];
    let (mut sharp, mut block, mut noise) = (Vec::new(), Vec::new(), Vec::new());
    let mut idx = 0usize;
    loop {
        match stdout.read_exact(&mut buf).await {
            Ok(_) => {
                if idx % stride == 0 {
                    sharp.push(variance_of_laplacian(&buf, w, h));
                    block.push(blockiness(&buf, w, h));
                    noise.push(noise_sigma(&buf, w, h));
                }
                idx += 1;
            }
            // Clean end of stream (or a trailing partial frame we ignore).
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
    }
    let status = child.wait().await?;
    if !status.success() && sharp.is_empty() {
        anyhow::bail!("ffmpeg failed to decode {input}");
    }

    let sharpness_pooled = PooledStats::from_values(&sharp);
    let blockiness_pooled = PooledStats::from_values(&block);
    let noise_pooled = PooledStats::from_values(&noise);
    Ok(NoRefResult {
        sharpness: sharpness_pooled.mean,
        blockiness: blockiness_pooled.mean,
        noise: noise_pooled.mean,
        sharpness_pooled,
        blockiness_pooled,
        noise_pooled,
        frames: sharp.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat field has no sharpness, no blockiness and no noise.
    #[test]
    fn flat_frame_is_quiet() {
        let frame = vec![128u8; 16 * 16];
        assert_eq!(variance_of_laplacian(&frame, 16, 16), 0.0);
        assert_eq!(blockiness(&frame, 16, 16), 0.0);
        assert_eq!(noise_sigma(&frame, 16, 16), 0.0);
    }

    /// A jump only at every 8th column is pure block-boundary energy.
    #[test]
    fn block_grid_reads_as_blockiness() {
        let (w, h) = (16, 16);
        let mut frame = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                // Step up by 40 at each 8-pixel block.
                frame[y * w + x] = (40 * (x / 8)) as u8;
            }
        }
        // Interior columns are flat; only x==8 carries a gradient.
        assert!(blockiness(&frame, w, h) > 0.0);
    }

    /// A sharp edge raises the Laplacian variance well above a soft ramp.
    #[test]
    fn edge_is_sharper_than_ramp() {
        let (w, h) = (32, 32);
        let mut edge = vec![0u8; w * h];
        let mut ramp = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                edge[y * w + x] = if x < w / 2 { 0 } else { 255 };
                ramp[y * w + x] = ((x * 255) / (w - 1)) as u8;
            }
        }
        assert!(variance_of_laplacian(&edge, w, h) > variance_of_laplacian(&ramp, w, h));
    }

    /// A smooth ramp carries (near-)zero estimated noise; salt-and-pepper does not.
    #[test]
    fn noise_estimate_separates_clean_from_noisy() {
        let (w, h) = (32, 32);
        let mut clean = vec![0u8; w * h];
        let mut noisy = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let base = ((x * 255) / (w - 1)) as u8;
                clean[y * w + x] = base;
                // Deterministic alternating perturbation.
                noisy[y * w + x] = if (x + y) % 2 == 0 {
                    base.saturating_add(30)
                } else {
                    base.saturating_sub(30)
                };
            }
        }
        assert!(noise_sigma(&noisy, w, h) > noise_sigma(&clean, w, h));
    }
}
