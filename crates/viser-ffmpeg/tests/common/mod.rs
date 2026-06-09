use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns `true` if ffmpeg and ffprobe are on PATH.
pub fn has_ffmpeg() -> bool {
    for bin in &["ffmpeg", "ffprobe"] {
        let status = Command::new(bin)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => return false,
        }
    }
    true
}

/// Generate a synthetic test video clip using ffmpeg's lavfi source.
/// Returns the path to the generated file.
pub fn generate_test_clip(
    dir: &Path,
    name: &str,
    size: &str,
    duration_secs: u32,
    fps: u32,
    codec: &str,
) -> PathBuf {
    let path = dir.join(name);
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={duration_secs}:size={size}:rate={fps}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={duration_secs}"),
            "-c:v",
            codec,
            "-c:a",
            "aac",
            "-shortest",
            "-t",
            &duration_secs.to_string(),
        ])
        .arg(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to spawn ffmpeg");

    assert!(status.success(), "ffmpeg failed to generate {name}");
    assert!(path.exists(), "output file {name} not created");
    path
}

/// Generate an HDR test clip (HEVC with PQ transfer, MP4 container).
/// MP4 reliably exposes color metadata to ffprobe, unlike Matroska.
pub fn generate_hdr_clip(dir: &Path) -> PathBuf {
    let path = dir.join("hdr_test.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "smptehdbars=size=1920x1080:rate=30",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-x265-params",
            "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
            "-pix_fmt",
            "yuv420p10le",
            "-colorspace",
            "bt2020nc",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            "smpte2084",
            "-t",
            "2",
        ])
        .arg(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to spawn ffmpeg");

    assert!(status.success(), "ffmpeg failed to generate HDR clip");
    path
}

/// Generate a reference clip using lossless x264.
pub fn generate_reference_clip(dir: &Path, name: &str, size: &str, duration_secs: u32) -> PathBuf {
    let path = dir.join(name);
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={duration_secs}:size={size}:rate=30"),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-qp",
            "0",
            "-t",
            &duration_secs.to_string(),
        ])
        .arg(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to spawn ffmpeg");

    assert!(status.success(), "ffmpeg failed to generate reference {name}");
    path
}
