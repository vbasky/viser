//! HLS and DASH manifest generation from ladder delivery artifacts.
//!
//! Produces streaming playlists from a set of encoded rung files — the
//! packaging step that turns per-title ladder outputs into ABR-ready manifests.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use viser_ffmpeg::Resolution;

/// One variant (rung) referenced by a streaming manifest.
#[derive(Debug, Clone)]
pub struct Variant {
    /// Relative or absolute path to the encoded file.
    pub path: String,
    /// Video width in pixels.
    pub width: i32,
    /// Video height in pixels.
    pub height: i32,
    /// FFmpeg encoder name (e.g. `libx264`, `libvpx-vp9`).
    pub codec: String,
    /// Measured or target bitrate in kbps.
    pub bitrate_kbps: f64,
    /// Content duration in seconds (used for DASH `mediaPresentationDuration`).
    pub duration_secs: f64,
}

impl Variant {
    /// Builds a variant from a ladder rung and output path.
    pub fn from_rung(
        path: impl Into<String>,
        resolution: Resolution,
        codec: &str,
        bitrate_kbps: f64,
        duration_secs: f64,
    ) -> Self {
        Self {
            path: path.into(),
            width: resolution.width,
            height: resolution.height,
            codec: codec.to_string(),
            bitrate_kbps,
            duration_secs,
        }
    }
}

/// Options controlling manifest emission.
#[derive(Debug, Clone, Default)]
pub struct ManifestOpts {
    /// Optional audio bitrate overhead (kbps) added to each variant bandwidth.
    pub audio_bitrate_kbps: f64,
    /// Base URL prefix prepended to each file path (e.g. `https://cdn.example.com/vod/`).
    pub base_url: Option<String>,
}

/// Writes an HLS master playlist referencing each variant file.
pub fn write_hls_master(
    path: &str,
    variants: &[Variant],
    opts: &ManifestOpts,
) -> anyhow::Result<()> {
    if variants.is_empty() {
        anyhow::bail!("cannot write HLS manifest with zero variants");
    }

    let manifest_dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let mut body = String::from("#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-INDEPENDENT-SEGMENTS\n");

    for variant in variants {
        let bandwidth = variant_bandwidth_bps(variant, opts);
        let avg_bandwidth = (variant.bitrate_kbps * 1000.0).round() as u64;
        let codecs = hls_codecs_string(&variant.codec);
        let file_ref =
            manifest_relative_path(manifest_dir, &variant.path, opts.base_url.as_deref());
        writeln!(
            body,
            "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth},AVERAGE-BANDWIDTH={avg_bandwidth},RESOLUTION={}x{},CODECS=\"{codecs}\"",
            variant.width, variant.height
        )?;
        writeln!(body, "{file_ref}")?;
    }

    std::fs::write(path, body)?;
    Ok(())
}

/// Writes a static DASH MPD referencing each variant as an on-demand representation.
pub fn write_dash_mpd(path: &str, variants: &[Variant], opts: &ManifestOpts) -> anyhow::Result<()> {
    if variants.is_empty() {
        anyhow::bail!("cannot write DASH manifest with zero variants");
    }

    let manifest_dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let duration = variants.iter().map(|v| v.duration_secs).fold(0.0_f64, f64::max);
    let duration_attr = format_iso8601_duration(duration);

    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    writeln!(
        body,
        "<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" profiles=\"urn:mpeg:dash:profile:isoff-on-demand:2011\" type=\"static\" mediaPresentationDuration=\"{duration_attr}\">"
    )?;
    body.push_str("  <Period>\n");
    body.push_str("    <AdaptationSet contentType=\"video\" segmentAlignment=\"true\">\n");

    for (index, variant) in variants.iter().enumerate() {
        let bandwidth = variant_bandwidth_bps(variant, opts);
        let codecs = dash_codecs_string(&variant.codec);
        let file_ref =
            manifest_relative_path(manifest_dir, &variant.path, opts.base_url.as_deref());
        writeln!(
            body,
            "      <Representation id=\"rung{:02}\" bandwidth=\"{bandwidth}\" width=\"{}\" height=\"{}\" codecs=\"{codecs}\">",
            index + 1,
            variant.width,
            variant.height
        )?;
        writeln!(body, "        <BaseURL>{file_ref}</BaseURL>")?;
        body.push_str("      </Representation>\n");
    }

    body.push_str("    </AdaptationSet>\n");
    body.push_str("  </Period>\n");
    body.push_str("</MPD>\n");

    std::fs::write(path, body)?;
    Ok(())
}

fn variant_bandwidth_bps(variant: &Variant, opts: &ManifestOpts) -> u64 {
    let video = (variant.bitrate_kbps * 1000.0).round() as u64;
    let audio = (opts.audio_bitrate_kbps * 1000.0).round() as u64;
    // HLS BANDWIDTH includes container overhead; add ~10% headroom.
    ((video + audio) as f64 * 1.1).round() as u64
}

fn manifest_relative_path(manifest_dir: &Path, file_path: &str, base_url: Option<&str>) -> String {
    if let Some(base) = base_url {
        let trimmed = base.trim_end_matches('/');
        let file = file_path.trim_start_matches('/');
        return format!("{trimmed}/{file}");
    }

    let file = Path::new(file_path);
    if file.is_absolute() {
        return path_to_relative(manifest_dir, file);
    }
    file_path.to_string()
}

fn path_to_relative(base: &Path, target: &Path) -> String {
    let mut rel = PathBuf::new();
    let mut base_iter = base.components().peekable();
    let mut target_iter = target.components().peekable();

    while base_iter.peek().is_some()
        && target_iter.peek().is_some()
        && base_iter.peek() == target_iter.peek()
    {
        base_iter.next();
        target_iter.next();
    }

    for _ in base_iter {
        rel.push("..");
    }
    for part in target_iter {
        match part {
            Component::Normal(p) => rel.push(p),
            Component::CurDir => {}
            Component::ParentDir => rel.push(".."),
            Component::RootDir | Component::Prefix(_) => rel.push(part.as_os_str()),
        }
    }

    rel.to_string_lossy().into_owned()
}

fn hls_codecs_string(codec: &str) -> &'static str {
    match codec {
        "libx264" | "h264_nvenc" | "h264_qsv" | "h264_videotoolbox" | "h264_vaapi" | "h264_amf" => {
            "avc1.4d401f"
        }
        "libx265" | "hevc_nvenc" | "hevc_qsv" | "hevc_videotoolbox" | "hevc_vaapi" | "hevc_amf" => {
            "hvc1.1.6.L93.B0"
        }
        "libsvtav1" | "av1_nvenc" | "av1_qsv" | "av1_vaapi" | "av1_amf" => "av01.0.05M.08",
        "libvpx-vp9" => "vp09.00.41.08",
        _ => "avc1.4d401f",
    }
}

fn dash_codecs_string(codec: &str) -> &'static str {
    hls_codecs_string(codec)
}

fn format_iso8601_duration(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return "PT0S".to_string();
    }
    let total = secs.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("PT{hours}H{minutes}M{seconds}S")
    } else if minutes > 0 {
        format!("PT{minutes}M{seconds}S")
    } else {
        format!("PT{seconds}S")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_variants() -> Vec<Variant> {
        vec![
            Variant::from_rung(
                "renditions/360p.mp4",
                Resolution::new(640, 360),
                "libx264",
                800.0,
                120.0,
            ),
            Variant::from_rung(
                "renditions/720p.mp4",
                Resolution::new(1280, 720),
                "libx264",
                2500.0,
                120.0,
            ),
        ]
    }

    #[test]
    fn test_write_hls_master() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.m3u8");
        write_hls_master(path.to_str().unwrap(), &sample_variants(), &ManifestOpts::default())
            .unwrap();
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.starts_with("#EXTM3U"));
        assert!(body.contains("RESOLUTION=640x360"));
        assert!(body.contains("renditions/360p.mp4"));
        assert!(body.contains("CODECS=\"avc1.4d401f\""));
    }

    #[test]
    fn test_write_dash_mpd() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.mpd");
        write_dash_mpd(path.to_str().unwrap(), &sample_variants(), &ManifestOpts::default())
            .unwrap();
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("<MPD"));
        assert!(body.contains("mediaPresentationDuration=\"PT2M0S\""));
        assert!(body.contains("id=\"rung01\""));
        assert!(body.contains("renditions/720p.mp4"));
    }

    #[test]
    fn test_vp9_codecs_string() {
        assert_eq!(hls_codecs_string("libvpx-vp9"), "vp09.00.41.08");
    }

    #[test]
    fn test_format_iso8601_duration() {
        assert_eq!(format_iso8601_duration(45.0), "PT45S");
        assert_eq!(format_iso8601_duration(125.0), "PT2M5S");
        assert_eq!(format_iso8601_duration(3665.0), "PT1H1M5S");
    }
}
