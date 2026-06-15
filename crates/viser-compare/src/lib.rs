//! Browser-based side-by-side comparison player with a VMAF quality timeline.
//!
//! Part of the `viser` video-encoding-optimizer workspace. Serves an interactive
//! QA page for visual inspection of encoded videos, streaming the reference and
//! encoded files over HTTP alongside per-frame VMAF data and detected quality dips.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Options for the comparison player.
#[derive(Debug, Clone)]
pub struct Opts {
    /// Path to the reference (original) video file.
    pub reference: String,
    /// Path to the encoded video file.
    pub encoded: String,
    /// Path to the per-frame VMAF JSON file (empty to disable the timeline).
    pub vmaf_data: String,
    /// Preferred HTTP port; falls back to an OS-assigned port if unavailable.
    pub port: u16,
}

/// A single per-frame VMAF score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameVmaf {
    /// Zero-based frame index.
    pub frame: i32,
    /// Presentation time of the frame in seconds.
    pub time: f64,
    /// VMAF score for the frame (0-100).
    pub vmaf: f64,
}

/// A detected quality dip — a frame whose VMAF falls notably below the average.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dip {
    /// Zero-based frame index of the dip.
    pub frame: i32,
    /// Presentation time of the dip in seconds.
    pub time: f64,
    /// VMAF score at the dip.
    pub vmaf: f64,
    /// Severity classification: `"warning"` or `"critical"`.
    pub severity: String,
}

/// Payload sent to the browser player describing the videos and quality data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerData {
    /// URL the player fetches the reference video from.
    pub reference_url: String,
    /// URL the player fetches the encoded video from.
    pub encoded_url: String,
    /// Per-frame VMAF scores backing the quality timeline.
    pub frames: Vec<FrameVmaf>,
    /// Detected quality dips, capped to the most severe entries.
    pub dips: Vec<Dip>,
    /// Average VMAF across all frames.
    pub avg_vmaf: f64,
    /// Minimum VMAF across all frames.
    pub min_vmaf: f64,
    /// Maximum VMAF across all frames.
    pub max_vmaf: f64,
}

const PLAYER_HTML: &str = include_str!("player.html");

/// Loads per-frame VMAF data from a JSON file.
///
/// `fps` is the source frame rate, used to convert frame indices to presentation
/// timestamps; values <= 0 fall back to 24 fps.
pub fn load_vmaf_data(path: &str, fps: f64) -> anyhow::Result<Vec<FrameVmaf>> {
    let fps = if fps > 0.0 { fps } else { 24.0 };
    let data = std::fs::read(path)?;

    // Try quality.Result format
    #[derive(Deserialize)]
    struct QualResult {
        #[serde(default)]
        frames: Vec<QualFrame>,
    }
    #[derive(Deserialize)]
    struct QualFrame {
        #[serde(rename = "frameNum", alias = "frame_num")]
        frame_num: i32,
        #[serde(default)]
        vmaf: f64,
    }

    if let Ok(result) = serde_json::from_slice::<QualResult>(&data)
        && !result.frames.is_empty() {
            return Ok(result
                .frames
                .into_iter()
                .map(|f| FrameVmaf {
                    frame: f.frame_num,
                    time: f.frame_num as f64 / fps,
                    vmaf: f.vmaf,
                })
                .collect());
        }

    // Try libvmaf raw format
    #[derive(Deserialize)]
    struct VmafRaw {
        frames: Vec<VmafRawFrame>,
    }
    #[derive(Deserialize)]
    struct VmafRawFrame {
        #[serde(rename = "frameNum")]
        frame_num: i32,
        metrics: std::collections::HashMap<String, f64>,
    }

    let raw: VmafRaw = serde_json::from_slice(&data)?;
    Ok(raw
        .frames
        .into_iter()
        .map(|f| FrameVmaf {
            frame: f.frame_num,
            time: f.frame_num as f64 / fps,
            vmaf: f.metrics.get("vmaf").copied().unwrap_or(0.0),
        })
        .collect())
}

/// Identifies quality dips in per-frame VMAF data.
pub fn find_dips(
    frames: &[FrameVmaf],
    warning_threshold: f64,
    critical_threshold: f64,
) -> Vec<Dip> {
    if frames.is_empty() {
        return vec![];
    }

    let avg: f64 = frames.iter().map(|f| f.vmaf).sum::<f64>() / frames.len() as f64;

    let mut dips: Vec<Dip> = frames
        .iter()
        .filter_map(|f| {
            if f.vmaf < avg - critical_threshold {
                Some(Dip {
                    frame: f.frame,
                    time: f.time,
                    vmaf: f.vmaf,
                    severity: "critical".into(),
                })
            } else if f.vmaf < avg - warning_threshold {
                Some(Dip { frame: f.frame, time: f.time, vmaf: f.vmaf, severity: "warning".into() })
            } else {
                None
            }
        })
        .collect();

    if dips.len() > 50 {
        dips.sort_by(|a, b| a.vmaf.partial_cmp(&b.vmaf).unwrap());
        dips.truncate(50);
    }

    dips
}

/// Starts the comparison player HTTP server and opens the browser.
pub async fn serve(opts: Opts) -> anyhow::Result<()> {
    let mut player_data = PlayerData {
        reference_url: "/video/reference".into(),
        encoded_url: "/video/encoded".into(),
        frames: vec![],
        dips: vec![],
        avg_vmaf: 0.0,
        min_vmaf: 0.0,
        max_vmaf: 0.0,
    };

    // Probe the reference for its real frame rate so dip timestamps line up with
    // the video timeline regardless of fps (24/25/30/50/60...).
    let fps = match viser_ffmpeg::probe(&opts.reference).await {
        Ok(p) => p.streams.iter().find(|s| s.codec_type == "video").map(|s| s.fps()).unwrap_or(0.0),
        Err(_) => 0.0,
    };

    if !opts.vmaf_data.is_empty()
        && let Ok(frames) = load_vmaf_data(&opts.vmaf_data, fps) {
            player_data.dips = find_dips(&frames, 5.0, 10.0);
            if !frames.is_empty() {
                let sum: f64 = frames.iter().map(|f| f.vmaf).sum();
                player_data.avg_vmaf = sum / frames.len() as f64;
                player_data.min_vmaf = frames.iter().map(|f| f.vmaf).fold(100.0, f64::min);
                player_data.max_vmaf = frames.iter().map(|f| f.vmaf).fold(0.0, f64::max);
            }
            player_data.frames = frames;
        }

    let port = if opts.port == 0 { 8787 } else { opts.port };
    let ref_path = opts.reference.clone();
    let enc_path = opts.encoded.clone();
    let data_json = serde_json::to_string(&player_data)?;

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(_) => tokio::net::TcpListener::bind("0.0.0.0:0").await?,
    };

    let actual_port = listener.local_addr()?.port();
    let url = format!("http://localhost:{actual_port}");

    println!("Comparison player: {url}");
    println!(
        "  Reference: {}",
        Path::new(&ref_path).file_name().unwrap_or_default().to_string_lossy()
    );
    println!(
        "  Encoded:   {}",
        Path::new(&enc_path).file_name().unwrap_or_default().to_string_lossy()
    );
    if !opts.vmaf_data.is_empty() {
        println!(
            "  VMAF data: {}",
            Path::new(&opts.vmaf_data).file_name().unwrap_or_default().to_string_lossy()
        );
        println!("  Dips:      {} quality dips detected", player_data.dips.len());
    }
    println!("\nPress Ctrl+C to stop");

    open_browser(&url);

    loop {
        let (stream, _) = listener.accept().await?;
        let ref_path = ref_path.clone();
        let enc_path = enc_path.clone();
        let data_json = data_json.clone();

        tokio::spawn(async move {
            let _ = handle_connection(stream, &ref_path, &enc_path, &data_json).await;
        });
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    ref_path: &str,
    enc_path: &str,
    data_json: &str,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();

    // Consume remaining headers
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
    }

    let stream = reader.into_inner();

    match path.as_str() {
        "/" => {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                PLAYER_HTML.len(),
                PLAYER_HTML
            );
            stream.write_all(response.as_bytes()).await?;
        }
        "/api/data" => {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                data_json.len(),
                data_json
            );
            stream.write_all(response.as_bytes()).await?;
        }
        "/video/reference" => serve_file(stream, ref_path).await?,
        "/video/encoded" => serve_file(stream, enc_path).await?,
        _ => {
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            stream.write_all(response.as_bytes()).await?;
        }
    }

    Ok(())
}

async fn serve_file(stream: &mut tokio::net::TcpStream, path: &str) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    let data = std::fs::read(path)?;
    let content_type = if path.ends_with(".mp4") {
        "video/mp4"
    } else if path.ends_with(".mkv") {
        "video/x-matroska"
    } else {
        "application/octet-stream"
    };

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        data.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&data).await?;
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", url]).spawn();
}
