use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::warn;
use viser_ffmpeg::{ProbeCache, ffmpeg_path};

/// Quality metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    Vmaf,
    Psnr,
    Ssim,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Result {
    pub vmaf: f64,
    pub psnr: f64,
    pub ssim: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<FrameResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameResult {
    pub frame_num: i32,
    pub vmaf: f64,
    pub psnr: f64,
    pub ssim: f64,
}

#[derive(Debug, Clone)]
pub struct MeasureOpts {
    pub metrics: Vec<Metric>,
    pub subsample: i32,
    pub model: String,
    pub per_frame: bool,
    pub probe_cache: Option<ProbeCache>,
}

impl Default for MeasureOpts {
    fn default() -> Self {
        Self {
            metrics: vec![Metric::Vmaf, Metric::Psnr, Metric::Ssim],
            subsample: 0,
            model: "vmaf_v0.6.1".into(),
            per_frame: false,
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

    for m in &metrics {
        match m {
            Metric::Psnr => vmaf_opts.push_str(":feature=name=psnr"),
            Metric::Ssim => vmaf_opts.push_str(":feature=name=float_ssim"),
            Metric::Vmaf => {}
        }
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
    parse_vmaf_log(&data, opts.per_frame)
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

    if let Some(m) = log.pooled_metrics.get("vmaf") {
        result.vmaf = m.mean;
    }
    if let Some(m) = log.pooled_metrics.get("psnr_y") {
        result.psnr = m.mean;
    } else if let Some(m) = log.pooled_metrics.get("psnr") {
        result.psnr = m.mean;
    }
    if let Some(m) = log.pooled_metrics.get("float_ssim") {
        result.ssim = m.mean;
    } else if let Some(m) = log.pooled_metrics.get("ssim") {
        result.ssim = m.mean;
    }

    if per_frame {
        for f in &log.frames {
            result.frames.push(FrameResult {
                frame_num: f.frame_num,
                vmaf: f.metrics.get("vmaf").copied().unwrap_or(0.0),
                psnr: f.metrics.get("psnr_y").copied().unwrap_or(0.0),
                ssim: f.metrics.get("float_ssim").copied().unwrap_or(0.0),
            });
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_serde_roundtrip() {
        for m in &[Metric::Vmaf, Metric::Psnr, Metric::Ssim] {
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
    }

    #[test]
    fn test_metric_eq() {
        assert_eq!(Metric::Vmaf, Metric::Vmaf);
        assert_ne!(Metric::Vmaf, Metric::Psnr);
    }

    #[test]
    fn test_result_default() {
        let r = Result::default();
        assert!((r.vmaf - 0.0).abs() < 1e-9);
        assert!((r.psnr - 0.0).abs() < 1e-9);
        assert!((r.ssim - 0.0).abs() < 1e-9);
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
    fn test_measure_opts_default() {
        let opts = MeasureOpts::default();
        assert_eq!(opts.metrics.len(), 3);
        assert_eq!(opts.subsample, 0);
        assert_eq!(opts.model, "vmaf_v0.6.1");
        assert!(!opts.per_frame);
        assert!(opts.probe_cache.is_none());
    }
}
