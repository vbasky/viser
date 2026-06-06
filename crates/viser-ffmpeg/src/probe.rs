use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::ffprobe_path;

/// Parsed result of an `ffprobe` run: container format plus all streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Container-level format information.
    pub format: FormatInfo,
    /// All streams (video, audio, subtitle) found in the file.
    pub streams: Vec<StreamInfo>,
}

/// Container-level metadata reported by ffprobe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatInfo {
    /// Probed file path.
    pub filename: String,
    /// Short container format name (e.g. `"mov,mp4,m4a,..."`).
    pub format_name: String,
    /// Human-readable container format name.
    pub format_long_name: String,
    /// Total duration in seconds.
    pub duration: f64, // seconds
    /// File size in bytes.
    pub size: i64, // bytes
    /// Overall bitrate in bits per second.
    pub bit_rate: i64, // bits/sec
    /// ffprobe's confidence score for the detected format.
    pub probe_score: i32,
}

/// Per-stream metadata reported by ffprobe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    /// Stream index within the container.
    pub index: i32,
    /// Short codec name (e.g. `"h264"`).
    pub codec_name: String,
    /// Human-readable codec name.
    pub codec_long_name: String,
    /// Stream type: `"video"`, `"audio"`, or `"subtitle"`.
    pub codec_type: String, // "video", "audio", "subtitle"
    /// Codec profile (e.g. `"High"`).
    pub profile: String,
    /// Pixel width (video).
    pub width: i32,
    /// Pixel height (video).
    pub height: i32,
    /// Pixel format (e.g. `"yuv420p"`).
    pub pix_fmt: String,
    /// Codec level.
    pub level: i32,
    /// Field/scan order (e.g. `"progressive"`).
    pub field_order: String,
    /// Color range (e.g. `"tv"`/`"pc"`).
    pub color_range: String,
    /// Color matrix / space.
    pub color_space: String,
    /// Color transfer characteristics (e.g. `"smpte2084"` for PQ).
    pub color_transfer: String,
    /// Color primaries (e.g. `"bt2020"`).
    pub color_primaries: String,
    /// Stream duration in seconds.
    pub duration: f64, // seconds
    /// Stream bitrate in bits per second.
    pub bit_rate: i64, // bits/sec
    /// Number of frames, when known.
    pub nb_frames: i32,
    /// Raw frame rate as a rational string (e.g. `"25/1"`).
    pub r_frame_rate: String, // e.g. "25/1"
    /// Average frame rate as a rational string (e.g. `"25/1"`).
    pub avg_frame_rate: String, // e.g. "25/1"
    /// Audio sample rate in Hz.
    pub sample_rate: i32, // audio
    /// Audio channel count.
    pub channels: i32, // audio
    /// Audio channel layout (e.g. `"stereo"`).
    pub channel_layout: String, // audio
    /// Bits per raw sample (used to detect high-bit-depth/HDR content).
    pub bits_per_raw_sample: i32,
}

impl StreamInfo {
    /// Returns an error unless this is a video stream with positive dimensions.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.codec_type != "video" {
            anyhow::bail!("not a video stream (type={})", self.codec_type);
        }
        if self.width <= 0 || self.height <= 0 {
            anyhow::bail!("invalid dimensions: {}x{}", self.width, self.height);
        }
        Ok(())
    }

    /// Frame rate parsed from r_frame_rate (e.g. "50/1" -> 50.0).
    pub fn fps(&self) -> f64 {
        parse_rational(&self.r_frame_rate)
    }

    /// Returns `"WIDTHxHEIGHT"`, or an empty string if dimensions are unknown.
    pub fn resolution_str(&self) -> String {
        if self.width == 0 || self.height == 0 {
            String::new()
        } else {
            format!("{}x{}", self.width, self.height)
        }
    }

    /// Detects HDR type from color metadata: `"PQ"`, `"HLG"`, `"BT.2020"`, or `None` for SDR.
    pub fn hdr_kind(&self) -> Option<&'static str> {
        let transfer = self.color_transfer.to_ascii_lowercase();
        if transfer == "smpte2084" {
            return Some("PQ");
        }
        if transfer == "arib-std-b67" {
            return Some("HLG");
        }

        let primaries_bt2020 = self.color_primaries.eq_ignore_ascii_case("bt2020");
        let high_bit_depth = self.bits_per_raw_sample >= 10
            || self.pix_fmt.contains("10")
            || self.pix_fmt.contains("12")
            || self.pix_fmt.contains("16");
        if primaries_bt2020 && high_bit_depth {
            return Some("BT.2020");
        }

        None
    }

    /// Returns `true` if the stream carries HDR color metadata.
    pub fn is_hdr(&self) -> bool {
        self.hdr_kind().is_some()
    }
}

impl FormatInfo {
    /// Container duration as a `Duration`.
    pub fn duration_secs(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.duration)
    }
}

impl ProbeResult {
    /// First video stream, if any.
    pub fn video_stream(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.codec_type == "video")
    }

    /// First audio stream, if any.
    pub fn audio_stream(&self) -> Option<&StreamInfo> {
        self.streams.iter().find(|s| s.codec_type == "audio")
    }
}

/// Runs ffprobe on the given file and returns parsed results.
pub async fn probe(path: &str) -> anyhow::Result<ProbeResult> {
    let args = ["-v", "error", "-print_format", "json", "-show_format", "-show_streams", path];

    let output = Command::new(ffprobe_path())
        .args(args)
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed for {path}: {stderr}");
    }

    let raw: ProbeJsonRaw = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse ffprobe output: {e}"))?;

    Ok(convert_probe(raw))
}

// Raw ffprobe JSON — numbers come as strings
#[derive(Deserialize)]
struct ProbeJsonRaw {
    format: ProbeFormatRaw,
    streams: Vec<ProbeStreamRaw>,
}

#[derive(Deserialize)]
struct ProbeFormatRaw {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    format_name: String,
    #[serde(default)]
    format_long_name: String,
    #[serde(default)]
    duration: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    bit_rate: String,
    #[serde(default)]
    probe_score: i32,
}

#[derive(Deserialize)]
struct ProbeStreamRaw {
    #[serde(default)]
    index: i32,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    codec_long_name: String,
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    width: i32,
    #[serde(default)]
    height: i32,
    #[serde(default)]
    pix_fmt: String,
    #[serde(default)]
    level: i32,
    #[serde(default)]
    field_order: String,
    #[serde(default)]
    color_range: String,
    #[serde(default)]
    color_space: String,
    #[serde(default)]
    color_transfer: String,
    #[serde(default)]
    color_primaries: String,
    #[serde(default)]
    duration: String,
    #[serde(default)]
    bit_rate: String,
    #[serde(default)]
    nb_frames: String,
    #[serde(default)]
    r_frame_rate: String,
    #[serde(default)]
    avg_frame_rate: String,
    #[serde(default)]
    sample_rate: String,
    #[serde(default)]
    channels: i32,
    #[serde(default)]
    channel_layout: String,
    #[serde(default)]
    bits_per_raw_sample: String,
}

fn convert_probe(raw: ProbeJsonRaw) -> ProbeResult {
    let format = FormatInfo {
        filename: raw.format.filename,
        format_name: raw.format.format_name,
        format_long_name: raw.format.format_long_name,
        duration: raw.format.duration.parse().unwrap_or(0.0),
        size: raw.format.size.parse().unwrap_or(0),
        bit_rate: raw.format.bit_rate.parse().unwrap_or(0),
        probe_score: raw.format.probe_score,
    };

    let streams = raw
        .streams
        .into_iter()
        .map(|s| StreamInfo {
            index: s.index,
            codec_name: s.codec_name,
            codec_long_name: s.codec_long_name,
            codec_type: s.codec_type,
            profile: s.profile,
            width: s.width,
            height: s.height,
            pix_fmt: s.pix_fmt,
            level: s.level,
            field_order: s.field_order,
            color_range: s.color_range,
            color_space: s.color_space,
            color_transfer: s.color_transfer,
            color_primaries: s.color_primaries,
            duration: s.duration.parse().unwrap_or(0.0),
            bit_rate: s.bit_rate.parse().unwrap_or(0),
            nb_frames: s.nb_frames.parse().unwrap_or(0),
            r_frame_rate: s.r_frame_rate,
            avg_frame_rate: s.avg_frame_rate,
            sample_rate: s.sample_rate.parse().unwrap_or(0),
            channels: s.channels,
            channel_layout: s.channel_layout,
            bits_per_raw_sample: s.bits_per_raw_sample.parse().unwrap_or(0),
        })
        .collect();

    ProbeResult { format, streams }
}

fn parse_rational(s: &str) -> f64 {
    if let Some((num_s, den_s)) = s.split_once('/') {
        let num: f64 = num_s.parse().unwrap_or(0.0);
        let den: f64 = den_s.parse().unwrap_or(0.0);
        if den != 0.0 { num / den } else { 0.0 }
    } else {
        s.parse().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_stream() -> StreamInfo {
        StreamInfo {
            index: 0,
            codec_name: "h264".into(),
            codec_long_name: String::new(),
            codec_type: "video".into(),
            profile: String::new(),
            width: 1920,
            height: 1080,
            pix_fmt: "yuv420p".into(),
            level: 0,
            field_order: String::new(),
            color_range: String::new(),
            color_space: String::new(),
            color_transfer: String::new(),
            color_primaries: String::new(),
            duration: 0.0,
            bit_rate: 0,
            nb_frames: 0,
            r_frame_rate: "24/1".into(),
            avg_frame_rate: "24/1".into(),
            sample_rate: 0,
            channels: 0,
            channel_layout: String::new(),
            bits_per_raw_sample: 8,
        }
    }

    #[test]
    fn test_hdr_kind_detects_pq() {
        let mut stream = video_stream();
        stream.color_transfer = "smpte2084".into();
        assert_eq!(stream.hdr_kind(), Some("PQ"));
        assert!(stream.is_hdr());
    }

    #[test]
    fn test_hdr_kind_detects_hlg() {
        let mut stream = video_stream();
        stream.color_transfer = "arib-std-b67".into();
        assert_eq!(stream.hdr_kind(), Some("HLG"));
    }

    #[test]
    fn test_hdr_kind_detects_bt2020_high_bit_depth() {
        let mut stream = video_stream();
        stream.color_primaries = "bt2020".into();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_hdr_kind_ignores_sdr() {
        assert_eq!(video_stream().hdr_kind(), None);
    }
}
