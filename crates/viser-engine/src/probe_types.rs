use serde::{Deserialize, Serialize};

/// Parsed result of a media probe: container format plus all streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Container-level format information.
    pub format: FormatInfo,
    /// All streams (video, audio, subtitle) found in the file.
    pub streams: Vec<StreamInfo>,
}

/// Container-level metadata.
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
    /// Probe confidence score for the detected format.
    pub probe_score: i32,
}

/// Per-stream metadata.
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

        let high_bit_depth = self.bits_per_raw_sample >= 10
            || self.pix_fmt.contains("10")
            || self.pix_fmt.contains("12")
            || self.pix_fmt.contains("16");

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

fn parse_rational(s: &str) -> f64 {
    if let Some((n, d)) = s.split_once('/') {
        let n: f64 = n.parse().unwrap_or(0.0);
        let d: f64 = d.parse().unwrap_or(1.0);
        if d != 0.0 { n / d } else { 0.0 }
    } else {
        s.parse().unwrap_or(0.0)
    }
}
