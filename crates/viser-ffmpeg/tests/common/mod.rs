#![allow(dead_code)]
// Test-support functions are shared across multiple test binaries; each
// binary only uses a subset, so dead_code is expected.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns `true` if ffmpeg and ffprobe are on PATH.
pub(crate) fn has_ffmpeg() -> bool {
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

/// Returns `true` if the installed ffmpeg lists the named encoder
/// (e.g. `libsvtav1`).
pub(crate) fn has_encoder(name: &str) -> bool {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).contains(name),
        _ => false,
    }
}

/// Returns `true` if the installed ffmpeg supports the `libvmaf` filter.
/// Checks by parsing `ffmpeg -filters` output for `libvmaf`.
pub(crate) fn has_libvmaf() -> bool {
    let output = Command::new("ffmpeg")
        .arg("-filters")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).contains("libvmaf"),
        _ => false,
    }
}

/// Generate a synthetic test video clip using ffmpeg's lavfi source.
/// Returns the path to the generated file.
pub(crate) fn generate_test_clip(
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

/// Generate a 10-bit SDR test clip (HEVC, BT.709, MP4 container).
pub(crate) fn generate_10bit_sdr_clip(dir: &Path) -> PathBuf {
    let path = dir.join("sdr10_test.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p10le",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-t",
            "2",
        ])
        .arg(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to spawn ffmpeg");

    assert!(status.success(), "ffmpeg failed to generate 10-bit SDR clip");
    path
}

/// Generate an HDR test clip (HEVC with PQ transfer, MP4 container).
/// MP4 reliably exposes color metadata to ffprobe, unlike Matroska.
pub(crate) fn generate_hdr_clip(dir: &Path) -> PathBuf {
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

/// Generate an HDR10 clip carrying mastering-display and content-light static
/// metadata (HEVC, PQ, BT.2020, MP4). Mirrors `generate_hdr_clip` but adds the
/// SMPTE ST 2086 / CTA-861.3 side data needed to exercise HDR10 round-tripping.
pub(crate) fn generate_hdr10_clip(dir: &Path) -> PathBuf {
    let path = dir.join("hdr10_test.mp4");
    let x265_params = "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:\
         master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):\
         max-cll=1000,400";
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
            x265_params,
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

    assert!(status.success(), "ffmpeg failed to generate HDR10 clip");
    path
}

/// Generate a reference clip using lossless x264.
pub(crate) fn generate_reference_clip(
    dir: &Path,
    name: &str,
    size: &str,
    duration_secs: u32,
) -> PathBuf {
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
