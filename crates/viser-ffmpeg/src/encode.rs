use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::{Codec, EncoderBackend, RateControlMode, Resolution, ffmpeg_path, probe};

/// Parameters for a single encode.
#[derive(Debug, Clone)]
pub struct EncodeJob {
    /// Source media file path.
    pub input: String,
    /// Destination file path for the encoded output.
    pub output: String,
    /// Optional target resolution; when set, scales with the lanczos filter.
    pub resolution: Option<Resolution>,
    /// Video codec to encode with.
    pub codec: Codec,
    /// Constant rate factor / quantizer value (interpretation depends on `rate_control`).
    pub crf: i32,
    /// Rate-control mode that determines how `crf`/bitrate fields are applied.
    pub rate_control: RateControlMode,
    /// Target bitrate in kbps; used for VBR mode.
    pub target_bitrate: f64, // kbps, used for VBR mode
    /// Maximum bitrate cap in kbps; used for capped CRF mode.
    pub max_bitrate: f64, // kbps, used for capped CRF mode
    /// VBV buffer size in kbps; used for capped CRF mode.
    pub bufsize: f64, // kbps, used for capped CRF mode
    /// Encoder speed preset (e.g. `"medium"`); empty leaves the encoder default.
    pub preset: String,
    /// Optional hardware-accelerated decode method (e.g. `"vaapi"`, `"cuda"`,
    /// `"qsv"`, `"videotoolbox"`). `None` (or empty) decodes in software.
    /// Frames are downloaded to system memory for the filter/encode pipeline.
    pub hwaccel: Option<String>,
    /// Extra raw FFmpeg arguments appended verbatim before the output path.
    pub extra_args: Vec<String>,
}

/// Output of a completed encode.
#[derive(Debug, Clone)]
pub struct EncodeResult {
    /// The job that produced this result.
    pub job: EncodeJob,
    /// Average bitrate of the output in kbps, measured by probing it.
    pub bitrate: f64, // kbps (average)
    /// Output file size in bytes.
    pub file_size: u64, // bytes
    /// Wall-clock time taken to encode.
    pub duration: Duration, // wall-clock encode time
}

/// Real-time encoding progress info parsed from FFmpeg.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Number of frames encoded so far.
    pub frame: i64,
    /// Current encoding rate in frames per second.
    pub fps: f64,
    /// Current output bitrate in kbps.
    pub bitrate: f64, // kbps
    /// Encoding speed relative to real time (e.g. 2.5 means 2.5x).
    pub speed: f64, // e.g. 2.5x
    /// Output timestamp reached so far.
    pub time: Duration,
}

/// Runs an FFmpeg encode job. Progress updates are sent on the channel if provided.
pub async fn encode(
    job: EncodeJob,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<EncodeResult> {
    match job.rate_control {
        RateControlMode::Vbr => encode_two_pass(job, progress_tx).await,
        _ => encode_single_pass(job, progress_tx).await,
    }
}

async fn encode_single_pass(
    job: EncodeJob,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<EncodeResult> {
    let args = build_encode_args(&job, EncodePass::Single)?;
    run_encode(job, args, progress_tx).await
}

async fn encode_two_pass(
    job: EncodeJob,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<EncodeResult> {
    if job.target_bitrate <= 0.0 {
        anyhow::bail!("target bitrate must be greater than zero for VBR mode");
    }

    let passlog_prefix = make_passlog_prefix(&job.output);
    let cleanup = PasslogCleanup::new(passlog_prefix.clone());

    let first_pass_args = build_encode_args(&job, EncodePass::First(&passlog_prefix))?;
    run_ffmpeg(first_pass_args, None).await?;

    let second_pass_args = build_encode_args(&job, EncodePass::Second(&passlog_prefix))?;
    let result = run_encode(job, second_pass_args, progress_tx).await;

    cleanup.run();
    result
}

async fn run_encode(
    job: EncodeJob,
    args: Vec<String>,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<EncodeResult> {
    let start = Instant::now();
    run_ffmpeg(args, progress_tx).await?;

    let elapsed = start.elapsed();

    // Probe the output to get actual bitrate and file size
    let meta = std::fs::metadata(&job.output)
        .map_err(|e| anyhow::anyhow!("failed to stat output: {e}"))?;

    let probe_result = probe(&job.output).await?;
    let bitrate = probe_result.format.bit_rate as f64 / 1000.0;

    Ok(EncodeResult { job, bitrate, file_size: meta.len(), duration: elapsed })
}

async fn run_ffmpeg(
    args: Vec<String>,
    progress_tx: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<()> {
    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(&args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("failed to start ffmpeg: {e}"))?;

    // Parse progress from stdout
    if let Some(stdout) = child.stdout.take() {
        let tx = progress_tx.clone();
        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            let mut p = Progress::default();
            while let Ok(Some(line)) = lines.next_line().await {
                if parse_progress_line(&line, &mut p)
                    && let Some(ref tx) = tx
                {
                    let _ = tx.try_send(p.clone());
                }
            }
        });
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg encode failed: {stderr}");
    }

    Ok(())
}

/// Copies a segment of a video file without re-encoding.
pub async fn extract(input: &str, output: &str, start: f64, duration: f64) -> anyhow::Result<()> {
    if start.is_finite() && start < 0.0 {
        anyhow::bail!("extract start must be non-negative, got {start}");
    }
    if !duration.is_finite() || duration <= 0.0 {
        anyhow::bail!("extract duration must be positive, got {duration}");
    }

    let args = vec![
        "-y".to_string(),
        "-ss".into(),
        format!("{start:.6}"),
        "-i".into(),
        input.into(),
        "-t".into(),
        format!("{duration:.6}"),
        "-c".into(),
        "copy".into(),
        "-avoid_negative_ts".into(),
        "make_zero".into(),
        output.into(),
    ];

    let output = Command::new(ffmpeg_path())
        .args(&args)
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg extract failed: {stderr}");
    }
    Ok(())
}

/// Concatenates multiple encoded chunks into a single output without re-encoding.
pub async fn concat(inputs: &[String], output: &str) -> anyhow::Result<()> {
    if inputs.is_empty() {
        anyhow::bail!("cannot concat an empty input list");
    }

    let list_path = make_concat_list_path(output);
    let list_body = inputs
        .iter()
        .map(|path| format!("file '{}'", escape_concat_path(path)))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_path, format!("{list_body}\n"))?;

    let args = vec![
        "-y".to_string(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list_path.to_string_lossy().into_owned(),
        "-c".into(),
        "copy".into(),
        output.into(),
    ];

    let result = run_ffmpeg(args, None).await;
    let _ = std::fs::remove_file(&list_path);
    result
}

enum EncodePass<'a> {
    Single,
    First(&'a Path),
    Second(&'a Path),
}

fn build_encode_args(job: &EncodeJob, pass: EncodePass<'_>) -> anyhow::Result<Vec<String>> {
    let mut args = vec!["-y".into()];

    // ── Input-level options (must precede -i) ──
    // Hardware-accelerated decode. Without an explicit output format the decoded
    // frames are downloaded back to system memory, so the rest of the software
    // filter/encode pipeline keeps working unchanged.
    if let Some(accel) = job.hwaccel.as_deref().filter(|a| !a.is_empty()) {
        args.extend(["-hwaccel".into(), accel.into()]);
    }
    // VAAPI encoders require a render device initialised before the input so the
    // `hwupload` filter has a target surface pool.
    if job.codec.backend() == EncoderBackend::Vaapi {
        args.extend(["-vaapi_device".into(), vaapi_device()]);
    }

    args.extend(["-i".into(), job.input.clone(), "-an".into()]);

    if !matches!(pass, EncodePass::First(_)) {
        args.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    }

    args.extend(["-c:v".into(), job.codec.as_str().into()]);

    if job.codec.is_hardware() {
        build_hw_args(&mut args, job, &pass)?;
    } else {
        build_sw_args(&mut args, job, &pass)?;
    }

    if !job.preset.is_empty() {
        if job.codec.is_hardware() {
            add_hw_preset(&mut args, job.codec, &job.preset);
        } else {
            args.extend(["-preset".into(), job.preset.clone()]);
        }
    }

    if let Some(vf) = build_filter_chain(job) {
        args.extend(["-vf".into(), vf]);
    }

    args.extend(job.extra_args.iter().cloned());

    match pass {
        EncodePass::First(_) => {
            args.extend(["-f".into(), "null".into()]);
            args.push(null_output_path().into());
        }
        EncodePass::Single | EncodePass::Second(_) => args.push(job.output.clone()),
    }

    Ok(args)
}

/// VAAPI render node to initialise. Overridable via `VISER_VAAPI_DEVICE` for
/// hosts where the primary render node is not `renderD128`.
fn vaapi_device() -> String {
    std::env::var("VISER_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".to_string())
}

/// Builds the `-vf` filter-chain value, or `None` when no filtering is needed.
///
/// Software encoders only scale (lanczos) when a target resolution is set.
/// VAAPI encoders additionally need the frames converted and uploaded to GPU
/// surfaces (`format=nv12,hwupload`), since the encoder consumes VAAPI surfaces
/// — without this the encode fails with a format-conversion error.
fn build_filter_chain(job: &EncodeJob) -> Option<String> {
    let scale = job
        .resolution
        .filter(|res| res.width > 0 && res.height > 0)
        .map(|res| format!("scale={}:{}:flags=lanczos", res.width, res.height));

    if job.codec.backend() == EncoderBackend::Vaapi {
        // Software-scale (if requested) in system memory, then upload to a VAAPI
        // surface for the encoder.
        Some(match scale {
            Some(s) => format!("{s},format=nv12,hwupload"),
            None => "format=nv12,hwupload".to_string(),
        })
    } else {
        scale
    }
}

fn build_sw_args(
    args: &mut Vec<String>,
    job: &EncodeJob,
    pass: &EncodePass<'_>,
) -> anyhow::Result<()> {
    match job.rate_control {
        RateControlMode::Qp => {
            if job.codec == Codec::SvtAv1 {
                args.extend(["-qp".into(), job.crf.to_string()]);
                args.extend(["-svtav1-params".into(), "enable-adaptive-quantization=0".into()]);
            } else {
                args.extend(["-qp".into(), job.crf.to_string()]);
            }
        }
        RateControlMode::CappedCrf => {
            if job.max_bitrate <= 0.0 {
                anyhow::bail!("max bitrate must be greater than zero for capped CRF mode");
            }
            let bufsize = if job.bufsize > 0.0 { job.bufsize } else { job.max_bitrate * 2.0 };
            args.extend(["-crf".into(), job.crf.to_string()]);
            args.extend(["-maxrate".into(), format!("{:.0}k", job.max_bitrate)]);
            args.extend(["-bufsize".into(), format!("{bufsize:.0}k")]);
        }
        RateControlMode::Vbr => {
            if job.target_bitrate <= 0.0 {
                anyhow::bail!("target bitrate must be greater than zero for VBR mode");
            }
            args.extend(["-b:v".into(), format!("{:.0}k", job.target_bitrate)]);
            args.extend(["-maxrate".into(), format!("{:.0}k", job.target_bitrate * 2.0)]);
            args.extend(["-bufsize".into(), format!("{:.0}k", job.target_bitrate * 4.0)]);

            let passlog = match pass {
                EncodePass::First(path) => {
                    args.extend(["-pass".into(), "1".into()]);
                    path
                }
                EncodePass::Second(path) => {
                    args.extend(["-pass".into(), "2".into()]);
                    path
                }
                EncodePass::Single => {
                    anyhow::bail!("VBR mode requires a two-pass encode flow");
                }
            };
            args.extend(["-passlogfile".into(), passlog.to_string_lossy().into_owned()]);
        }
        RateControlMode::Crf => {
            args.extend(["-crf".into(), job.crf.to_string()]);
        }
    }
    Ok(())
}

fn build_hw_args(
    args: &mut Vec<String>,
    job: &EncodeJob,
    _pass: &EncodePass<'_>,
) -> anyhow::Result<()> {
    let backend = job.codec.backend();
    match job.rate_control {
        RateControlMode::Crf | RateControlMode::CappedCrf => {
            match backend {
                EncoderBackend::Nvenc => {
                    let cq = crf_to_nvenc_cq(job.crf);
                    args.extend(["-cq".into(), cq.to_string()]);
                    // `constqp` is constant-QP and ignores -maxrate/-bufsize, so a
                    // capped-CRF encode must use VBR for the bitrate cap to take effect.
                    let rc = if matches!(job.rate_control, RateControlMode::CappedCrf) {
                        "vbr"
                    } else {
                        "constqp"
                    };
                    args.extend(["-rc".into(), rc.into()]);
                }
                EncoderBackend::Qsv => {
                    let gq = crf_to_qsv_quality(job.crf);
                    args.extend(["-global_quality".into(), gq.to_string()]);
                }
                EncoderBackend::VideoToolbox => {
                    let qual = crf_to_vt_quality(job.crf);
                    args.extend(["-quality".into(), qual.to_string()]);
                }
                EncoderBackend::Vaapi => {
                    let gq = crf_to_qsv_quality(job.crf);
                    args.extend(["-global_quality".into(), gq.to_string()]);
                }
                EncoderBackend::Amf => {
                    args.extend(["-qp_i".into(), job.crf.to_string()]);
                    args.extend(["-qp_p".into(), (job.crf + 2).to_string()]);
                    args.extend(["-usage".into(), "transcoding".into()]);
                }
                EncoderBackend::Software => unreachable!(),
            }
            // VBV / maxrate for capped mode
            if let RateControlMode::CappedCrf = job.rate_control {
                if job.max_bitrate <= 0.0 {
                    anyhow::bail!("max bitrate must be greater than zero for capped CRF mode");
                }
                let bufsize = if job.bufsize > 0.0 { job.bufsize } else { job.max_bitrate * 2.0 };
                args.extend(["-maxrate".into(), format!("{:.0}k", job.max_bitrate)]);
                args.extend(["-bufsize".into(), format!("{bufsize:.0}k")]);
            }
        }
        RateControlMode::Qp => match backend {
            EncoderBackend::VideoToolbox => {
                anyhow::bail!("VideoToolbox does not support QP rate control mode");
            }
            _ => {
                args.extend(["-qp".into(), job.crf.to_string()]);
            }
        },
        RateControlMode::Vbr => {
            if job.target_bitrate <= 0.0 {
                anyhow::bail!("target bitrate must be greater than zero for VBR mode");
            }
            args.extend(["-b:v".into(), format!("{:.0}k", job.target_bitrate)]);
            args.extend(["-maxrate".into(), format!("{:.0}k", job.target_bitrate * 2.0)]);
            args.extend(["-bufsize".into(), format!("{:.0}k", job.target_bitrate * 4.0)]);

            if backend == EncoderBackend::Nvenc {
                args.extend(["-rc".into(), "vbr_hq".into()]);
            }
        }
    }
    Ok(())
}

fn crf_to_nvenc_cq(crf: i32) -> i32 {
    let cq = (crf * 51) / 63;
    cq.clamp(1, 51)
}

fn crf_to_qsv_quality(crf: i32) -> i32 {
    let gq = 100 - ((crf * 100) / 51);
    gq.clamp(1, 100)
}

fn crf_to_vt_quality(crf: i32) -> f64 {
    let q = 1.0 - (crf as f64 / 51.0);
    q.clamp(0.0, 1.0)
}

fn add_hw_preset(args: &mut Vec<String>, codec: Codec, preset: &str) {
    match codec.backend() {
        EncoderBackend::Nvenc => {
            let p = map_nvenc_preset(preset);
            args.extend(["-preset".into(), p.into()]);
        }
        EncoderBackend::Qsv => {
            args.extend(["-preset".into(), preset.to_string()]);
        }
        EncoderBackend::Vaapi => {
            args.extend(["-compression_level".into(), map_vaapi_preset(preset).into()]);
        }
        EncoderBackend::Amf => {
            args.extend(["-quality".into(), map_amf_quality(preset).into()]);
        }
        EncoderBackend::VideoToolbox => {
            if preset == "ultrafast" || preset == "superfast" || preset == "veryfast" {
                args.extend(["-realtime".into(), "1".into()]);
            }
        }
        EncoderBackend::Software => unreachable!(),
    }
}

fn map_nvenc_preset(preset: &str) -> &str {
    match preset {
        "ultrafast" | "superfast" => "p1",
        "veryfast" => "p2",
        "faster" => "p3",
        "fast" => "p4",
        "medium" => "p5",
        "slow" => "p6",
        "slower" | "veryslow" => "p7",
        other => other,
    }
}

fn map_vaapi_preset(preset: &str) -> &str {
    match preset {
        "ultrafast" | "superfast" => "1",
        "veryfast" | "faster" => "2",
        "fast" | "medium" => "3",
        "slow" => "4",
        "slower" | "veryslow" => "5",
        other => other,
    }
}

fn map_amf_quality(preset: &str) -> &str {
    match preset {
        "ultrafast" | "superfast" => "speed",
        "veryfast" | "faster" | "fast" => "balanced",
        "medium" | "slow" | "slower" | "veryslow" => "quality",
        other => other,
    }
}

/// Escape a path for use inside single quotes in an FFmpeg concat list file.
/// The concat demuxer treats backslash as an escape character, so both
/// backslashes and single quotes must be escaped.
fn escape_concat_path(path: &str) -> String {
    path.replace('\\', "\\\\").replace('\'', "\\'")
}

fn make_passlog_prefix(output: &str) -> PathBuf {
    let output_path = Path::new(output);
    let parent =
        output_path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("viser");
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    parent.join(format!(".{stem}.viser-passlog-{unique}-{}", std::process::id()))
}

fn make_concat_list_path(output: &str) -> PathBuf {
    let output_path = Path::new(output);
    let parent =
        output_path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("viser");
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    parent.join(format!(".{stem}.viser-concat-{unique}-{}.txt", std::process::id()))
}

fn null_output_path() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

struct PasslogCleanup {
    parent: PathBuf,
    prefix: String,
}

impl PasslogCleanup {
    fn new(path: PathBuf) -> Self {
        let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let prefix = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
        Self { parent, prefix }
    }

    fn run(&self) {
        let Ok(entries) = std::fs::read_dir(&self.parent) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with(&self.prefix) {
                continue;
            }
            if let Err(err) = std::fs::remove_file(&path) {
                tracing::debug!(?path, ?err, "failed to remove ffmpeg two-pass log file");
            }
        }
    }
}

/// Returns true when a complete progress block is ready.
fn parse_progress_line(line: &str, p: &mut Progress) -> bool {
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };

    match key {
        "frame" => {
            p.frame = value.parse().unwrap_or(0);
        }
        "fps" => {
            p.fps = value.parse().unwrap_or(0.0);
        }
        "bitrate" => {
            let v = value.trim_end_matches("kbits/s");
            p.bitrate = v.parse().unwrap_or(0.0);
        }
        "speed" => {
            let v = value.trim_end_matches('x');
            p.speed = v.parse().unwrap_or(0.0);
        }
        "out_time_us" => {
            let us: u64 = value.parse().unwrap_or(0);
            p.time = Duration::from_micros(us);
        }
        "progress" => return true,
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Codec;

    fn sample_job(mode: RateControlMode) -> EncodeJob {
        EncodeJob {
            input: "input.mp4".into(),
            output: "output.mp4".into(),
            resolution: Some(crate::Resolution::new(1280, 720)),
            codec: Codec::X264,
            crf: 23,
            rate_control: mode,
            target_bitrate: 2500.0,
            max_bitrate: 3000.0,
            bufsize: 6000.0,
            preset: "medium".into(),
            hwaccel: None,
            extra_args: vec![],
        }
    }

    fn job_with_codec(codec: Codec, mode: RateControlMode) -> EncodeJob {
        EncodeJob { codec, rate_control: mode, ..sample_job(mode) }
    }

    // ── Helper: find adjacent argument pair ──
    fn has_pair(args: &[String], a: &str, b: &str) -> bool {
        args.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    fn has_arg(args: &[String], a: &str) -> bool {
        args.iter().any(|s| s == a)
    }

    // ── Software CRF ──
    #[test]
    fn test_build_encode_args_crf_single_pass() {
        let args =
            build_encode_args(&sample_job(RateControlMode::Crf), EncodePass::Single).unwrap();
        assert!(args.windows(2).any(|w| w == ["-crf", "23"]));
        assert_eq!(args.last().unwrap(), "output.mp4");
    }

    #[test]
    fn test_x264_crf_args() {
        let args = build_encode_args(
            &job_with_codec(Codec::X264, RateControlMode::Crf),
            EncodePass::Single,
        )
        .unwrap();
        assert!(has_pair(&args, "-c:v", "libx264"));
        assert!(has_pair(&args, "-crf", "23"));
        assert!(has_pair(&args, "-preset", "medium"));
    }

    #[test]
    fn test_x265_crf_args() {
        let args = build_encode_args(
            &job_with_codec(Codec::X265, RateControlMode::Crf),
            EncodePass::Single,
        )
        .unwrap();
        assert!(has_pair(&args, "-c:v", "libx265"));
        assert!(has_pair(&args, "-crf", "23"));
    }

    #[test]
    fn test_svtav1_crf_args() {
        let args = build_encode_args(
            &job_with_codec(Codec::SvtAv1, RateControlMode::Crf),
            EncodePass::Single,
        )
        .unwrap();
        assert!(has_pair(&args, "-c:v", "libsvtav1"));
        assert!(has_pair(&args, "-crf", "23"));
    }

    // ── Software QP ──
    #[test]
    fn test_x264_qp_args() {
        let args = build_encode_args(
            &job_with_codec(Codec::X264, RateControlMode::Qp),
            EncodePass::Single,
        )
        .unwrap();
        assert!(has_pair(&args, "-qp", "23"));
        assert!(!has_arg(&args, "-crf"));
    }

    #[test]
    fn test_x265_qp_args() {
        let args = build_encode_args(
            &job_with_codec(Codec::X265, RateControlMode::Qp),
            EncodePass::Single,
        )
        .unwrap();
        assert!(has_pair(&args, "-qp", "23"));
    }

    #[test]
    fn test_svtav1_qp_adds_adaptive_quantization_off() {
        let args = build_encode_args(
            &job_with_codec(Codec::SvtAv1, RateControlMode::Qp),
            EncodePass::Single,
        )
        .unwrap();
        assert!(has_pair(&args, "-qp", "23"));
        assert!(has_pair(&args, "-svtav1-params", "enable-adaptive-quantization=0"));
    }

    // ── Software Capped CRF ──
    #[test]
    fn test_build_encode_args_capped_crf_sets_vbv() {
        let args =
            build_encode_args(&sample_job(RateControlMode::CappedCrf), EncodePass::Single).unwrap();
        assert!(args.windows(2).any(|w| w == ["-crf", "23"]));
        assert!(args.windows(2).any(|w| w == ["-maxrate", "3000k"]));
        assert!(args.windows(2).any(|w| w == ["-bufsize", "6000k"]));
    }

    #[test]
    fn test_capped_crf_max_bitrate_zero_errors() {
        let job = EncodeJob {
            max_bitrate: 0.0,
            rate_control: RateControlMode::CappedCrf,
            ..sample_job(RateControlMode::CappedCrf)
        };
        assert!(build_encode_args(&job, EncodePass::Single).is_err());
    }

    #[test]
    fn test_capped_crf_max_bitrate_negative_errors() {
        let job = EncodeJob {
            max_bitrate: -1.0,
            rate_control: RateControlMode::CappedCrf,
            ..sample_job(RateControlMode::CappedCrf)
        };
        assert!(build_encode_args(&job, EncodePass::Single).is_err());
    }

    #[test]
    fn test_capped_crf_auto_bufsize_when_zero() {
        let job = EncodeJob {
            max_bitrate: 4000.0,
            bufsize: 0.0,
            rate_control: RateControlMode::CappedCrf,
            ..sample_job(RateControlMode::CappedCrf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-bufsize", "8000k"));
    }

    // ── Software VBR ──
    #[test]
    fn test_vbr_target_bitrate_zero_errors() {
        let job = EncodeJob {
            target_bitrate: 0.0,
            rate_control: RateControlMode::Vbr,
            ..sample_job(RateControlMode::Vbr)
        };
        assert!(build_encode_args(&job, EncodePass::First(Path::new("passlog"))).is_err());
    }

    #[test]
    fn test_vbr_single_pass_errors() {
        assert!(build_encode_args(&sample_job(RateControlMode::Vbr), EncodePass::Single).is_err());
    }

    #[test]
    fn test_build_encode_args_vbr_first_pass_uses_null_output() {
        let job = sample_job(RateControlMode::Vbr);
        let passlog = Path::new("/tmp/viser-passlog");
        let args = build_encode_args(&job, EncodePass::First(passlog)).unwrap();
        assert!(args.windows(2).any(|w| w == ["-pass", "1"]));
        assert!(args.windows(2).any(|w| w == ["-f", "null"]));
        assert_eq!(args.last().unwrap(), null_output_path());
    }

    #[test]
    fn test_build_encode_args_vbr_second_pass_writes_output() {
        let job = sample_job(RateControlMode::Vbr);
        let passlog = Path::new("/tmp/viser-passlog");
        let args = build_encode_args(&job, EncodePass::Second(passlog)).unwrap();
        assert!(args.windows(2).any(|w| w == ["-pass", "2"]));
        assert_eq!(args.last().unwrap(), "output.mp4");
    }

    #[test]
    fn test_vbr_first_pass_no_progress_args() {
        let job = sample_job(RateControlMode::Vbr);
        let passlog = Path::new("/tmp/viser-passlog");
        let args = build_encode_args(&job, EncodePass::First(passlog)).unwrap();
        assert!(!has_arg(&args, "-progress"));
        assert!(!has_arg(&args, "-nostats"));
    }

    #[test]
    fn test_vbr_second_pass_sets_bitrate_and_vbv() {
        let job = sample_job(RateControlMode::Vbr);
        let passlog = Path::new("/tmp/viser-passlog");
        let args = build_encode_args(&job, EncodePass::Second(passlog)).unwrap();
        assert!(has_pair(&args, "-b:v", "2500k"));
        assert!(has_pair(&args, "-maxrate", "5000k"));
        assert!(has_pair(&args, "-bufsize", "10000k"));
    }

    #[test]
    fn test_vbr_sets_passlog() {
        let job = sample_job(RateControlMode::Vbr);
        let passlog = Path::new("/tmp/viser-passlog");
        let args = build_encode_args(&job, EncodePass::First(passlog)).unwrap();
        assert!(has_arg(&args, "/tmp/viser-passlog"));
    }

    // ── Resolution scaling ──
    #[test]
    fn test_resolution_scaling_adds_vf() {
        let args =
            build_encode_args(&sample_job(RateControlMode::Crf), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-vf"));
        assert!(has_arg(&args, "scale=1280:720:flags=lanczos"));
    }

    #[test]
    fn test_zero_width_skips_scale() {
        let job = EncodeJob {
            resolution: Some(crate::Resolution::new(0, 720)),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-vf"));
    }

    #[test]
    fn test_zero_height_skips_scale() {
        let job = EncodeJob {
            resolution: Some(crate::Resolution::new(1280, 0)),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-vf"));
    }

    #[test]
    fn test_no_resolution_skips_scale() {
        let job = EncodeJob { resolution: None, ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-vf"));
    }

    #[test]
    fn test_resolution_negative_skip_scale() {
        let job = EncodeJob {
            resolution: Some(crate::Resolution::new(-1, -1)),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-vf"));
    }

    // ── Preset handling ──
    #[test]
    fn test_empty_preset_no_preset_arg() {
        let job = EncodeJob { preset: String::new(), ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-preset"));
    }

    #[test]
    fn test_preset_with_x264() {
        let job = EncodeJob {
            codec: Codec::X264,
            preset: "fast".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-preset", "fast"));
    }

    // ── Extra args ──
    #[test]
    fn test_extra_args_appended_before_output() {
        let job = EncodeJob {
            extra_args: vec!["-g".into(), "30".into(), "-bf".into(), "2".into()],
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-g", "30"));
        assert!(has_pair(&args, "-bf", "2"));
        assert_eq!(args.last().unwrap(), "output.mp4");
    }

    // ── Null output path ──
    #[test]
    fn test_null_output_path_is_platform_appropriate() {
        let null = null_output_path();
        assert!(!null.is_empty());
        assert!(null == "/dev/null" || null == "NUL");
    }

    // ── Progress parsing ──
    #[test]
    fn test_parse_progress_line_frame() {
        let mut p = Progress::default();
        assert!(!parse_progress_line("frame=100", &mut p));
        assert_eq!(p.frame, 100);
    }

    #[test]
    fn test_parse_progress_line_fps() {
        let mut p = Progress::default();
        parse_progress_line("fps=23.976", &mut p);
        assert!((p.fps - 23.976).abs() < 0.001);
    }

    #[test]
    fn test_parse_progress_line_bitrate() {
        let mut p = Progress::default();
        parse_progress_line("bitrate=1500.5kbits/s", &mut p);
        assert!((p.bitrate - 1500.5).abs() < 0.001);
    }

    #[test]
    fn test_parse_progress_line_speed() {
        let mut p = Progress::default();
        parse_progress_line("speed=1.5x", &mut p);
        assert!((p.speed - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_parse_progress_line_out_time_us() {
        let mut p = Progress::default();
        parse_progress_line("out_time_us=1234567", &mut p);
        assert_eq!(p.time, Duration::from_micros(1234567));
    }

    #[test]
    fn test_parse_progress_returns_true_on_progress() {
        let mut p = Progress::default();
        assert!(parse_progress_line("progress=continue", &mut p));
    }

    #[test]
    fn test_parse_progress_full_block() {
        let mut p = Progress::default();
        parse_progress_line("frame=1500", &mut p);
        parse_progress_line("fps=25.0", &mut p);
        parse_progress_line("bitrate=2000.0kbits/s", &mut p);
        parse_progress_line("speed=2.0x", &mut p);
        parse_progress_line("out_time_us=60000000", &mut p);
        assert!(parse_progress_line("progress=continue", &mut p));
        assert_eq!(p.frame, 1500);
        assert_eq!(p.time, Duration::from_secs(60));
    }

    #[test]
    fn test_parse_progress_line_missing_equals() {
        let mut p = Progress::default();
        assert!(!parse_progress_line("noequals", &mut p));
    }

    #[test]
    fn test_parse_progress_line_unknown_key() {
        let mut p = Progress::default();
        assert!(!parse_progress_line("unknown=42", &mut p));
    }

    #[test]
    fn test_parse_progress_line_bogus_numbers() {
        let mut p = Progress::default();
        parse_progress_line("frame=abc", &mut p);
        assert_eq!(p.frame, 0);
    }

    // ── Make passlog prefix ──
    #[test]
    fn test_make_passlog_prefix_uses_output_dir() {
        let prefix = make_passlog_prefix("/path/to/video.mp4");
        assert!(prefix.starts_with(Path::new("/path/to")));
        assert!(prefix.to_string_lossy().contains("video"));
    }

    #[test]
    fn test_make_passlog_prefix_no_parent_falls_back_to_cwd() {
        let prefix = make_passlog_prefix("video.mp4");
        assert!(prefix.starts_with(Path::new(".")));
    }

    // ── Make concat list path ──
    #[test]
    fn test_make_concat_list_path_is_txt() {
        let path = make_concat_list_path("output.mp4");
        assert!(path.to_string_lossy().ends_with(".txt"));
    }

    // ── Concat path escaping ──
    #[test]
    fn test_escape_concat_path_escapes_single_quotes() {
        assert_eq!(escape_concat_path("video's.mp4"), "video\\'s.mp4");
    }

    #[test]
    fn test_escape_concat_path_escapes_backslashes() {
        assert_eq!(escape_concat_path("dir\\video.mp4"), "dir\\\\video.mp4");
    }

    #[test]
    fn test_escape_concat_path_no_change_for_simple_paths() {
        assert_eq!(escape_concat_path("/tmp/video.mp4"), "/tmp/video.mp4");
    }

    // ── Extract input validation ──
    #[tokio::test]
    async fn test_extract_rejects_negative_start() {
        let err = extract("in.mp4", "out.mp4", -1.0, 5.0).await.unwrap_err();
        assert!(err.to_string().contains("start must be non-negative"));
    }

    #[tokio::test]
    async fn test_extract_rejects_zero_duration() {
        let err = extract("in.mp4", "out.mp4", 0.0, 0.0).await.unwrap_err();
        assert!(err.to_string().contains("duration must be positive"));
    }

    #[tokio::test]
    async fn test_extract_rejects_negative_duration() {
        let err = extract("in.mp4", "out.mp4", 0.0, -5.0).await.unwrap_err();
        assert!(err.to_string().contains("duration must be positive"));
    }

    #[tokio::test]
    async fn test_extract_rejects_nan_duration() {
        let err = extract("in.mp4", "out.mp4", 0.0, f64::NAN).await.unwrap_err();
        assert!(err.to_string().contains("duration must be positive"));
    }

    // ── Helper: hardware-specific job builders ──
    fn hw_crf(codec: Codec) -> EncodeJob {
        EncodeJob {
            codec,
            preset: String::new(),
            resolution: None,
            extra_args: vec![],
            ..sample_job(RateControlMode::Crf)
        }
    }

    fn hw_qp(codec: Codec) -> EncodeJob {
        EncodeJob {
            codec,
            preset: String::new(),
            resolution: None,
            extra_args: vec![],
            ..sample_job(RateControlMode::Qp)
        }
    }

    // ── Hardware encoder CRF (quality-based constant mode) ──
    #[test]
    fn test_nvenc_h264_crf_uses_constqp() {
        let args = build_encode_args(&hw_crf(Codec::NvencH264), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-rc", "constqp"));
        assert!(has_arg(&args, "-cq"));
    }

    #[test]
    fn test_nvenc_h265_crf_uses_constqp() {
        let args = build_encode_args(&hw_crf(Codec::NvencH265), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-rc", "constqp"));
        assert!(has_arg(&args, "-cq"));
    }

    #[test]
    fn test_qsv_h264_crf_uses_global_quality() {
        let args = build_encode_args(&hw_crf(Codec::QsvH264), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-global_quality"));
    }

    #[test]
    fn test_qsv_h265_crf_uses_global_quality() {
        let args = build_encode_args(&hw_crf(Codec::QsvH265), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-global_quality"));
    }

    #[test]
    fn test_vt_h264_crf_uses_quality() {
        let args = build_encode_args(&hw_crf(Codec::VideoToolboxH264), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-quality"));
    }

    #[test]
    fn test_vt_h265_crf_uses_quality() {
        let args = build_encode_args(&hw_crf(Codec::VideoToolboxH265), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-quality"));
    }

    #[test]
    fn test_vaapi_h264_crf_uses_global_quality() {
        let args = build_encode_args(&hw_crf(Codec::VaapiH264), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-global_quality"));
    }

    #[test]
    fn test_vaapi_h265_crf_uses_global_quality() {
        let args = build_encode_args(&hw_crf(Codec::VaapiH265), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-global_quality"));
    }

    #[test]
    fn test_amf_h264_crf_uses_qp_and_usage() {
        let args = build_encode_args(&hw_crf(Codec::AmfH264), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-qp_i", "23"));
        assert!(has_pair(&args, "-qp_p", "25"));
        assert!(has_pair(&args, "-usage", "transcoding"));
    }

    #[test]
    fn test_amf_h265_crf_uses_qp_and_usage() {
        let args = build_encode_args(&hw_crf(Codec::AmfH265), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-qp_i", "23"));
        assert!(has_pair(&args, "-qp_p", "25"));
        assert!(has_pair(&args, "-usage", "transcoding"));
    }

    // ── Hardware encoder QP ──
    #[test]
    fn test_nvenc_h264_qp() {
        let args = build_encode_args(&hw_qp(Codec::NvencH264), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-qp", "23"));
    }

    #[test]
    fn test_qsv_h264_qp() {
        let args = build_encode_args(&hw_qp(Codec::QsvH264), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-qp", "23"));
    }

    #[test]
    fn test_vaapi_h264_qp() {
        let args = build_encode_args(&hw_qp(Codec::VaapiH264), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-qp", "23"));
    }

    #[test]
    fn test_amf_h264_qp() {
        let args = build_encode_args(&hw_qp(Codec::AmfH264), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-qp", "23"));
    }

    #[test]
    fn test_vt_qp_rejected() {
        let result = build_encode_args(&hw_qp(Codec::VideoToolboxH264), EncodePass::Single);
        assert!(result.is_err());
    }

    #[test]
    fn test_vt_h265_qp_rejected() {
        let result = build_encode_args(&hw_qp(Codec::VideoToolboxH265), EncodePass::Single);
        assert!(result.is_err());
    }

    // ── Hardware encoder capped CRF ──
    #[test]
    fn test_nvenc_capped_crf_sets_vbv() {
        let job = EncodeJob {
            codec: Codec::NvencH264,
            max_bitrate: 5000.0,
            bufsize: 10000.0,
            rate_control: RateControlMode::CappedCrf,
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        // Capped CRF must use VBR (not constqp) so the bitrate cap is honored.
        assert!(has_pair(&args, "-rc", "vbr"));
        assert!(has_pair(&args, "-maxrate", "5000k"));
        assert!(has_pair(&args, "-bufsize", "10000k"));
    }

    #[test]
    fn test_hw_capped_crf_max_bitrate_zero_errors() {
        let job = EncodeJob {
            codec: Codec::NvencH264,
            max_bitrate: 0.0,
            rate_control: RateControlMode::CappedCrf,
            ..sample_job(RateControlMode::Crf)
        };
        assert!(build_encode_args(&job, EncodePass::Single).is_err());
    }

    // ── Hardware encoder VBR ──
    #[test]
    fn test_nvenc_vbr_uses_vbr_hq() {
        let job = EncodeJob {
            codec: Codec::NvencH264,
            target_bitrate: 5000.0,
            rate_control: RateControlMode::Vbr,
            ..sample_job(RateControlMode::Vbr)
        };
        let passlog = Path::new("/tmp/plog");
        let args = build_encode_args(&job, EncodePass::Second(passlog)).unwrap();
        assert!(has_pair(&args, "-rc", "vbr_hq"));
    }

    #[test]
    fn test_qsv_vbr_no_special_rc() {
        let job = EncodeJob {
            codec: Codec::QsvH264,
            target_bitrate: 5000.0,
            rate_control: RateControlMode::Vbr,
            ..sample_job(RateControlMode::Vbr)
        };
        let passlog = Path::new("/tmp/plog");
        let args = build_encode_args(&job, EncodePass::Second(passlog)).unwrap();
        assert!(!has_arg(&args, "-rc"));
    }

    #[test]
    fn test_hw_vbr_target_bitrate_zero_errors() {
        let job = EncodeJob {
            codec: Codec::NvencH264,
            target_bitrate: 0.0,
            rate_control: RateControlMode::Vbr,
            ..sample_job(RateControlMode::Vbr)
        };
        let passlog = Path::new("/tmp/plog");
        assert!(build_encode_args(&job, EncodePass::Second(passlog)).is_err());
    }

    // ── Hardware preset mappings ──
    #[test]
    fn test_nvenc_preset_maps_to_p_numbers() {
        let job = EncodeJob {
            codec: Codec::NvencH264,
            preset: "veryfast".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-preset", "p2"));
    }

    #[test]
    fn test_vaapi_preset_uses_compression_level() {
        let job = EncodeJob {
            codec: Codec::VaapiH264,
            preset: "medium".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-compression_level", "3"));
    }

    #[test]
    fn test_amf_preset_uses_quality() {
        let job = EncodeJob {
            codec: Codec::AmfH264,
            preset: "slow".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-quality", "quality"));
    }

    #[test]
    fn test_amf_preset_speed() {
        let job = EncodeJob {
            codec: Codec::AmfH264,
            preset: "ultrafast".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-quality", "speed"));
    }

    // ── AV1 hardware encoders ──
    #[test]
    fn test_av1_hw_codecs_have_correct_codec_string() {
        for codec in &[Codec::NvencAv1, Codec::QsvAv1, Codec::VaapiAv1, Codec::AmfAv1] {
            let job = EncodeJob {
                codec: *codec,
                preset: String::new(),
                resolution: None,
                extra_args: vec![],
                ..sample_job(RateControlMode::Crf)
            };
            let args = build_encode_args(&job, EncodePass::Single).unwrap();
            assert!(has_pair(&args, "-c:v", codec.as_str()), "expected -c:v {}", codec.as_str());
        }
    }

    #[test]
    fn test_av1_nvenc_crf_uses_constqp() {
        let args = build_encode_args(&hw_crf(Codec::NvencAv1), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-rc", "constqp"));
        assert!(has_arg(&args, "-cq"));
    }

    #[test]
    fn test_av1_vaapi_crf_uses_global_quality() {
        let args = build_encode_args(&hw_crf(Codec::VaapiAv1), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-global_quality"));
    }

    // ── VAAPI device init + hwupload filter chain ──
    #[test]
    fn test_vaapi_sets_device_before_input() {
        let args = build_encode_args(&hw_crf(Codec::VaapiH264), EncodePass::Single).unwrap();
        let dev_idx =
            args.iter().position(|a| a == "-vaapi_device").expect("missing -vaapi_device");
        let i_idx = args.iter().position(|a| a == "-i").expect("missing -i");
        assert!(dev_idx < i_idx, "-vaapi_device must precede -i: {args:?}");
    }

    #[test]
    fn test_vaapi_filter_chain_has_hwupload() {
        // With a target resolution: scale then format+upload, in one -vf chain.
        let job = EncodeJob { codec: Codec::VaapiH264, ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        let vf_idx = args.iter().position(|a| a == "-vf").expect("missing -vf");
        let vf = &args[vf_idx + 1];
        assert!(vf.contains("scale=1280:720:flags=lanczos"), "missing scale: {vf}");
        assert!(vf.contains("format=nv12,hwupload"), "missing hwupload: {vf}");
        assert_eq!(args.iter().filter(|a| *a == "-vf").count(), 1, "exactly one -vf: {args:?}");
    }

    #[test]
    fn test_vaapi_hwupload_present_without_resolution() {
        let job = EncodeJob {
            codec: Codec::VaapiH264,
            resolution: None,
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        let vf_idx = args.iter().position(|a| a == "-vf").expect("missing -vf");
        assert_eq!(args[vf_idx + 1], "format=nv12,hwupload");
    }

    #[test]
    fn test_non_vaapi_has_no_hwupload_or_device() {
        let job = EncodeJob { codec: Codec::NvencH264, ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-vaapi_device"));
        let vf_idx = args.iter().position(|a| a == "-vf").expect("missing -vf");
        assert_eq!(args[vf_idx + 1], "scale=1280:720:flags=lanczos");
    }

    // ── Hardware decode (hwaccel) ──
    #[test]
    fn test_hwaccel_injected_before_input() {
        let job = EncodeJob {
            codec: Codec::X264,
            hwaccel: Some("cuda".into()),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        let acc_idx = args.iter().position(|a| a == "-hwaccel").expect("missing -hwaccel");
        let i_idx = args.iter().position(|a| a == "-i").expect("missing -i");
        assert_eq!(args[acc_idx + 1], "cuda");
        assert!(acc_idx < i_idx, "-hwaccel must precede -i: {args:?}");
    }

    #[test]
    fn test_no_hwaccel_when_unset_or_empty() {
        for hw in [None, Some(String::new())] {
            let job =
                EncodeJob { codec: Codec::X264, hwaccel: hw, ..sample_job(RateControlMode::Crf) };
            let args = build_encode_args(&job, EncodePass::Single).unwrap();
            assert!(!has_arg(&args, "-hwaccel"), "unexpected -hwaccel: {args:?}");
        }
    }

    #[test]
    fn test_amf_preset_balanced() {
        let job = EncodeJob {
            codec: Codec::AmfH264,
            preset: "fast".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-quality", "balanced"));
    }

    #[test]
    fn test_vt_preset_realtime_for_ultrafast() {
        let job = EncodeJob {
            codec: Codec::VideoToolboxH264,
            preset: "ultrafast".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-realtime", "1"));
    }

    #[test]
    fn test_vt_preset_realtime_for_veryfast() {
        let job = EncodeJob {
            codec: Codec::VideoToolboxH264,
            preset: "veryfast".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-realtime", "1"));
    }

    #[test]
    fn test_vt_preset_no_realtime_for_slow() {
        let job = EncodeJob {
            codec: Codec::VideoToolboxH264,
            preset: "slow".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(!has_arg(&args, "-realtime"));
    }

    #[test]
    fn test_qsv_preset_passthrough() {
        let job = EncodeJob {
            codec: Codec::QsvH264,
            preset: "medium".into(),
            ..sample_job(RateControlMode::Crf)
        };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-preset", "medium"));
    }

    // ── CRF-to-HW quality conversion ──
    #[test]
    fn test_crf_to_nvenc_cq_bounds() {
        assert_eq!(crf_to_nvenc_cq(0), 1); // clamped to 1
        assert_eq!(crf_to_nvenc_cq(51), 41); // (51*51)/63 ≈ 41
        assert_eq!(crf_to_nvenc_cq(63), 51); // (63*51)/63 = 51
        assert_eq!(crf_to_nvenc_cq(100), 51); // clamped to 51
    }

    #[test]
    fn test_crf_to_nvenc_cq_typical_values() {
        assert_eq!(crf_to_nvenc_cq(23), 18); // (23*51)/63 ≈ 18.6 → 18
        assert_eq!(crf_to_nvenc_cq(30), 24); // (30*51)/63 ≈ 24.2 → 24
    }

    #[test]
    fn test_crf_to_qsv_quality_bounds() {
        let q0 = crf_to_qsv_quality(0);
        assert!((95..=100).contains(&q0)); // 100 - (0*100)/51 = 100
        let q51 = crf_to_qsv_quality(51);
        assert_eq!(q51, 1); // clamped to 1
        let q100 = crf_to_qsv_quality(100);
        assert_eq!(q100, 1); // clamped at bottom
    }

    #[test]
    fn test_crf_to_qsv_quality_mid() {
        let q = crf_to_qsv_quality(25);
        // 100 - (25*100)/51 ≈ 100 - 49 = 51
        assert!((50..=52).contains(&q));
    }

    #[test]
    fn test_crf_to_vt_quality_bounds() {
        assert!((crf_to_vt_quality(0) - 1.0).abs() < 1e-9);
        assert!((crf_to_vt_quality(51) - 0.0).abs() < 1e-9);
        assert!((crf_to_vt_quality(100) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_crf_to_vt_quality_mid() {
        let q = crf_to_vt_quality(25);
        assert!(q > 0.4 && q < 0.6);
    }

    // ── Specific CRF value edge cases ──
    #[test]
    fn test_crf_zero() {
        let job = EncodeJob { crf: 0, ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-crf", "0"));
    }

    #[test]
    fn test_crf_high_value() {
        let job = EncodeJob { crf: 51, ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-crf", "51"));
    }

    #[test]
    fn test_crf_negative_allowed() {
        let job = EncodeJob { crf: -1, ..sample_job(RateControlMode::Crf) };
        let args = build_encode_args(&job, EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-crf", "-1"));
    }

    #[test]
    fn test_input_arg_is_present() {
        let args =
            build_encode_args(&sample_job(RateControlMode::Crf), EncodePass::Single).unwrap();
        assert!(has_pair(&args, "-i", "input.mp4"));
    }

    #[test]
    fn test_no_audio_flag_is_present() {
        let args =
            build_encode_args(&sample_job(RateControlMode::Crf), EncodePass::Single).unwrap();
        assert!(has_arg(&args, "-an"));
    }

    // ── All software codecs + modes use correct codec string ──
    #[test]
    fn test_all_sw_codecs_have_correct_codec_string() {
        for codec in &[Codec::X264, Codec::X265, Codec::SvtAv1] {
            let job = EncodeJob {
                codec: *codec,
                preset: String::new(),
                resolution: None,
                extra_args: vec![],
                ..sample_job(RateControlMode::Crf)
            };
            let args = build_encode_args(&job, EncodePass::Single).unwrap();
            assert!(has_pair(&args, "-c:v", codec.as_str()), "expected -c:v {}", codec.as_str());
        }
    }

    #[test]
    fn test_all_hw_codecs_have_correct_codec_string() {
        for codec in &[
            Codec::NvencH264,
            Codec::NvencH265,
            Codec::QsvH264,
            Codec::QsvH265,
            Codec::VideoToolboxH264,
            Codec::VideoToolboxH265,
            Codec::VaapiH264,
            Codec::VaapiH265,
            Codec::AmfH264,
            Codec::AmfH265,
        ] {
            let job = EncodeJob {
                codec: *codec,
                preset: String::new(),
                resolution: None,
                extra_args: vec![],
                ..sample_job(RateControlMode::Crf)
            };
            let args = build_encode_args(&job, EncodePass::Single).unwrap();
            assert!(has_pair(&args, "-c:v", codec.as_str()), "expected -c:v {}", codec.as_str());
        }
    }

    // ── Property-based: verify against FFmpeg encoder documentation ──
    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_codec() -> impl Strategy<Value = Codec> {
            prop_oneof![
                Just(Codec::X264),
                Just(Codec::X265),
                Just(Codec::SvtAv1),
                Just(Codec::NvencH264),
                Just(Codec::NvencH265),
                Just(Codec::QsvH264),
                Just(Codec::QsvH265),
                Just(Codec::VideoToolboxH264),
                Just(Codec::VideoToolboxH265),
                Just(Codec::VaapiH264),
                Just(Codec::VaapiH265),
                Just(Codec::AmfH264),
                Just(Codec::AmfH265),
                Just(Codec::NvencAv1),
                Just(Codec::QsvAv1),
                Just(Codec::VaapiAv1),
                Just(Codec::AmfAv1),
            ]
        }

        fn arb_rate_control() -> impl Strategy<Value = RateControlMode> {
            prop_oneof![
                Just(RateControlMode::Crf),
                Just(RateControlMode::Qp),
                Just(RateControlMode::CappedCrf),
            ]
        }

        fn arb_encode_job() -> impl Strategy<Value = EncodeJob> {
            (
                arb_codec(),
                arb_rate_control(),
                any::<i32>(),
                any::<f64>(),
                any::<f64>(),
                any::<f64>(),
                any::<String>(),
            )
                .prop_map(|(codec, rc, crf, target_br, max_br, bufsize, preset)| {
                    let crf = crf.abs().min(63);
                    EncodeJob {
                        input: "input.mp4".into(),
                        output: "output.mp4".into(),
                        resolution: Some(Resolution::new(1920, 1080)),
                        codec,
                        crf,
                        rate_control: rc,
                        target_bitrate: target_br.abs().min(100000.0),
                        max_bitrate: max_br.abs().min(100000.0),
                        bufsize: bufsize.abs().min(200000.0),
                        preset,
                        hwaccel: None,
                        extra_args: vec![],
                    }
                })
        }

        proptest! {
            /// Invariant: every arg list starts with -y, and the input file is
            /// named immediately after a `-i` flag. (Input-level options such as
            /// `-hwaccel` or `-vaapi_device` may sit between `-y` and `-i`.)
            #[test]
            fn args_start_with_overwrite_and_input(job in arb_encode_job()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    assert!(args.len() >= 3, "too few args: {args:?}");
                    assert_eq!(args[0], "-y", "first arg must be -y");
                    let i_idx = args.iter().position(|a| a == "-i").expect("must contain -i");
                    assert_eq!(args[i_idx + 1], "input.mp4", "input path must follow -i");
                }
            }

            /// Invariant: -an (no audio) present in single pass.
            #[test]
            fn args_have_no_audio_flag(job in arb_encode_job()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    assert!(has_arg(&args, "-an"),
                        "missing -an: {args:?}");
                }
            }

            /// Invariant: -c:v <codec> present and matches the job codec.
            #[test]
            fn args_have_correct_codec(job in arb_encode_job()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    assert!(has_pair(&args, "-c:v", job.codec.as_str()),
                        "missing or wrong -c:v: {args:?}, expected {}", job.codec.as_str());
                }
            }

            /// Invariant: the output path is the final argument.
            #[test]
            fn output_is_the_last_argument(job in arb_encode_job()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    assert_eq!(args.last().unwrap(), "output.mp4",
                        "output not last: {args:?}");
                }
            }

            /// Invariant: no duplicate flag keys (e.g. two -crf, two -preset).
            /// FFmpeg uses the last value for duplicate flags, which is a common source of bugs.
            #[test]
            fn no_duplicate_flag_keys(job in arb_encode_job()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    let mut seen = std::collections::HashSet::new();
                    for arg_chunk in args.chunks(2) {
                        if arg_chunk[0].starts_with('-') {
                            assert!(seen.insert(&arg_chunk[0]),
                                "duplicate flag {} in {args:?}", arg_chunk[0]);
                        }
                    }
                }
            }

            /// Invariant: for software codecs with CRF mode, -crf <value> present.
            #[test]
            fn sw_crf_has_crf_flag(
                crf in 0i32..63i32,
                preset in ".*",
            ) {
                for codec in &[Codec::X264, Codec::X265, Codec::SvtAv1] {
                    let job = EncodeJob {
                        codec: *codec, crf, rate_control: RateControlMode::Crf,
                        preset: preset.clone(), resolution: None, extra_args: vec![],
                        ..sample_job(RateControlMode::Crf)
                    };
                    let args = build_encode_args(&job, EncodePass::Single).unwrap();
                    assert!(has_pair(&args, "-crf", &crf.to_string()),
                        "{codec:?}: missing -crf {crf} in {args:?}");
                }
            }

            /// Invariant: CRF and QP are mutually exclusive for software codecs.
            #[test]
            fn sw_crf_and_qp_never_both_present(
                crf in 0i32..63i32,
                mode in prop_oneof![Just(RateControlMode::Crf), Just(RateControlMode::Qp)],
            ) {
                for codec in &[Codec::X264, Codec::X265] {
                    let job = EncodeJob {
                        codec: *codec, crf, rate_control: mode, preset: String::new(),
                        resolution: None, extra_args: vec![],
                        ..sample_job(mode)
                    };
                    if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                        let has_crf = has_arg(&args, "-crf");
                        let has_qp = has_arg(&args, "-qp");
                        assert!(!(has_crf && has_qp),
                            "{codec:?} mode={mode:?}: both -crf and -qp present: {args:?}");
                    }
                }
            }

            /// Invariant: for capped CRF, both -maxrate and -bufsize present with 'k' suffix.
            #[test]
            fn capped_crf_has_rate_control_args(job in arb_encode_job_sw_capped()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    // Find -maxrate argument
                    let maxrate_idx = args.iter().position(|a| a == "-maxrate");
                    if let Some(idx) = maxrate_idx {
                        let val = &args[idx + 1];
                        assert!(val.ends_with('k'),
                            "-maxrate value should end with 'k': {val}");
                    }
                    let bufsize_idx = args.iter().position(|a| a == "-bufsize");
                    if let Some(idx) = bufsize_idx {
                        let val = &args[idx + 1];
                        assert!(val.ends_with('k'),
                            "-bufsize value should end with 'k': {val}");
                    }
                }
            }

            /// Invariant: first-pass VBR has no progress flags, writes to null output.
            #[test]
            fn vbr_first_pass_has_null_output(
                job in arb_encode_job_sw_vbr(),
            ) {
                let passlog = Path::new("/tmp/plog");
                if let Ok(args) = build_encode_args(&job, EncodePass::First(passlog)) {
                    assert!(!has_arg(&args, "-progress"),
                        "first pass should not have -progress: {args:?}");
                    assert!(has_pair(&args, "-f", "null"),
                        "first pass must write to null: {args:?}");
                }
            }

            /// Invariant: SVT-AV1 QP mode includes enable-adaptive-quantization=0.
            #[test]
            fn svtav1_qp_disables_aq(
                crf in 1i32..63i32,
                preset in ".*",
            ) {
                let job = EncodeJob {
                    codec: Codec::SvtAv1, crf, rate_control: RateControlMode::Qp,
                    preset, resolution: None, extra_args: vec![],
                    ..sample_job(RateControlMode::Qp)
                };
                let args = build_encode_args(&job, EncodePass::Single).unwrap();
                assert!(has_pair(&args, "-svtav1-params", "enable-adaptive-quantization=0"),
                    "SVT-AV1 QP must disable adaptive quantization: {args:?}");
            }

            /// Invariant: NVENC CRF uses -rc constqp + -cq, never -crf.
            #[test]
            fn nvenc_crf_uses_cq_not_crf(
                crf in 0i32..63i32,
                h264_h265 in prop_oneof![Just(Codec::NvencH264), Just(Codec::NvencH265)],
            ) {
                let job = EncodeJob {
                    codec: h264_h265, crf, rate_control: RateControlMode::Crf,
                    preset: String::new(), resolution: None, extra_args: vec![],
                    ..sample_job(RateControlMode::Crf)
                };
                let args = build_encode_args(&job, EncodePass::Single).unwrap();
                assert!(has_pair(&args, "-rc", "constqp"),
                    "NVENC CRF missing -rc constqp: {args:?}");
                assert!(has_arg(&args, "-cq"),
                    "NVENC CRF missing -cq: {args:?}");
                assert!(!has_arg(&args, "-crf"),
                    "NVENC must not use -crf: {args:?}");
            }

            /// Invariant: QSV CRF uses -global_quality, never -crf.
            #[test]
            fn qsv_crf_uses_global_quality(
                crf in 0i32..63i32,
                h264_h265 in prop_oneof![Just(Codec::QsvH264), Just(Codec::QsvH265)],
            ) {
                let job = EncodeJob {
                    codec: h264_h265, crf, rate_control: RateControlMode::Crf,
                    preset: String::new(), resolution: None, extra_args: vec![],
                    ..sample_job(RateControlMode::Crf)
                };
                let args = build_encode_args(&job, EncodePass::Single).unwrap();
                assert!(has_arg(&args, "-global_quality"),
                    "QSV CRF missing -global_quality: {args:?}");
                assert!(!has_arg(&args, "-crf"),
                    "QSV must not use -crf: {args:?}");
            }

            /// Invariant: AMF CRF uses -qp_i -qp_p -usage transcoding, never -crf.
            #[test]
            fn amf_crf_uses_qp_pairs(
                crf in 0i32..63i32,
                h264_h265 in prop_oneof![Just(Codec::AmfH264), Just(Codec::AmfH265)],
            ) {
                let job = EncodeJob {
                    codec: h264_h265, crf, rate_control: RateControlMode::Crf,
                    preset: String::new(), resolution: None, extra_args: vec![],
                    ..sample_job(RateControlMode::Crf)
                };
                let args = build_encode_args(&job, EncodePass::Single).unwrap();
                assert!(has_pair(&args, "-qp_i", &crf.to_string()),
                    "AMF missing -qp_i: {args:?}");
                assert!(!has_arg(&args, "-crf"),
                    "AMF must not use -crf: {args:?}");
            }

            /// Invariant: VideoToolbox QP is rejected (not supported).
            #[test]
            fn videotoolbox_qp_always_rejected(
                crf in 0i32..63i32,
                h264_h265 in prop_oneof![Just(Codec::VideoToolboxH264), Just(Codec::VideoToolboxH265)],
            ) {
                let job = EncodeJob {
                    codec: h264_h265, crf, rate_control: RateControlMode::Qp,
                    preset: String::new(), resolution: None, extra_args: vec![],
                    ..sample_job(RateControlMode::Qp)
                };
                assert!(build_encode_args(&job, EncodePass::Single).is_err(),
                    "VideoToolbox QP should be rejected");
            }

            /// Invariant: VBR single-pass always errors for software codecs
            /// (hardware encoders support single-pass VBR natively).
            #[test]
            fn vbr_single_pass_errors_for_sw_codecs(
                target_br in 100.0f64..100000.0f64,
            ) {
                for codec in &[Codec::X264, Codec::X265, Codec::SvtAv1] {
                    let job = EncodeJob {
                        codec: *codec, rate_control: RateControlMode::Vbr,
                        target_bitrate: target_br,
                        ..sample_job(RateControlMode::Vbr)
                    };
                    assert!(build_encode_args(&job, EncodePass::Single).is_err(),
                        "{codec:?} VBR single-pass should error");
                }
            }

            /// Invariant: hardware VBR single-pass is valid (sets bitrate args without passlog).
            #[test]
            fn hw_vbr_single_pass_is_valid(
                target_br in 100.0f64..100000.0f64,
                codec in prop_oneof![
                    Just(Codec::NvencH264), Just(Codec::QsvH264),
                    Just(Codec::VideoToolboxH264), Just(Codec::VaapiH264), Just(Codec::AmfH264),
                ],
            ) {
                let job = EncodeJob {
                    codec, rate_control: RateControlMode::Vbr,
                    target_bitrate: target_br,
                    resolution: None, preset: String::new(), extra_args: vec![],
                    ..sample_job(RateControlMode::Vbr)
                };
                let args = build_encode_args(&job, EncodePass::Single).unwrap();
                assert!(has_pair(&args, "-b:v", &format!("{target_br:.0}k")),
                    "HW VBR single-pass missing -b:v: {args:?}");
            }

            /// Invariant: for any valid single-pass job, output is a single file path (not null).
            #[test]
            fn single_pass_output_is_file(job in arb_encode_job()) {
                if let Ok(args) = build_encode_args(&job, EncodePass::Single) {
                    let last = args.last().unwrap();
                    assert!(!last.starts_with('-'),
                        "last arg should not be a flag: {last}");
                    assert!(!last.is_empty(),
                        "last arg should not be empty");
                }
            }
        }

        fn arb_encode_job_sw_capped() -> impl Strategy<Value = EncodeJob> {
            (any::<i32>(), any::<f64>(), any::<f64>(), any::<String>()).prop_map(
                |(crf, max_br, bufsize, preset)| {
                    let crf = crf.abs().min(63);
                    let max_br = max_br.abs().clamp(100.0, 100000.0);
                    EncodeJob {
                        codec: Codec::X264,
                        crf,
                        rate_control: RateControlMode::CappedCrf,
                        max_bitrate: max_br,
                        bufsize: bufsize.abs().min(200000.0),
                        preset,
                        resolution: None,
                        extra_args: vec![],
                        ..sample_job(RateControlMode::CappedCrf)
                    }
                },
            )
        }

        fn arb_encode_job_sw_vbr() -> impl Strategy<Value = EncodeJob> {
            (any::<i32>(), any::<f64>(), any::<String>()).prop_map(|(crf, target_br, preset)| {
                let crf = crf.abs().min(63);
                let target_br = target_br.abs().clamp(100.0, 100000.0);
                EncodeJob {
                    codec: Codec::X264,
                    crf,
                    rate_control: RateControlMode::Vbr,
                    target_bitrate: target_br,
                    preset,
                    resolution: None,
                    extra_args: vec![],
                    ..sample_job(RateControlMode::Vbr)
                }
            })
        }
    }
}
