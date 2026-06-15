//! Video quality measurement for the `viser` video-encoding-optimizer workspace.
//!
//! Computes VMAF, PSNR, SSIM, SSIMULACRA2, and butteraugli scores between a
//! reference and a distorted video. VMAF/PSNR/SSIM use FFmpeg's libvmaf filter,
//! while SSIMULACRA2 and butteraugli shell out to their CLI tools on extracted
//! PNG frames. See `measure` for the entry point.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::warn;
use viser_ffmpeg::{ProbeCache, ffmpeg_path};

pub mod noref;
pub mod pool;
pub use noref::{NoRefOpts, NoRefResult, measure_noref};
pub use pool::{PoolStrategy, PooledStats};

/// Quality metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    /// Netflix VMAF perceptual score (0-100, higher is better).
    Vmaf,
    /// Peak signal-to-noise ratio in dB (higher is better).
    Psnr,
    /// Structural similarity index (0-1, higher is better).
    Ssim,
    /// SSIMULACRA2 perceptual score (higher is better), via the `ssimulacra2` CLI.
    Ssimulacra2,
    /// Butteraugli perceptual distance (lower is better), via the `butteraugli` CLI.
    Butteraugli,
    /// Multi-scale SSIM (0-1, higher is better), via libvmaf's `float_ms_ssim`.
    MsSsim,
    /// Visual information fidelity (higher is better), the mean of libvmaf's VIF scales.
    Vif,
    /// CAMBI banding score (lower is better), via libvmaf's `cambi` feature.
    Cambi,
    /// Perceptually-weighted PSNR in dB (higher is better), via FFmpeg's `xpsnr` filter.
    Xpsnr,
}

/// Aggregate (pooled) quality scores, with optional per-frame breakdown.
///
/// Each score is `0.0` when its metric was not requested or is unavailable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Result {
    /// Mean VMAF score.
    pub vmaf: f64,
    /// Mean luma (Y) PSNR (dB).
    pub psnr: f64,
    /// Mean Cb/U-plane PSNR (dB); `0.0` when per-component PSNR is unavailable.
    pub psnr_u: f64,
    /// Mean Cr/V-plane PSNR (dB); `0.0` when per-component PSNR is unavailable.
    pub psnr_v: f64,
    /// Weighted PSNR `(6·Y + U + V) / 8` (dB); falls back to luma when chroma is absent.
    pub psnr_avg: f64,
    /// Mean SSIM.
    pub ssim: f64,
    /// SSIMULACRA2 score (mean over sampled frames).
    pub ssimulacra2: f64,
    /// Butteraugli distance (mean over sampled frames).
    pub butteraugli: f64,
    /// Mean multi-scale SSIM; `0.0` when not requested.
    pub ms_ssim: f64,
    /// Mean VIF (visual information fidelity); computed alongside VMAF.
    pub vif: f64,
    /// Mean CAMBI banding score (lower is better); `0.0` when not requested.
    pub cambi: f64,
    /// Mean weighted XPSNR `(6·Y + U + V) / 8` (dB); `0.0` when not requested.
    pub xpsnr: f64,
    /// Distribution statistics (mean, harmonic mean, percentiles, …) per metric.
    pub pooled: Pooled,
    /// Per-frame scores; populated only when `MeasureOpts::per_frame` is set.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<FrameResult>,
}

/// Pooled distribution statistics for each metric, computed from per-frame scores.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pooled {
    /// VMAF distribution.
    pub vmaf: PooledStats,
    /// Luma (Y) PSNR distribution.
    pub psnr: PooledStats,
    /// SSIM distribution.
    pub ssim: PooledStats,
    /// SSIMULACRA2 distribution (populated when more than one frame is sampled).
    pub ssimulacra2: PooledStats,
    /// Butteraugli distribution (populated when more than one frame is sampled).
    pub butteraugli: PooledStats,
    /// Multi-scale SSIM distribution.
    pub ms_ssim: PooledStats,
    /// VIF distribution.
    pub vif: PooledStats,
    /// CAMBI banding distribution (lower is better).
    pub cambi: PooledStats,
    /// Weighted XPSNR distribution (dB).
    pub xpsnr: PooledStats,
}

/// Quality scores for a single frame.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrameResult {
    /// Frame index within the video.
    pub frame_num: i32,
    /// VMAF score for this frame.
    pub vmaf: f64,
    /// Luma (Y) PSNR (dB) for this frame.
    pub psnr: f64,
    /// Cb/U-plane PSNR (dB) for this frame.
    #[serde(default)]
    pub psnr_u: f64,
    /// Cr/V-plane PSNR (dB) for this frame.
    #[serde(default)]
    pub psnr_v: f64,
    /// SSIM for this frame.
    pub ssim: f64,
    /// SSIMULACRA2 score for this frame.
    pub ssimulacra2: f64,
    /// Butteraugli distance for this frame.
    pub butteraugli: f64,
    /// Multi-scale SSIM for this frame.
    #[serde(default)]
    pub ms_ssim: f64,
    /// VIF for this frame.
    #[serde(default)]
    pub vif: f64,
    /// CAMBI banding score for this frame (lower is better).
    #[serde(default)]
    pub cambi: f64,
    /// Weighted XPSNR (dB) for this frame.
    #[serde(default)]
    pub xpsnr: f64,
}

/// Options controlling a `measure` call.
#[derive(Debug, Clone)]
pub struct MeasureOpts {
    /// Metrics to compute; an empty list defaults to VMAF, PSNR, and SSIM.
    pub metrics: Vec<Metric>,
    /// Subsample factor for libvmaf (every Nth frame); `0` means no subsampling.
    pub subsample: i32,
    /// VMAF model version name (e.g. `"vmaf_v0.6.1"`).
    pub model: String,
    /// When `true`, also collect per-frame scores into `Result::frames`.
    pub per_frame: bool,
    /// How many frames to measure for SSIMULACRA2/butteraugli. `0` (the default)
    /// measures the whole clip; `1` a single frame (frame 0, fastest); higher
    /// values measure that many evenly-spaced frames. Results pool into
    /// `Result::pooled`.
    pub frame_samples: usize,
    /// Optional probe cache reused across measurements to avoid redundant probes.
    pub probe_cache: Option<ProbeCache>,
}

impl Default for MeasureOpts {
    fn default() -> Self {
        Self {
            metrics: vec![
                Metric::Vmaf,
                Metric::Psnr,
                Metric::Ssim,
                Metric::Ssimulacra2,
                Metric::Butteraugli,
            ],
            subsample: 0,
            model: "vmaf_v0.6.1".into(),
            per_frame: false,
            frame_samples: 0,
            probe_cache: None,
        }
    }
}

/// Computes quality metrics between a reference and distorted video.
pub async fn measure(
    reference: &str,
    distorted: &str,
    opts: MeasureOpts,
) -> anyhow::Result<Result> {
    let model_name = if opts.model.is_empty() { "vmaf_v0.6.1" } else { &opts.model };
    let metrics = if opts.metrics.is_empty() {
        vec![Metric::Vmaf, Metric::Psnr, Metric::Ssim]
    } else {
        opts.metrics.clone()
    };

    let tmp = tempfile::Builder::new().prefix("viser-vmaf-").suffix(".json").tempfile()?;
    let log_path = tmp.path().to_string_lossy().to_string();

    // Build libvmaf filter string
    let mut vmaf_opts = format!("log_fmt=json:log_path={log_path}:model=version={model_name}");

    // libvmaf accepts the `feature` option only once; repeating `:feature=...`
    // makes later entries silently override earlier ones (dropping metrics).
    // Collect all requested features into a single `|`-separated option.
    let mut features: Vec<&str> = Vec::new();
    for m in &metrics {
        match m {
            Metric::Psnr => features.push("name=psnr"),
            Metric::Ssim => features.push("name=float_ssim"),
            Metric::MsSsim => features.push("name=float_ms_ssim"),
            Metric::Cambi => features.push("name=cambi"),
            // VIF rides along with VMAF (vif_scale features are always emitted).
            Metric::Vmaf | Metric::Vif => {}
            // Measured outside libvmaf.
            Metric::Xpsnr | Metric::Ssimulacra2 | Metric::Butteraugli => {}
        }
    }
    if !features.is_empty() {
        vmaf_opts.push_str(&format!(":feature={}", features.join("|")));
    }

    if opts.subsample > 0 {
        vmaf_opts.push_str(&format!(":n_subsample={}", opts.subsample));
    }

    // Probe reference to get resolution for scaling
    let ref_info = if let Some(ref cache) = opts.probe_cache {
        cache.probe(reference).await?
    } else {
        viser_ffmpeg::probe(reference).await?
    };

    let ref_video =
        ref_info.video_stream().ok_or_else(|| anyhow::anyhow!("no video stream in reference"))?;

    if ref_video.bits_per_raw_sample > 8 {
        warn!(
            bits_per_sample = ref_video.bits_per_raw_sample,
            reference = reference,
            "10-bit content detected; VMAF scores calibrated for 8-bit may differ"
        );
    }

    let filtergraph = format!(
        "[0:v]scale={}:{}:flags=bicubic[dist];[dist][1:v]libvmaf={}",
        ref_video.width, ref_video.height, vmaf_opts
    );

    let args = ["-i", distorted, "-i", reference, "-lavfi", &filtergraph, "-f", "null", "-"];

    let output = Command::new(ffmpeg_path())
        .args(args)
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg quality measurement failed: {stderr}");
    }

    let data = std::fs::read(&log_path)?;
    let mut result = parse_vmaf_log(&data, opts.per_frame)?;

    // SSIMULACRA2: run CLI on extracted PNG frames (one frame, or full-clip sample).
    if metrics.contains(&Metric::Ssimulacra2) {
        let scores = measure_ssimulacra2(reference, distorted, &opts).await?;
        result.ssimulacra2 = pool::PoolStrategy::Mean.apply(&scores);
        result.pooled.ssimulacra2 = PooledStats::from_values(&scores);
    }

    // Butteraugli: run CLI on extracted PNG frames (one frame, or full-clip sample).
    if metrics.contains(&Metric::Butteraugli) {
        let scores = measure_butteraugli(reference, distorted, &opts).await?;
        result.butteraugli = pool::PoolStrategy::Mean.apply(&scores);
        result.pooled.butteraugli = PooledStats::from_values(&scores);
    }

    // XPSNR: a separate FFmpeg pass with the `xpsnr` filter (full clip).
    if metrics.contains(&Metric::Xpsnr) {
        let scores = measure_xpsnr(reference, distorted, &opts).await?;
        result.xpsnr = pool::PoolStrategy::Mean.apply(&scores);
        result.pooled.xpsnr = PooledStats::from_values(&scores);
        if opts.per_frame && scores.len() == result.frames.len() {
            for (fr, s) in result.frames.iter_mut().zip(scores) {
                fr.xpsnr = s;
            }
        }
    }

    Ok(result)
}

// libvmaf JSON output structures
#[derive(Deserialize)]
struct VmafLog {
    frames: Vec<VmafFrame>,
    #[serde(default)]
    pooled_metrics: std::collections::HashMap<String, PooledMetric>,
}

#[derive(Deserialize)]
struct VmafFrame {
    #[serde(rename = "frameNum")]
    frame_num: i32,
    metrics: std::collections::HashMap<String, f64>,
}

#[derive(Deserialize)]
struct PooledMetric {
    mean: f64,
}

fn parse_vmaf_log(data: &[u8], per_frame: bool) -> anyhow::Result<Result> {
    let log: VmafLog = serde_json::from_slice(data)?;

    let mut result = Result::default();

    // Scalar (pooled-mean) values, with naming fallbacks across libvmaf versions.
    result.vmaf = pooled_mean(&log, &["vmaf"]);
    result.psnr = pooled_mean(&log, &["psnr_y", "psnr"]);
    result.psnr_u = pooled_mean(&log, &["psnr_cb", "psnr_u"]);
    result.psnr_v = pooled_mean(&log, &["psnr_cr", "psnr_v"]);
    result.psnr_avg = if result.psnr_u > 0.0 && result.psnr_v > 0.0 {
        // Standard 4:2:0 luma-weighted PSNR. Requires both chroma planes;
        // with only one present the (6Y+U+V)/8 weighting would divide by a
        // spurious zero term and under-report, so fall back to luma.
        (6.0 * result.psnr + result.psnr_u + result.psnr_v) / 8.0
    } else {
        result.psnr
    };
    result.ssim = pooled_mean(&log, &["float_ssim", "ssim"]);

    // Per-frame series for distribution pooling (computed regardless of `per_frame`).
    let mut vmaf_series = Vec::with_capacity(log.frames.len());
    let mut psnr_series = Vec::with_capacity(log.frames.len());
    let mut ssim_series = Vec::with_capacity(log.frames.len());
    let mut ms_ssim_series = Vec::with_capacity(log.frames.len());
    let mut vif_series = Vec::with_capacity(log.frames.len());
    let mut cambi_series = Vec::with_capacity(log.frames.len());
    for f in &log.frames {
        if let Some(v) = f.metrics.get("vmaf") {
            vmaf_series.push(*v);
        }
        if let Some(v) = frame_metric(&f.metrics, &["psnr_y", "psnr"]) {
            psnr_series.push(v);
        }
        if let Some(v) = frame_metric(&f.metrics, &["float_ssim", "ssim"]) {
            ssim_series.push(v);
        }
        if let Some(v) = frame_metric(&f.metrics, &["float_ms_ssim", "ms_ssim"]) {
            ms_ssim_series.push(v);
        }
        if let Some(v) = vif_mean(&f.metrics) {
            vif_series.push(v);
        }
        if let Some(v) = f.metrics.get("cambi") {
            cambi_series.push(*v);
        }
    }
    result.pooled.vmaf = PooledStats::from_values(&vmaf_series);
    result.pooled.psnr = PooledStats::from_values(&psnr_series);
    result.pooled.ssim = PooledStats::from_values(&ssim_series);
    result.pooled.ms_ssim = PooledStats::from_values(&ms_ssim_series);
    result.pooled.vif = PooledStats::from_values(&vif_series);
    result.pooled.cambi = PooledStats::from_values(&cambi_series);
    result.ms_ssim = result.pooled.ms_ssim.mean;
    result.vif = result.pooled.vif.mean;
    result.cambi = result.pooled.cambi.mean;

    // When libvmaf omits pooled_metrics but emits per-frame data, fall back to the mean.
    if result.vmaf == 0.0 {
        result.vmaf = result.pooled.vmaf.mean;
    }
    if result.psnr == 0.0 {
        result.psnr = result.pooled.psnr.mean;
        if result.psnr_avg == 0.0 {
            result.psnr_avg = result.psnr;
        }
    }
    if result.ssim == 0.0 {
        result.ssim = result.pooled.ssim.mean;
    }

    if per_frame {
        for f in &log.frames {
            result.frames.push(FrameResult {
                frame_num: f.frame_num,
                vmaf: f.metrics.get("vmaf").copied().unwrap_or(0.0),
                psnr: frame_metric(&f.metrics, &["psnr_y", "psnr"]).unwrap_or(0.0),
                psnr_u: frame_metric(&f.metrics, &["psnr_cb", "psnr_u"]).unwrap_or(0.0),
                psnr_v: frame_metric(&f.metrics, &["psnr_cr", "psnr_v"]).unwrap_or(0.0),
                ssim: frame_metric(&f.metrics, &["float_ssim", "ssim"]).unwrap_or(0.0),
                ssimulacra2: f.metrics.get("ssimulacra2").copied().unwrap_or(0.0),
                butteraugli: f.metrics.get("butteraugli").copied().unwrap_or(0.0),
                ms_ssim: frame_metric(&f.metrics, &["float_ms_ssim", "ms_ssim"]).unwrap_or(0.0),
                vif: vif_mean(&f.metrics).unwrap_or(0.0),
                cambi: f.metrics.get("cambi").copied().unwrap_or(0.0),
                xpsnr: 0.0,
            });
        }
    }

    Ok(result)
}

/// First matching pooled-metric mean across naming variants, or `0.0`.
fn pooled_mean(log: &VmafLog, keys: &[&str]) -> f64 {
    for k in keys {
        if let Some(m) = log.pooled_metrics.get(*k) {
            return m.mean;
        }
    }
    0.0
}

/// First matching per-frame metric value across naming variants.
fn frame_metric(metrics: &std::collections::HashMap<String, f64>, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(v) = metrics.get(*k) {
            return Some(*v);
        }
    }
    None
}

/// Mean of libvmaf's four VIF scales (`*_vif_scale0..3`), across naming variants.
/// Returns `None` when no VIF scale is present.
fn vif_mean(metrics: &std::collections::HashMap<String, f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0;
    for s in 0..4 {
        if let Some(v) = frame_metric(
            metrics,
            &[
                &format!("integer_vif_scale{s}"),
                &format!("float_vif_scale{s}"),
                &format!("vif_scale{s}"),
            ],
        ) {
            sum += v;
            n += 1;
        }
    }
    if n > 0 { Some(sum / n as f64) } else { None }
}

/// Evenly-spaced frame indices for a given sample count: a single frame (`0`)
/// for `samples <= 1`, otherwise `samples` indices across the clip. Full-clip
/// measurement (`frame_samples == 0`) is handled by the caller, which skips
/// this and extracts every frame in one pass.
fn sample_indices(nb_frames: i32, samples: usize) -> Vec<i32> {
    if samples <= 1 || nb_frames <= 1 {
        return vec![0];
    }
    let count = samples.min(nb_frames as usize);
    if count <= 1 {
        return vec![0];
    }
    (0..count)
        .map(|i| ((i as f64) * (nb_frames as f64 - 1.0) / (count as f64 - 1.0)).round() as i32)
        .collect()
}

/// Resolve the reference video stream's dimensions and frame count.
async fn reference_dims(reference: &str, opts: &MeasureOpts) -> anyhow::Result<(i32, i32, i32)> {
    let ref_info = if let Some(ref cache) = opts.probe_cache {
        cache.probe(reference).await?
    } else {
        viser_ffmpeg::probe(reference).await?
    };
    let ref_video =
        ref_info.video_stream().ok_or_else(|| anyhow::anyhow!("no video stream in reference"))?;
    Ok((ref_video.width, ref_video.height, ref_video.nb_frames))
}

/// Extract frames from `input` as PNGs into `dir` in a single decode pass.
///
/// `selection == None` extracts every frame (full clip); otherwise just the
/// given indices. Frames are written as zero-padded sequential PNGs and returned
/// in extraction (ascending-index) order. One pass per video avoids the
/// quadratic cost of re-decoding from the start for each frame.
async fn extract_frames_png(
    input: &str,
    selection: Option<&[i32]>,
    width: i32,
    height: i32,
    dir: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let scale = format!("scale={width}:{height}:flags=bicubic");
    let vf = match selection {
        None => scale,
        Some(indices) => {
            let sel = indices.iter().map(|i| format!("eq(n\\,{i})")).collect::<Vec<_>>().join("+");
            format!("select='{sel}',{scale}")
        }
    };
    let pattern = dir.join("%06d.png");
    let output = Command::new(ffmpeg_path())
        .args(["-i", input, "-vf", &vf, "-fps_mode", "passthrough", "-c:v", "png"])
        .arg(&pattern)
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to extract frames from {input}: {stderr}");
    }

    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Aligned reference/distorted PNG frame pairs for the perceptual metrics, kept
/// alive by their temp dirs. `frame_samples == 0` measures the whole clip;
/// otherwise the evenly-spaced [`sample_indices`].
struct FramePairs {
    _ref_dir: tempfile::TempDir,
    _dist_dir: tempfile::TempDir,
    pairs: Vec<(PathBuf, PathBuf)>,
}

async fn extract_frame_pairs(
    reference: &str,
    distorted: &str,
    opts: &MeasureOpts,
) -> anyhow::Result<FramePairs> {
    let (width, height, nb_frames) = reference_dims(reference, opts).await?;
    let (_, _, dist_nb_frames) = reference_dims(distorted, opts).await?;
    if dist_nb_frames != nb_frames {
        warn!(
            reference_frames = nb_frames,
            distorted_frames = dist_nb_frames,
            "reference and distorted frame counts differ; sampled perceptual metrics may be misaligned"
        );
    }

    let selection: Option<Vec<i32>> = if opts.frame_samples == 0 {
        None
    } else {
        Some(sample_indices(nb_frames, opts.frame_samples))
    };
    let sel = selection.as_deref();

    let ref_dir = tempfile::Builder::new().prefix("viser-q-ref-").tempdir()?;
    let dist_dir = tempfile::Builder::new().prefix("viser-q-dist-").tempdir()?;
    let ref_paths = extract_frames_png(reference, sel, width, height, ref_dir.path()).await?;
    let dist_paths = extract_frames_png(distorted, sel, width, height, dist_dir.path()).await?;

    let n = ref_paths.len().min(dist_paths.len());
    let pairs =
        ref_paths.into_iter().take(n).zip(dist_paths.into_iter().take(n)).collect::<Vec<_>>();
    Ok(FramePairs { _ref_dir: ref_dir, _dist_dir: dist_dir, pairs })
}

/// Run the `ssimulacra2` CLI over the measured frames; one score per frame.
async fn measure_ssimulacra2(
    reference: &str,
    distorted: &str,
    opts: &MeasureOpts,
) -> anyhow::Result<Vec<f64>> {
    let frames = extract_frame_pairs(reference, distorted, opts).await?;
    let mut scores = Vec::with_capacity(frames.pairs.len());
    for (ref_png, dist_png) in &frames.pairs {
        let s2_output = Command::new("ssimulacra2")
            .arg(ref_png)
            .arg(dist_png)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await?;

        if !s2_output.status.success() {
            anyhow::bail!("ssimulacra2 failed: {}", String::from_utf8_lossy(&s2_output.stderr));
        }

        let stdout_str = String::from_utf8_lossy(&s2_output.stdout);
        let score: f64 = stdout_str
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("ssimulacra2: could not parse score: {stdout_str}"))?;
        scores.push(score);
    }

    Ok(scores)
}

/// Run the `butteraugli` CLI over the measured frames; one score per frame.
///
/// Butteraugli may be absent or silent on success; missing or unparseable output
/// yields a `0.0` sentinel for that frame rather than failing the measurement.
async fn measure_butteraugli(
    reference: &str,
    distorted: &str,
    opts: &MeasureOpts,
) -> anyhow::Result<Vec<f64>> {
    let frames = extract_frame_pairs(reference, distorted, opts).await?;
    let mut scores = Vec::with_capacity(frames.pairs.len());
    for (i, (ref_png, dist_png)) in frames.pairs.iter().enumerate() {
        let ba_output = Command::new("butteraugli")
            .arg(ref_png)
            .arg(dist_png)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await;

        let mut score = 0.0;
        let mut parsed = false;
        if let Ok(out) = ba_output
            && out.status.success()
        {
            let stdout_str = String::from_utf8_lossy(&out.stdout);
            if let Ok(s) = stdout_str.trim().parse::<f64>() {
                score = s;
                parsed = true;
            } else if let Some(last_line) = stdout_str.lines().last() {
                // butteraugli may emit extra lines; the score is usually the last.
                if let Ok(s) = last_line.trim().parse::<f64>() {
                    score = s;
                    parsed = true;
                }
            }
        }
        if !parsed {
            warn!(frame = i, "butteraugli not available or failed; recording 0.0");
        }
        scores.push(score);
    }

    Ok(scores)
}

/// Parse the number after `tag` (e.g. `"y:"`) on an xpsnr stats line, mapping
/// non-finite values (identical frames report `inf`) to a `100.0` dB cap.
fn parse_xpsnr_component(line: &str, tag: &str) -> Option<f64> {
    let idx = line.find(tag)?;
    let token = line[idx + tag.len()..].split_whitespace().next()?;
    match token {
        "inf" | "-inf" => Some(100.0),
        t => t.parse::<f64>().ok().map(|x| if x.is_finite() { x } else { 100.0 }),
    }
}

/// Run FFmpeg's `xpsnr` filter over the whole clip; one weighted XPSNR
/// `(6·Y + U + V) / 8` (dB) per frame, parsed from the per-frame stats file.
async fn measure_xpsnr(
    reference: &str,
    distorted: &str,
    opts: &MeasureOpts,
) -> anyhow::Result<Vec<f64>> {
    let (width, height, _nb) = reference_dims(reference, opts).await?;
    let stats = tempfile::Builder::new().prefix("viser-xpsnr-").suffix(".log").tempfile()?;
    let stats_path = stats.path().to_string_lossy().to_string();

    // Match the libvmaf path: scale the distorted input to reference dimensions.
    let filtergraph = format!(
        "[0:v]scale={width}:{height}:flags=bicubic[dist];[dist][1:v]xpsnr=stats_file={stats_path}"
    );
    let output = Command::new(ffmpeg_path())
        .args(["-i", distorted, "-i", reference, "-lavfi", &filtergraph, "-f", "null", "-"])
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("xpsnr measurement failed: {stderr}");
    }

    let log = std::fs::read_to_string(stats.path())?;
    let mut scores = Vec::new();
    for line in log.lines() {
        // e.g. "n:    1  XPSNR y: 46.9714  XPSNR u: 45.1188  XPSNR v: 45.0873"
        if let Some(y) = parse_xpsnr_component(line, "y:") {
            let u = parse_xpsnr_component(line, "u:").unwrap_or(y);
            let v = parse_xpsnr_component(line, "v:").unwrap_or(y);
            scores.push((6.0 * y + u + v) / 8.0);
        }
    }
    Ok(scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_serde_roundtrip() {
        for m in
            &[Metric::Vmaf, Metric::Psnr, Metric::Ssim, Metric::Ssimulacra2, Metric::Butteraugli]
        {
            let json = serde_json::to_string(m).unwrap();
            let back: Metric = serde_json::from_str(&json).unwrap();
            assert_eq!(*m, back);
        }
    }

    #[test]
    fn test_metric_serde_names() {
        assert_eq!(serde_json::to_string(&Metric::Vmaf).unwrap(), "\"vmaf\"");
        assert_eq!(serde_json::to_string(&Metric::Psnr).unwrap(), "\"psnr\"");
        assert_eq!(serde_json::to_string(&Metric::Ssim).unwrap(), "\"ssim\"");
        assert_eq!(serde_json::to_string(&Metric::Ssimulacra2).unwrap(), "\"ssimulacra2\"");
        assert_eq!(serde_json::to_string(&Metric::Butteraugli).unwrap(), "\"butteraugli\"");
    }

    #[test]
    fn test_metric_eq() {
        assert_eq!(Metric::Vmaf, Metric::Vmaf);
        assert_ne!(Metric::Vmaf, Metric::Psnr);
        assert_eq!(Metric::Ssimulacra2, Metric::Ssimulacra2);
        assert_ne!(Metric::Ssimulacra2, Metric::Butteraugli);
    }

    #[test]
    fn test_result_default() {
        let r = Result::default();
        assert!((r.vmaf - 0.0).abs() < 1e-9);
        assert!((r.psnr - 0.0).abs() < 1e-9);
        assert!((r.ssim - 0.0).abs() < 1e-9);
        assert!((r.ssimulacra2 - 0.0).abs() < 1e-9);
        assert!((r.butteraugli - 0.0).abs() < 1e-9);
        assert!(r.frames.is_empty());
    }

    #[test]
    fn test_parse_vmaf_log_basic() {
        let json = br#"{
            "frames": [
                {"frameNum": 0, "metrics": {"vmaf": 85.0, "psnr_y": 38.5, "float_ssim": 0.95}}
            ],
            "pooled_metrics": {
                "vmaf": {"mean": 86.5},
                "psnr_y": {"mean": 39.2},
                "float_ssim": {"mean": 0.96}
            }
        }"#;
        let result = parse_vmaf_log(json, false).unwrap();
        assert!((result.vmaf - 86.5).abs() < 1e-9);
        assert!((result.psnr - 39.2).abs() < 1e-9);
        assert!((result.ssim - 0.96).abs() < 1e-9);
        assert!(result.frames.is_empty());
    }

    #[test]
    fn test_parse_vmaf_log_per_frame() {
        let json = br#"{
            "frames": [
                {"frameNum": 0, "metrics": {"vmaf": 80.0, "psnr_y": 37.0, "float_ssim": 0.93}},
                {"frameNum": 1, "metrics": {"vmaf": 90.0, "psnr_y": 40.0, "float_ssim": 0.97}}
            ],
            "pooled_metrics": {
                "vmaf": {"mean": 85.0},
                "psnr_y": {"mean": 38.5},
                "float_ssim": {"mean": 0.95}
            }
        }"#;
        let result = parse_vmaf_log(json, true).unwrap();
        assert_eq!(result.frames.len(), 2);
        assert_eq!(result.frames[0].frame_num, 0);
        assert!((result.frames[0].vmaf - 80.0).abs() < 1e-9);
        assert_eq!(result.frames[1].frame_num, 1);
        assert!((result.frames[1].vmaf - 90.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_vmaf_log_fallback_psnr() {
        let json = br#"{
            "frames": [],
            "pooled_metrics": {
                "vmaf": {"mean": 85.0},
                "psnr": {"mean": 39.0},
                "ssim": {"mean": 0.94}
            }
        }"#;
        let result = parse_vmaf_log(json, false).unwrap();
        assert!((result.psnr - 39.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_vmaf_log_missing_metrics() {
        let json = br#"{
            "frames": [],
            "pooled_metrics": {}
        }"#;
        let result = parse_vmaf_log(json, false).unwrap();
        assert!((result.vmaf - 0.0).abs() < 1e-9);
        assert!((result.psnr - 0.0).abs() < 1e-9);
        assert!((result.ssim - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_vmaf_log_invalid_json() {
        assert!(parse_vmaf_log(b"not json", false).is_err());
    }

    #[test]
    fn test_result_serde_roundtrip() {
        let r = Result {
            vmaf: 85.0,
            psnr: 38.5,
            ssim: 0.95,
            ssimulacra2: 70.0,
            butteraugli: 0.5,
            ..Default::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: Result = serde_json::from_str(&json).unwrap();
        assert!((back.vmaf - 85.0).abs() < 1e-9);
        assert!((back.ssimulacra2 - 70.0).abs() < 1e-9);
        assert!((back.butteraugli - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_parse_vmaf_log_per_component_psnr() {
        let json = br#"{
            "frames": [],
            "pooled_metrics": {
                "vmaf": {"mean": 85.0},
                "psnr_y": {"mean": 40.0},
                "psnr_cb": {"mean": 44.0},
                "psnr_cr": {"mean": 46.0},
                "float_ssim": {"mean": 0.95}
            }
        }"#;
        let result = parse_vmaf_log(json, false).unwrap();
        assert!((result.psnr - 40.0).abs() < 1e-9, "luma");
        assert!((result.psnr_u - 44.0).abs() < 1e-9, "Cb");
        assert!((result.psnr_v - 46.0).abs() < 1e-9, "Cr");
        // weighted (6*40 + 44 + 46) / 8 = 41.25
        assert!((result.psnr_avg - 41.25).abs() < 1e-9, "weighted avg");
    }

    #[test]
    fn test_parse_vmaf_log_psnr_avg_falls_back_to_luma() {
        let json = br#"{
            "frames": [],
            "pooled_metrics": {"psnr_y": {"mean": 39.0}}
        }"#;
        let result = parse_vmaf_log(json, false).unwrap();
        assert!((result.psnr_avg - 39.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_vmaf_log_pooled_distribution() {
        let json = br#"{
            "frames": [
                {"frameNum": 0, "metrics": {"vmaf": 80.0, "psnr_y": 37.0, "float_ssim": 0.93}},
                {"frameNum": 1, "metrics": {"vmaf": 90.0, "psnr_y": 41.0, "float_ssim": 0.97}}
            ],
            "pooled_metrics": {"vmaf": {"mean": 85.0}}
        }"#;
        let result = parse_vmaf_log(json, false).unwrap();
        assert_eq!(result.pooled.vmaf.count, 2);
        assert!((result.pooled.vmaf.min - 80.0).abs() < 1e-9);
        assert!((result.pooled.vmaf.max - 90.0).abs() < 1e-9);
        assert!((result.pooled.vmaf.mean - 85.0).abs() < 1e-9);
        // psnr/ssim distributions are pooled even without a pooled_metrics entry
        assert!((result.pooled.psnr.min - 37.0).abs() < 1e-9);
        assert!((result.psnr - 39.0).abs() < 1e-9, "psnr falls back to frame mean");
    }

    #[test]
    fn test_sample_indices() {
        assert_eq!(sample_indices(100, 0), vec![0]);
        assert_eq!(sample_indices(100, 1), vec![0]);
        assert_eq!(sample_indices(0, 5), vec![0]);
        assert_eq!(sample_indices(1, 5), vec![0]);
        assert_eq!(sample_indices(101, 3), vec![0, 50, 100]);
        // never asks for more frames than exist
        assert_eq!(sample_indices(2, 10), vec![0, 1]);
    }

    #[test]
    fn test_result_serde_omits_zero_frames() {
        let r = Result::default();
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("frames"));
    }

    #[test]
    fn test_measure_opts_default() {
        let opts = MeasureOpts::default();
        assert_eq!(opts.metrics.len(), 5);
        assert_eq!(opts.subsample, 0);
        assert_eq!(opts.model, "vmaf_v0.6.1");
        assert!(!opts.per_frame);
        assert_eq!(opts.frame_samples, 0);
        assert!(opts.probe_cache.is_none());
    }

    #[test]
    fn test_vif_mean() {
        let mut m = std::collections::HashMap::new();
        m.insert("integer_vif_scale0".to_string(), 0.2);
        m.insert("integer_vif_scale1".to_string(), 0.4);
        m.insert("integer_vif_scale2".to_string(), 0.6);
        m.insert("integer_vif_scale3".to_string(), 0.8);
        assert!((vif_mean(&m).unwrap() - 0.5).abs() < 1e-9);

        // Naming-variant fallback and partial presence.
        let mut m2 = std::collections::HashMap::new();
        m2.insert("vif_scale0".to_string(), 1.0);
        m2.insert("float_vif_scale1".to_string(), 0.0);
        assert!((vif_mean(&m2).unwrap() - 0.5).abs() < 1e-9);

        assert!(vif_mean(&std::collections::HashMap::new()).is_none());
    }

    #[test]
    fn test_parse_xpsnr_component() {
        let line = "n:    1  XPSNR y: 46.9714  XPSNR u: 45.1188  XPSNR v: 45.0873";
        assert!((parse_xpsnr_component(line, "y:").unwrap() - 46.9714).abs() < 1e-9);
        assert!((parse_xpsnr_component(line, "u:").unwrap() - 45.1188).abs() < 1e-9);
        assert!((parse_xpsnr_component(line, "v:").unwrap() - 45.0873).abs() < 1e-9);
        // Identical frames report inf → clamped to the 100 dB cap.
        assert_eq!(parse_xpsnr_component("XPSNR y: inf", "y:"), Some(100.0));
        assert_eq!(parse_xpsnr_component("nothing here", "y:"), None);
    }

    #[test]
    fn test_parse_vmaf_log_extended_metrics() {
        let json = br#"{
            "frames": [
                {"frameNum": 0, "metrics": {"vmaf": 80.0, "float_ms_ssim": 0.90, "cambi": 2.0,
                    "integer_vif_scale0": 0.2, "integer_vif_scale1": 0.4,
                    "integer_vif_scale2": 0.6, "integer_vif_scale3": 0.8}},
                {"frameNum": 1, "metrics": {"vmaf": 90.0, "float_ms_ssim": 1.00, "cambi": 0.0,
                    "integer_vif_scale0": 0.4, "integer_vif_scale1": 0.6,
                    "integer_vif_scale2": 0.8, "integer_vif_scale3": 1.0}}
            ],
            "pooled_metrics": {"vmaf": {"mean": 85.0}}
        }"#;
        let result = parse_vmaf_log(json, true).unwrap();
        // MS-SSIM mean of 0.90 and 1.00.
        assert!((result.ms_ssim - 0.95).abs() < 1e-9);
        // CAMBI mean of 2.0 and 0.0.
        assert!((result.cambi - 1.0).abs() < 1e-9);
        // VIF: frame means 0.5 and 0.7 → overall 0.6.
        assert!((result.vif - 0.6).abs() < 1e-9);
        // Per-frame propagation.
        assert!((result.frames[0].ms_ssim - 0.90).abs() < 1e-9);
        assert!((result.frames[0].vif - 0.5).abs() < 1e-9);
        assert!((result.frames[1].cambi - 0.0).abs() < 1e-9);
    }
}
