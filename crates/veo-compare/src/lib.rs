use serde::{Deserialize, Serialize};
use std::path::Path;

/// Options for the comparison player.
#[derive(Debug, Clone)]
pub struct Opts {
    pub reference: String,
    pub encoded: String,
    pub vmaf_data: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameVmaf {
    pub frame: i32,
    pub time: f64,
    pub vmaf: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dip {
    pub frame: i32,
    pub time: f64,
    pub vmaf: f64,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerData {
    pub reference_url: String,
    pub encoded_url: String,
    pub frames: Vec<FrameVmaf>,
    pub dips: Vec<Dip>,
    pub avg_vmaf: f64,
    pub min_vmaf: f64,
    pub max_vmaf: f64,
}

const PLAYER_HTML: &str = include_str!("player.html");

/// Loads per-frame VMAF data from a JSON file.
pub fn load_vmaf_data(path: &str) -> anyhow::Result<Vec<FrameVmaf>> {
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

    if let Ok(result) = serde_json::from_slice::<QualResult>(&data) {
        if !result.frames.is_empty() {
            return Ok(result.frames.into_iter().map(|f| FrameVmaf {
                frame: f.frame_num,
                time: f.frame_num as f64 / 24.0,
                vmaf: f.vmaf,
            }).collect());
        }
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
    Ok(raw.frames.into_iter().map(|f| FrameVmaf {
        frame: f.frame_num,
        time: f.frame_num as f64 / 24.0,
        vmaf: f.metrics.get("vmaf").copied().unwrap_or(0.0),
    }).collect())
}

/// Identifies quality dips in per-frame VMAF data.
pub fn find_dips(frames: &[FrameVmaf], warning_threshold: f64, critical_threshold: f64) -> Vec<Dip> {
    if frames.is_empty() { return vec![]; }

    let avg: f64 = frames.iter().map(|f| f.vmaf).sum::<f64>() / frames.len() as f64;

    let mut dips: Vec<Dip> = frames.iter().filter_map(|f| {
        if f.vmaf < avg - critical_threshold {
            Some(Dip { frame: f.frame, time: f.time, vmaf: f.vmaf, severity: "critical".into() })
        } else if f.vmaf < avg - warning_threshold {
            Some(Dip { frame: f.frame, time: f.time, vmaf: f.vmaf, severity: "warning".into() })
        } else {
            None
        }
    }).collect();

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

    if !opts.vmaf_data.is_empty() {
        if let Ok(frames) = load_vmaf_data(&opts.vmaf_data) {
            player_data.dips = find_dips(&frames, 5.0, 10.0);
            if !frames.is_empty() {
                let sum: f64 = frames.iter().map(|f| f.vmaf).sum();
                player_data.avg_vmaf = sum / frames.len() as f64;
                player_data.min_vmaf = frames.iter().map(|f| f.vmaf).fold(100.0, f64::min);
                player_data.max_vmaf = frames.iter().map(|f| f.vmaf).fold(0.0, f64::max);
            }
            player_data.frames = frames;
        }
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
    println!("  Reference: {}", Path::new(&ref_path).file_name().unwrap_or_default().to_string_lossy());
    println!("  Encoded:   {}", Path::new(&enc_path).file_name().unwrap_or_default().to_string_lossy());
    if !opts.vmaf_data.is_empty() {
        println!("  VMAF data: {}", Path::new(&opts.vmaf_data).file_name().unwrap_or_default().to_string_lossy());
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
        if line.trim().is_empty() { break; }
    }

    let stream = reader.into_inner();

    match path.as_str() {
        "/" => {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                PLAYER_HTML.len(), PLAYER_HTML
            );
            stream.write_all(response.as_bytes()).await?;
        }
        "/api/data" => {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                data_json.len(), data_json
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
    let content_type = if path.ends_with(".mp4") { "video/mp4" }
        else if path.ends_with(".mkv") { "video/x-matroska" }
        else { "application/octet-stream" };

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
