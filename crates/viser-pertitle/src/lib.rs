use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};
use viser_checkpoint::Checkpoint;
use viser_encoding::{Config as EncodingConfig, ProgressSender, preset_for_codec};
use viser_ffmpeg::{Codec, EncodeJob, ProbeCache, ProbeResult, Resolution, encode, probe};
use viser_hull::{Crossover, Hull, Point, compute_per_codec, compute_upper};
use viser_ladder::{self, Ladder, Opts as LadderOpts};
use viser_quality::{self, MeasureOpts, Metric};

/// Config defines the search space and parameters for per-title analysis.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(flatten)]
    pub encoding: EncodingConfig,
    pub ladder_opts: LadderOpts,
    #[serde(default)]
    pub checkpoint_path: String,
    #[serde(default)]
    pub vmaf_model: String,
    #[serde(default)]
    pub allow_hdr: bool,
}

/// Complete output of a per-title analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    pub source: String,
    pub source_info: ProbeResult,
    pub config: Config,
    pub points: Vec<Point>,
    pub hull: Hull,
    pub per_codec: std::collections::HashMap<Codec, Hull>,
    pub crossovers: Vec<Crossover>,
    pub ladder: Ladder,
    pub duration: Duration,
    pub trial_count: usize,
    #[serde(default)]
    pub audio_bitrate_kbps: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Progress update for each completed trial encode.
#[derive(Debug, Clone)]
pub struct TrialProgress {
    pub done: usize,
    pub total: usize,
    pub resolution: Resolution,
    pub codec: Codec,
    pub crf: i32,
    pub bitrate: f64,
    pub vmaf: f64,
    pub error: Option<String>,
}

/// Runs a full per-title analysis on the given source video.
pub async fn analyze(
    source: &str,
    cfg: Config,
    progress_tx: Option<mpsc::Sender<TrialProgress>>,
) -> anyhow::Result<Result> {
    let start = Instant::now();

    // Validate input
    if !Path::new(source).exists() {
        anyhow::bail!("input file not found: {source}");
    }
    cfg.encoding.validate()?;

    // Probe source
    let source_info = probe(source).await?;
    let video = source_info
        .video_stream()
        .ok_or_else(|| anyhow::anyhow!("no video stream found in {source}"))?;
    video.validate()?;
    if video.is_hdr() && !cfg.allow_hdr {
        anyhow::bail!(
            "HDR source detected ({}) in {source}. Per-title analysis currently targets SDR/VMAF workflows; rerun with allow_hdr enabled only for best-effort behavior.",
            video.hdr_kind().unwrap_or("HDR")
        );
    }

    // Filter resolutions to those <= source resolution
    let mut resolutions: Vec<Resolution> =
        cfg.encoding.resolutions.iter().filter(|r| r.height <= video.height).copied().collect();
    if resolutions.is_empty() {
        resolutions = vec![cfg.encoding.resolutions[0]];
    }

    // Build trial matrix
    #[derive(Clone)]
    struct Trial {
        resolution: Resolution,
        codec: Codec,
        crf: i32,
    }

    let mut trials = Vec::new();
    for res in &resolutions {
        for codec in &cfg.encoding.codecs {
            for crf in &cfg.encoding.crf_values {
                trials.push(Trial { resolution: *res, codec: *codec, crf: *crf });
            }
        }
    }

    // Set up checkpointing
    let cp = if !cfg.checkpoint_path.is_empty() {
        let res_strs: Vec<String> = resolutions.iter().map(|r| r.label()).collect();
        let codec_strs: Vec<String> =
            cfg.encoding.codecs.iter().map(|c| c.as_str().to_string()).collect();
        let hash = viser_checkpoint::config_hash(
            source,
            &res_strs,
            &codec_strs,
            &cfg.encoding.crf_values,
            &cfg.encoding.preset,
        );
        let cp = Checkpoint::new(&cfg.checkpoint_path, &hash, source)?;
        if cp.completed_count() > 0 {
            info!(
                completed = cp.completed_count(),
                total = trials.len(),
                "resuming from checkpoint"
            );
        }
        Some(Arc::new(cp))
    } else {
        None
    };

    // Create temp directory
    let tmp_dir = tempfile::Builder::new().prefix("viser-pertitle-").tempdir()?;

    // Parallelism
    let parallel = cfg.encoding.effective_parallel();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallel));
    let probe_cache = ProbeCache::new();
    let sender = Arc::new(ProgressSender::new(progress_tx));

    let points = Arc::new(Mutex::new(Vec::new()));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    if video.is_hdr() {
        warnings.lock().await.push(format!(
            "HDR source detected ({}; transfer={}, primaries={}, pix_fmt={}). Metrics and ladder selection are best-effort.",
            video.hdr_kind().unwrap_or("HDR"),
            if video.color_transfer.is_empty() { "unknown" } else { &video.color_transfer },
            if video.color_primaries.is_empty() { "unknown" } else { &video.color_primaries },
            if video.pix_fmt.is_empty() { "unknown" } else { &video.pix_fmt },
        ));
    }
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total = trials.len();

    let mut handles = Vec::new();

    for t in trials {
        let sem = semaphore.clone();
        let cp = cp.clone();
        let source = source.to_string();
        let cfg = cfg.clone();
        let tmp_dir_path = tmp_dir.path().to_path_buf();
        let probe_cache = probe_cache.clone();
        let sender = sender.clone();
        let points = points.clone();
        let warnings = warnings.clone();
        let done = done.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            // Check checkpoint
            if let Some(ref cp) = cp {
                if let Some(p) = cp.get(&t.resolution.label(), t.codec.as_str(), t.crf) {
                    let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    points.lock().await.push(p.clone());
                    sender.send(TrialProgress {
                        done: d,
                        total,
                        resolution: t.resolution,
                        codec: t.codec,
                        crf: t.crf,
                        bitrate: p.bitrate,
                        vmaf: p.vmaf,
                        error: None,
                    });
                    return;
                }
            }

            let out_path = tmp_dir_path.join(format!(
                "{}_{}_crf{}.mp4",
                t.resolution.label(),
                t.codec.as_str(),
                t.crf
            ));

            let job = EncodeJob {
                input: source.clone(),
                output: out_path.to_string_lossy().to_string(),
                resolution: Some(t.resolution),
                codec: t.codec,
                crf: t.crf,
                rate_control: cfg.encoding.rate_control,
                target_bitrate: 0.0,
                max_bitrate: 0.0,
                bufsize: 0.0,
                preset: preset_for_codec(t.codec, &cfg.encoding.preset),
                extra_args: vec![],
            };

            let enc_result = match encode(job, None).await {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!(
                        "encode failed ({} {} CRF {}): {e}",
                        t.resolution.label(),
                        t.codec.as_str(),
                        t.crf
                    );
                    warn!("{msg}");
                    warnings.lock().await.push(msg.clone());
                    let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    sender.send(TrialProgress {
                        done: d,
                        total,
                        resolution: t.resolution,
                        codec: t.codec,
                        crf: t.crf,
                        bitrate: 0.0,
                        vmaf: 0.0,
                        error: Some(msg),
                    });
                    return;
                }
            };

            // Measure quality
            let q_result = match viser_quality::measure(
                &source,
                &out_path.to_string_lossy(),
                MeasureOpts {
                    metrics: vec![Metric::Vmaf, Metric::Psnr],
                    subsample: cfg.encoding.subsample,
                    model: cfg.vmaf_model.clone(),
                    probe_cache: Some(probe_cache.clone()),
                    ..Default::default()
                },
            )
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!(
                        "quality measurement failed ({} {} CRF {}): {e}",
                        t.resolution.label(),
                        t.codec.as_str(),
                        t.crf
                    );
                    warn!("{msg}");
                    warnings.lock().await.push(msg.clone());
                    let _ = std::fs::remove_file(&out_path);
                    let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    sender.send(TrialProgress {
                        done: d,
                        total,
                        resolution: t.resolution,
                        codec: t.codec,
                        crf: t.crf,
                        bitrate: 0.0,
                        vmaf: 0.0,
                        error: Some(msg),
                    });
                    return;
                }
            };

            let _ = std::fs::remove_file(&out_path);

            let p = Point {
                resolution: t.resolution,
                codec: t.codec,
                crf: t.crf,
                bitrate: enc_result.bitrate,
                vmaf: q_result.vmaf,
                psnr: q_result.psnr,
                ssim: 0.0,
            };

            if let Some(ref cp) = cp {
                if let Err(e) = cp.save(&t.resolution.label(), t.codec.as_str(), t.crf, p.clone()) {
                    warn!("checkpoint save failed: {e}");
                }
            }

            let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            points.lock().await.push(p.clone());
            sender.send(TrialProgress {
                done: d,
                total,
                resolution: t.resolution,
                codec: t.codec,
                crf: t.crf,
                bitrate: p.bitrate,
                vmaf: p.vmaf,
                error: None,
            });
        }));
    }

    for h in handles {
        h.await?;
    }

    let points = Arc::try_unwrap(points).unwrap().into_inner();
    let warnings = Arc::try_unwrap(warnings).unwrap().into_inner();

    if points.is_empty() {
        anyhow::bail!("all {total} trials failed; check warnings");
    }

    let all_hull = compute_upper(&points);
    let per_codec = compute_per_codec(&points);
    let crossovers = all_hull.crossovers();
    let mut ladder_opts = cfg.ladder_opts.clone();
    ladder_opts.audio_bitrate_kbps = source_info
        .audio_stream()
        .and_then(|a| if a.bit_rate > 0 { Some(a.bit_rate as f64 / 1000.0) } else { None })
        .unwrap_or(0.0);
    let selected_ladder = viser_ladder::select(&all_hull, &ladder_opts);

    if let Some(ref cp) = cp {
        let _ = cp.remove();
    }

    Ok(Result {
        source: source.to_string(),
        source_info: source_info.clone(),
        config: cfg,
        points,
        hull: all_hull,
        per_codec,
        crossovers,
        ladder: selected_ladder,
        duration: start.elapsed(),
        trial_count: total,
        audio_bitrate_kbps: ladder_opts.audio_bitrate_kbps,
        warnings,
    })
}

impl Result {
    pub fn save_json(&self, path: &str) -> anyhow::Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn load_json(path: &str) -> anyhow::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&data)?)
    }
}
