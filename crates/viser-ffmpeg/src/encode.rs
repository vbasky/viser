use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::{Codec, RateControlMode, Resolution, ffmpeg_path, probe};

/// Parameters for a single encode.
#[derive(Debug, Clone)]
pub struct EncodeJob {
    pub input: String,
    pub output: String,
    pub resolution: Option<Resolution>,
    pub codec: Codec,
    pub crf: i32,
    pub rate_control: RateControlMode,
    pub target_bitrate: f64, // kbps, used for VBR mode
    pub max_bitrate: f64,    // kbps, used for capped CRF mode
    pub bufsize: f64,        // kbps, used for capped CRF mode
    pub preset: String,
    pub extra_args: Vec<String>,
}

/// Output of a completed encode.
#[derive(Debug, Clone)]
pub struct EncodeResult {
    pub job: EncodeJob,
    pub bitrate: f64,       // kbps (average)
    pub file_size: u64,     // bytes
    pub duration: Duration, // wall-clock encode time
}

/// Real-time encoding progress info parsed from FFmpeg.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub frame: i64,
    pub fps: f64,
    pub bitrate: f64, // kbps
    pub speed: f64,   // e.g. 2.5x
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
                if parse_progress_line(&line, &mut p) {
                    if let Some(ref tx) = tx {
                        let _ = tx.try_send(p.clone());
                    }
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
        .map(|path| format!("file '{}'", path.replace('\'', "'\\''")))
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
    let mut args = vec!["-y".into(), "-i".into(), job.input.clone(), "-an".into()];

    if !matches!(pass, EncodePass::First(_)) {
        args.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    }

    args.extend(["-c:v".into(), job.codec.as_str().into()]);

    // Rate control mode
    match job.rate_control {
        RateControlMode::Qp => match job.codec {
            Codec::SvtAv1 => {
                args.extend(["-qp".into(), job.crf.to_string()]);
                args.extend(["-svtav1-params".into(), "enable-adaptive-quantization=0".into()]);
            }
            _ => {
                args.extend(["-qp".into(), job.crf.to_string()]);
            }
        },
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

    if !job.preset.is_empty() {
        args.extend(["-preset".into(), job.preset.clone()]);
    }

    if let Some(ref res) = job.resolution {
        if res.width > 0 && res.height > 0 {
            args.extend([
                "-vf".into(),
                format!("scale={}:{}:flags=lanczos", res.width, res.height),
            ]);
        }
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
            extra_args: vec![],
        }
    }

    #[test]
    fn test_build_encode_args_crf_single_pass() {
        let args =
            build_encode_args(&sample_job(RateControlMode::Crf), EncodePass::Single).unwrap();
        assert!(args.windows(2).any(|w| w == ["-crf", "23"]));
        assert_eq!(args.last().unwrap(), "output.mp4");
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
    fn test_build_encode_args_capped_crf_sets_vbv() {
        let args =
            build_encode_args(&sample_job(RateControlMode::CappedCrf), EncodePass::Single).unwrap();
        assert!(args.windows(2).any(|w| w == ["-crf", "23"]));
        assert!(args.windows(2).any(|w| w == ["-maxrate", "3000k"]));
        assert!(args.windows(2).any(|w| w == ["-bufsize", "6000k"]));
    }
}
