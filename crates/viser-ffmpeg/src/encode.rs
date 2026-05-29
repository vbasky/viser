use std::time::{Duration, Instant};
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
    let args = build_encode_args(&job);

    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(&args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());

    let start = Instant::now();
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

    let elapsed = start.elapsed();

    // Probe the output to get actual bitrate and file size
    let meta = std::fs::metadata(&job.output)
        .map_err(|e| anyhow::anyhow!("failed to stat output: {e}"))?;

    let probe_result = probe(&job.output).await?;
    let bitrate = probe_result.format.bit_rate as f64 / 1000.0;

    Ok(EncodeResult { job, bitrate, file_size: meta.len(), duration: elapsed })
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

fn build_encode_args(job: &EncodeJob) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        "-i".into(),
        job.input.clone(),
        "-an".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];

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
        RateControlMode::Vbr => {
            args.extend(["-b:v".into(), format!("{:.0}k", job.target_bitrate)]);
            args.extend(["-maxrate".into(), format!("{:.0}k", job.target_bitrate * 2.0)]);
            args.extend(["-bufsize".into(), format!("{:.0}k", job.target_bitrate * 4.0)]);
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
    args.push(job.output.clone());

    args
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
