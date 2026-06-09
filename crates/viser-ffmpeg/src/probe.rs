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
    ///
    /// Detection order:
    /// 1. `color_transfer` → `"smpte2084"` → `"PQ"`
    /// 2. `color_transfer` → `"arib-std-b67"` → `"HLG"`
    /// 3. BT.2020 colour primaries (or colour-space) + high bit depth → `"BT.2020"`
    ///
    /// ffprobe ≥ 8.0 does not always populate `color_primaries` or `bits_per_raw_sample`
    /// as top-level fields for HEVC/Matroska; the `color_space` field and pixel-format
    /// bit-depth are used as fallbacks.
    pub fn hdr_kind(&self) -> Option<&'static str> {
        let transfer = self.color_transfer.to_ascii_lowercase();
        if transfer == "smpte2084" {
            return Some("PQ");
        }
        if transfer == "arib-std-b67" {
            return Some("HLG");
        }

        let high_bit_depth = self.bits_per_raw_sample >= 10
            || self.pix_fmt.contains("10")
            || self.pix_fmt.contains("12")
            || self.pix_fmt.contains("16");

        // color_primaries is the canonical field; when absent (ffprobe ≥ 8.0 for some
        // containers) fall back to color_space which carries equivalent information.
        let primaries_or_space_bt2020 = self.color_primaries.eq_ignore_ascii_case("bt2020")
            || self.color_space.contains("bt2020");
        if primaries_or_space_bt2020 && high_bit_depth {
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

    fn audio_stream() -> StreamInfo {
        StreamInfo {
            index: 1,
            codec_type: "audio".into(),
            codec_name: "aac".into(),
            sample_rate: 48000,
            channels: 2,
            channel_layout: "stereo".into(),
            ..video_stream()
        }
    }

    // ── HDR detection ──
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

    #[test]
    fn test_hdr_kind_case_insensitive_pq() {
        let mut stream = video_stream();
        stream.color_transfer = "SMPTE2084".into();
        assert_eq!(stream.hdr_kind(), Some("PQ"));
    }

    #[test]
    fn test_hdr_kind_case_insensitive_hlg() {
        let mut stream = video_stream();
        stream.color_transfer = "ARIB-STD-B67".into();
        assert_eq!(stream.hdr_kind(), Some("HLG"));
    }

    #[test]
    fn test_hdr_kind_case_insensitive_primaries() {
        let mut stream = video_stream();
        stream.color_primaries = "BT2020".into();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_hdr_kind_pq_takes_priority_over_bt2020() {
        let mut stream = video_stream();
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(stream.hdr_kind(), Some("PQ"));
    }

    #[test]
    fn test_hdr_bt2020_no_high_bit_depth_not_hdr() {
        let mut stream = video_stream();
        stream.color_primaries = "bt2020".into();
        stream.pix_fmt = "yuv420p".into();
        stream.bits_per_raw_sample = 8;
        assert_eq!(stream.hdr_kind(), None);
    }

    #[test]
    fn test_hdr_16bit_pix_fmt_detected() {
        let mut stream = video_stream();
        stream.color_primaries = "bt2020".into();
        stream.pix_fmt = "yuv420p16le".into();
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_hdr_12bit_pix_fmt_detected() {
        let mut stream = video_stream();
        stream.color_primaries = "bt2020".into();
        stream.pix_fmt = "yuv420p12le".into();
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_hdr_bits_per_raw_sample_10() {
        let mut stream = video_stream();
        stream.color_primaries = "bt2020".into();
        stream.bits_per_raw_sample = 10;
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_is_hdr_false_for_sdr() {
        assert!(!video_stream().is_hdr());
    }

    #[test]
    fn test_hdr_color_space_fallback_without_primaries() {
        // ffprobe >= 8.0 may omit color_primaries but report color_space
        let mut stream = video_stream();
        stream.color_primaries = String::new();
        stream.color_space = "bt2020nc".into();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_hdr_color_space_fallback_bt2020c() {
        let mut stream = video_stream();
        stream.color_primaries = String::new();
        stream.color_space = "bt2020c".into();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(stream.hdr_kind(), Some("BT.2020"));
    }

    #[test]
    fn test_hdr_color_space_sdr_8bit_not_hdr() {
        let mut stream = video_stream();
        stream.color_primaries = String::new();
        stream.color_space = "bt2020nc".into();
        stream.pix_fmt = "yuv420p".into();
        stream.bits_per_raw_sample = 8;
        assert_eq!(stream.hdr_kind(), None);
    }

    // ── Stream validation ──
    #[test]
    fn test_validate_video_stream_ok() {
        assert!(video_stream().validate().is_ok());
    }

    #[test]
    fn test_validate_audio_stream_fails() {
        assert!(audio_stream().validate().is_err());
    }

    #[test]
    fn test_validate_zero_width_fails() {
        let mut stream = video_stream();
        stream.width = 0;
        assert!(stream.validate().is_err());
    }

    #[test]
    fn test_validate_zero_height_fails() {
        let mut stream = video_stream();
        stream.height = 0;
        assert!(stream.validate().is_err());
    }

    #[test]
    fn test_validate_negative_dimensions_fails() {
        let mut stream = video_stream();
        stream.width = -1;
        assert!(stream.validate().is_err());
    }

    // ── Frame rate parsing ──
    #[test]
    fn test_fps_standard() {
        let mut stream = video_stream();
        stream.r_frame_rate = "24/1".into();
        assert!((stream.fps() - 24.0).abs() < 1e-9);
    }

    #[test]
    fn test_fps_ntsc() {
        let mut stream = video_stream();
        stream.r_frame_rate = "30000/1001".into();
        assert!((stream.fps() - 29.97).abs() < 0.1);
    }

    #[test]
    fn test_fps_pal() {
        let mut stream = video_stream();
        stream.r_frame_rate = "25/1".into();
        assert!((stream.fps() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn test_fps_60fps() {
        let mut stream = video_stream();
        stream.r_frame_rate = "60/1".into();
        assert!((stream.fps() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn test_fps_high_frame_rate() {
        let mut stream = video_stream();
        stream.r_frame_rate = "120/1".into();
        assert!((stream.fps() - 120.0).abs() < 1e-9);
    }

    // ── parse_rational edge cases ──
    #[test]
    fn test_parse_rational_division_by_zero() {
        assert!((parse_rational("24/0") - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_rational_no_slash() {
        assert!((parse_rational("30") - 30.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_rational_empty_string() {
        assert!((parse_rational("") - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_rational_bogus() {
        assert!((parse_rational("abc") - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_rational_negative_numerator() {
        let v = parse_rational("-24/1001");
        assert!(v < 0.0, "negative rational should be negative, got {v}");
        assert!(v > -0.05, "expected > -0.05, got {v}");
    }

    #[test]
    fn test_fps_empty_r_frame_rate() {
        let mut stream = video_stream();
        stream.r_frame_rate = String::new();
        assert!((stream.fps() - 0.0).abs() < 1e-9);
    }

    // ── Resolution string ──
    #[test]
    fn test_resolution_str_standard() {
        let mut stream = video_stream();
        stream.width = 3840;
        stream.height = 2160;
        assert_eq!(stream.resolution_str(), "3840x2160");
    }

    #[test]
    fn test_resolution_str_zero_dimensions() {
        let mut stream = video_stream();
        stream.width = 0;
        stream.height = 0;
        assert_eq!(stream.resolution_str(), "");
    }

    // ── Video/audio stream finders ──
    #[test]
    fn test_probe_result_video_stream() {
        let result = ProbeResult {
            format: FormatInfo {
                filename: "test.mp4".into(),
                format_name: "mov,mp4".into(),
                format_long_name: String::new(),
                duration: 10.0,
                size: 5000000,
                bit_rate: 4000000,
                probe_score: 100,
            },
            streams: vec![video_stream(), audio_stream()],
        };
        assert!(result.video_stream().is_some());
        assert_eq!(result.video_stream().unwrap().codec_type, "video");
    }

    #[test]
    fn test_probe_result_audio_stream() {
        let result = ProbeResult {
            format: FormatInfo {
                filename: "test.mp4".into(),
                format_name: "mov,mp4".into(),
                format_long_name: String::new(),
                duration: 10.0,
                size: 5000000,
                bit_rate: 4000000,
                probe_score: 100,
            },
            streams: vec![video_stream(), audio_stream()],
        };
        assert!(result.audio_stream().is_some());
        assert_eq!(result.audio_stream().unwrap().codec_type, "audio");
    }

    #[test]
    fn test_probe_result_audio_only_no_video() {
        let result = ProbeResult {
            format: FormatInfo {
                filename: "audio.aac".into(),
                format_name: "aac".into(),
                format_long_name: String::new(),
                duration: 180.0,
                size: 500000,
                bit_rate: 128000,
                probe_score: 100,
            },
            streams: vec![audio_stream()],
        };
        assert!(result.video_stream().is_none());
    }

    #[test]
    fn test_probe_result_multiple_video_streams_finds_first() {
        let mut stream1 = video_stream();
        stream1.index = 0;
        let mut stream2 = video_stream();
        stream2.index = 1;
        let result = ProbeResult {
            format: FormatInfo {
                filename: "multi.mp4".into(),
                format_name: "mov,mp4".into(),
                format_long_name: String::new(),
                duration: 10.0,
                size: 5000000,
                bit_rate: 4000000,
                probe_score: 100,
            },
            streams: vec![stream1.clone(), stream2],
        };
        let found = result.video_stream().unwrap();
        assert_eq!(found.index, 0);
    }

    // ── Format duration ──
    #[test]
    fn test_format_duration_secs() {
        let format = FormatInfo {
            filename: "test.mp4".into(),
            format_name: "mov,mp4".into(),
            format_long_name: String::new(),
            duration: 123.456,
            size: 0,
            bit_rate: 0,
            probe_score: 100,
        };
        let d = format.duration_secs();
        assert!((d.as_secs_f64() - 123.456).abs() < 0.001);
    }

    // ── convert_probe edge cases ──
    #[test]
    fn test_convert_probe_empty_streams() {
        let raw = serde_json::from_str::<serde_json::Value>(
            r#"{
            "format": {
                "filename": "test.mp4",
                "format_name": "mov,mp4",
                "format_long_name": "QuickTime / MOV",
                "duration": "10.500",
                "size": "1000000",
                "bit_rate": "800000",
                "probe_score": 100
            },
            "streams": []
        }"#,
        )
        .unwrap();
        let raw_probe: ProbeJsonRaw = serde_json::from_value(raw).unwrap();
        let result = convert_probe(raw_probe);
        assert!((result.format.duration - 10.5).abs() < 1e-9);
        assert_eq!(result.streams.len(), 0);
    }

    #[test]
    fn test_convert_probe_missing_optional_fields() {
        let raw = serde_json::from_str::<serde_json::Value>(
            r#"{
            "format": {},
            "streams": [
                {
                    "codec_type": "video",
                    "r_frame_rate": "30/1",
                    "avg_frame_rate": "30/1"
                }
            ]
        }"#,
        )
        .unwrap();
        let raw_probe: ProbeJsonRaw = serde_json::from_value(raw).unwrap();
        let result = convert_probe(raw_probe);
        assert_eq!(result.format.duration, 0.0);
        assert_eq!(result.format.size, 0);
        assert_eq!(result.format.bit_rate, 0);
        assert_eq!(result.streams[0].width, 0);
        assert_eq!(result.streams[0].height, 0);
    }

    #[test]
    fn test_convert_probe_bogus_numeric_strings() {
        let raw = serde_json::from_str::<serde_json::Value>(
            r#"{
            "format": {
                "duration": "not_a_number",
                "size": "",
                "bit_rate": "also_bogus"
            },
            "streams": [{
                "codec_type": "video",
                "nb_frames": "bogus",
                "r_frame_rate": "abc",
                "avg_frame_rate": "def"
            }]
        }"#,
        )
        .unwrap();
        let raw_probe: ProbeJsonRaw = serde_json::from_value(raw).unwrap();
        let result = convert_probe(raw_probe);
        assert_eq!(result.format.duration, 0.0);
        assert_eq!(result.format.size, 0);
        assert_eq!(result.format.bit_rate, 0);
        assert_eq!(result.streams[0].nb_frames, 0);
    }

    #[test]
    fn test_convert_probe_multiple_streams_mixed_types() {
        let raw = serde_json::from_str::<serde_json::Value>(r#"{
            "format": {"duration": "60.0"},
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
                 "r_frame_rate": "24/1", "avg_frame_rate": "24/1"},
                {"index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2,
                 "r_frame_rate": "0/0", "avg_frame_rate": "0/0"},
                {"index": 2, "codec_type": "subtitle", "codec_name": "mov_text",
                 "r_frame_rate": "0/0", "avg_frame_rate": "0/0"}
            ]
        }"#).unwrap();
        let raw_probe: ProbeJsonRaw = serde_json::from_value(raw).unwrap();
        let result = convert_probe(raw_probe);
        assert_eq!(result.streams.len(), 3);
        assert_eq!(result.streams[0].codec_type, "video");
        assert_eq!(result.streams[1].codec_type, "audio");
        assert_eq!(result.streams[2].codec_type, "subtitle");
        // audio stream parsed correctly
        assert_eq!(result.streams[1].sample_rate, 48000);
        assert_eq!(result.streams[1].channels, 2);
    }

    #[test]
    fn test_convert_probe_fractional_fps() {
        let raw = serde_json::from_str::<serde_json::Value>(
            r#"{
            "format": {},
            "streams": [{
                "codec_type": "video",
                "r_frame_rate": "30000/1001",
                "avg_frame_rate": "30000/1001"
            }]
        }"#,
        )
        .unwrap();
        let raw_probe: ProbeJsonRaw = serde_json::from_value(raw).unwrap();
        let result = convert_probe(raw_probe);
        let fps = result.streams[0].fps();
        assert!(fps > 29.0 && fps < 30.0);
    }

    // ── ProbeResult serde roundtrip ──
    #[test]
    fn test_probe_result_serde_roundtrip() {
        let result = ProbeResult {
            format: FormatInfo {
                filename: "sintel_trailer.mp4".into(),
                format_name: "mov,mp4,m4a,3gp,3g2,mj2".into(),
                format_long_name: "QuickTime / MOV".into(),
                duration: 52.0,
                size: 23976340,
                bit_rate: 3688667,
                probe_score: 100,
            },
            streams: vec![video_stream(), audio_stream()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ProbeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.format.filename, result.format.filename);
        assert!((back.format.duration - result.format.duration).abs() < 1e-9);
        assert_eq!(back.streams.len(), 2);
        assert_eq!(back.streams[0].codec_type, "video");
        assert_eq!(back.streams[1].codec_type, "audio");
    }

    // ── Stream info defaults ──
    #[test]
    fn test_stream_info_resolution_str_empty_for_zero() {
        let stream = StreamInfo { width: 0, height: 720, ..video_stream() };
        assert_eq!(stream.resolution_str(), "");
    }

    #[test]
    fn test_stream_info_hdr_pq_case_variation() {
        let mut stream = video_stream();
        stream.color_transfer = "SmPtE2084".into();
        assert_eq!(stream.hdr_kind(), Some("PQ"));
    }

    // ── Validation message for non-video streams ──
    #[test]
    fn test_validate_subtitle_stream_fails() {
        let mut stream = video_stream();
        stream.codec_type = "subtitle".into();
        let err = stream.validate().unwrap_err();
        assert!(err.to_string().contains("not a video stream"));
    }

    #[test]
    fn test_validate_data_stream_fails() {
        let mut stream = video_stream();
        stream.codec_type = "data".into();
        assert!(stream.validate().is_err());
    }
}
