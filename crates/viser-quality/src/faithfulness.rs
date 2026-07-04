//! Faithfulness / hallucination signals (v0).
//!
//! Detects *invented* detail in a distorted video relative to a reference — the
//! gap conventional full-reference metrics miss when an enhancer adds confident
//! texture that was never in the source. v0 uses three frame-level heuristics:
//!
//! - **HF gain** — excess Laplacian variance in the distorted frame vs. reference;
//! - **texture paradox** — sharper distorted frames with lower PSNR than expected;
//! - **blockiness inversion** — blocking artefacts removed but sharpness rises
//!   (classic over-sharpen / denoise-then-invent pipeline signature).

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use viser_ffmpeg::{ProbeCache, ffmpeg_path};

use crate::noref::{blockiness, variance_of_laplacian};
use crate::{MeasureOpts, Metric, PooledStats, measure};

/// Options controlling a [`measure_faithfulness`] call.
#[derive(Debug, Clone, Default)]
pub struct FaithfulnessOpts {
    /// Analyse every `stride`-th frame; `0` or `1` analyses every frame.
    pub stride: usize,
    /// Optional probe cache to avoid redundant probes.
    pub probe_cache: Option<ProbeCache>,
    /// When `true`, also run a quick PSNR/VMAF pass for the quality-paradox signal.
    pub check_quality_paradox: bool,
}

/// Faithfulness score and component signals for one reference/distorted pair.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaithfulnessResult {
    /// Overall faithfulness score (0–100; higher = more faithful, less invented detail).
    pub score: f64,
    /// Mean excess high-frequency energy ratio `(vol_dist / vol_ref - 1)+`.
    pub hf_gain: f64,
    /// Mean Laplacian-variance ratio `vol_dist / vol_ref`.
    pub sharpness_ratio: f64,
    /// Fraction of frames where distorted is >20% sharper with blockiness drop >0.5.
    pub texture_paradox_rate: f64,
    /// Quality paradox: VMAF gain without matching PSNR gain (0–1 scale).
    pub quality_paradox: f64,
    /// Distribution of per-frame HF-gain values.
    pub hf_gain_pooled: PooledStats,
    /// Number of aligned frame pairs analysed.
    pub frames: usize,
    /// Human-readable interpretation.
    pub summary: String,
}

/// Measures faithfulness of `distorted` relative to `reference`.
pub async fn measure_faithfulness(
    reference: &str,
    distorted: &str,
    opts: &FaithfulnessOpts,
) -> anyhow::Result<FaithfulnessResult> {
    let (w, h) = luma_dims(reference, opts).await?;
    let stride = opts.stride.max(1);

    let ref_frames = decode_gray_frames(reference, w, h, stride).await?;
    let dist_frames = decode_gray_frames(distorted, w, h, stride).await?;
    let n = ref_frames.len().min(dist_frames.len());
    if n == 0 {
        anyhow::bail!("no aligned frames decoded from reference/distorted pair");
    }

    let mut hf_gains = Vec::with_capacity(n);
    let mut sharp_ratios = Vec::with_capacity(n);
    let mut paradox_frames = 0usize;

    for i in 0..n {
        let vol_ref = variance_of_laplacian(&ref_frames[i], w, h);
        let vol_dist = variance_of_laplacian(&dist_frames[i], w, h);
        let blk_ref = blockiness(&ref_frames[i], w, h);
        let blk_dist = blockiness(&dist_frames[i], w, h);

        let ratio = if vol_ref > 1.0 { vol_dist / vol_ref } else { 1.0 };
        sharp_ratios.push(ratio);
        hf_gains.push((ratio - 1.0).max(0.0));

        if ratio > 1.2 && blk_dist + 0.5 < blk_ref {
            paradox_frames += 1;
        }
    }

    let hf_gain_pooled = PooledStats::from_values(&hf_gains);
    let hf_gain = hf_gain_pooled.mean;
    let sharpness_ratio = sharp_ratios.iter().sum::<f64>() / n as f64;
    let texture_paradox_rate = paradox_frames as f64 / n as f64;

    let mut quality_paradox = 0.0;
    if opts.check_quality_paradox {
        let q = measure(
            reference,
            distorted,
            MeasureOpts {
                metrics: vec![Metric::Vmaf, Metric::Psnr],
                subsample: 5,
                probe_cache: opts.probe_cache.clone(),
                ..Default::default()
            },
        )
        .await?;
        // Paradox: perceptual score looks good but luma PSNR is mediocre while HF rose.
        if q.vmaf > 85.0 && q.psnr < 38.0 && hf_gain > 0.15 {
            quality_paradox = ((q.vmaf - 85.0) / 15.0).clamp(0.0, 1.0);
        }
    }

    let penalty = (hf_gain * 40.0 + texture_paradox_rate * 25.0 + quality_paradox * 20.0).min(95.0);
    let score = (100.0 - penalty).max(0.0);

    let summary = if score >= 85.0 {
        "faithful — no significant invented-detail signals".into()
    } else if score >= 65.0 {
        format!("mild HF gain ({hf_gain:.2}) — possible sharpening or mild enhancement")
    } else if score >= 40.0 {
        format!(
            "suspect — HF gain {hf_gain:.2}, texture-paradox {:.0}% of frames",
            texture_paradox_rate * 100.0
        )
    } else {
        format!(
            "likely hallucinated detail — HF gain {hf_gain:.2}, sharpness ratio {sharpness_ratio:.2}"
        )
    };

    Ok(FaithfulnessResult {
        score,
        hf_gain,
        sharpness_ratio,
        texture_paradox_rate,
        quality_paradox,
        hf_gain_pooled,
        frames: n,
        summary,
    })
}

async fn luma_dims(input: &str, opts: &FaithfulnessOpts) -> anyhow::Result<(usize, usize)> {
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

async fn decode_gray_frames(
    input: &str,
    w: usize,
    h: usize,
    stride: usize,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let frame_size = w * h;
    let vf = if stride > 1 {
        format!("format=gray,select=not(mod(n\\,{stride}))")
    } else {
        "format=gray".into()
    };

    let mut child = Command::new(ffmpeg_path())
        .args(["-i", input, "-vf", &vf, "-f", "rawvideo", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no ffmpeg stdout"))?;

    let mut frames = Vec::new();
    let mut buf = vec![0u8; frame_size];
    loop {
        match stdout.read_exact(&mut buf).await {
            Ok(_) => frames.push(buf.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
    }
    let status = child.wait().await?;
    if !status.success() && frames.is_empty() {
        anyhow::bail!("ffmpeg failed to decode {input}");
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_frames_have_unit_sharpness_ratio() {
        let (w, h) = (32, 32);
        let mut frame = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                frame[y * w + x] = if x < w / 2 { 0 } else { 255 };
            }
        }
        let vol = variance_of_laplacian(&frame, w, h);
        assert!(vol > 0.0);
        assert!((vol / vol - 1.0).abs() < 1e-9);
    }
}
