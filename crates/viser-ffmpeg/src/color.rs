//! Bit depth, pixel format, and HDR color metadata helpers.

use crate::{Codec, StreamInfo};

/// Snapshot of source video color characteristics for encode preservation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceFormat {
    /// Preferred output pixel format (e.g. `yuv420p10le`).
    pub pix_fmt: String,
    /// Effective bit depth (8, 10, 12, or 16).
    pub bit_depth: u8,
    /// Color primaries from probe (e.g. `bt2020`).
    pub color_primaries: String,
    /// Color transfer from probe (e.g. `smpte2084`).
    pub color_transfer: String,
    /// Color matrix / space from probe.
    pub color_space: String,
    /// Whether the stream carries HDR signaling.
    pub is_hdr: bool,
}

impl SourceFormat {
    /// Builds a format snapshot from a probed video stream.
    pub fn from_stream(stream: &StreamInfo) -> Self {
        let bit_depth = bit_depth(stream);
        let pix_fmt = if stream.pix_fmt.is_empty() {
            yuv420p_for_depth(bit_depth).to_string()
        } else {
            stream.pix_fmt.clone()
        };
        Self {
            pix_fmt,
            bit_depth,
            color_primaries: stream.color_primaries.clone(),
            color_transfer: stream.color_transfer.clone(),
            color_space: stream.color_space.clone(),
            is_hdr: stream.is_hdr(),
        }
    }

    /// Returns `true` when the source should be encoded at more than 8 bits per sample.
    pub fn is_high_bit_depth(&self) -> bool {
        self.bit_depth > 8
    }
}

/// Returns the effective bit depth of a video stream.
pub fn bit_depth(stream: &StreamInfo) -> u8 {
    if stream.bits_per_raw_sample >= 10 {
        return stream.bits_per_raw_sample.clamp(8, 16) as u8;
    }
    if stream.pix_fmt.contains("16") {
        return 16;
    }
    if stream.pix_fmt.contains("12") {
        return 12;
    }
    if stream.pix_fmt.contains("10") {
        return 10;
    }
    8
}

/// Default 4:2:0 pixel format for a given bit depth.
pub fn yuv420p_for_depth(depth: u8) -> &'static str {
    match depth {
        10 => "yuv420p10le",
        12 => "yuv420p12le",
        16 => "yuv420p16le",
        _ => "yuv420p",
    }
}

/// PSNR peak value for a given bit depth.
pub fn psnr_peak(depth: u8) -> f64 {
    match depth {
        10 => 1023.0,
        12 => 4095.0,
        16 => 65535.0,
        _ => 255.0,
    }
}

/// Returns whether a software codec can encode at the requested bit depth.
pub fn codec_supports_bit_depth(codec: Codec, depth: u8) -> bool {
    if depth <= 8 {
        return true;
    }
    match codec {
        Codec::X264 | Codec::X265 | Codec::SvtAv1 => true,
        _ => false,
    }
}

/// FFmpeg output arguments that preserve source bit depth and HDR metadata.
pub fn encode_color_args(codec: Codec, format: &SourceFormat) -> Vec<String> {
    let mut args = Vec::new();

    if format.is_high_bit_depth()
        && codec.is_software()
        && codec_supports_bit_depth(codec, format.bit_depth)
    {
        args.extend(["-pix_fmt".into(), format.pix_fmt.clone()]);
        match codec {
            Codec::X264 => args.extend(["-profile:v".into(), "high10".into()]),
            Codec::X265 => args.extend(["-x265-params".into(), x265_params(format)]),
            Codec::SvtAv1 => {}
            _ => {}
        }
    }

    if format.is_hdr {
        append_color_metadata(&mut args, format);
        if codec == Codec::X265 {
            merge_x265_color_params(&mut args, format);
        }
    }

    args
}

fn append_color_metadata(args: &mut Vec<String>, format: &SourceFormat) {
    if !format.color_primaries.is_empty() {
        args.extend(["-color_primaries".into(), format.color_primaries.clone()]);
    }
    if !format.color_transfer.is_empty() {
        args.extend(["-color_trc".into(), format.color_transfer.clone()]);
    }
    if !format.color_space.is_empty() {
        args.extend(["-colorspace".into(), format.color_space.clone()]);
    }
}

fn x265_params(format: &SourceFormat) -> String {
    let mut parts = Vec::new();
    if format.bit_depth > 8 {
        parts.push("profile=main10".into());
    }
    if format.is_hdr {
        if !format.color_primaries.is_empty() {
            parts.push(format!("colorprim={}", format.color_primaries));
        }
        if !format.color_transfer.is_empty() {
            parts.push(format!("transfer={}", format.color_transfer));
        }
        if !format.color_space.is_empty() {
            parts.push(format!("colormatrix={}", format.color_space));
        }
    }
    parts.join(":")
}

fn merge_x265_color_params(args: &mut Vec<String>, format: &SourceFormat) {
    let color = x265_params(format);
    if color.is_empty() {
        return;
    }
    if let Some(idx) = args.iter().position(|a| a == "-x265-params") {
        let existing = args.get(idx + 1).cloned().unwrap_or_default();
        let merged = if existing.is_empty() { color } else { format!("{existing}:{color}") };
        args[idx + 1] = merged;
    } else {
        args.extend(["-x265-params".into(), color]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StreamInfo;

    fn base_stream() -> StreamInfo {
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
            color_space: "bt709".into(),
            color_transfer: "bt709".into(),
            color_primaries: "bt709".into(),
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
    fn test_bit_depth_from_pix_fmt() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(bit_depth(&stream), 10);
    }

    #[test]
    fn test_source_format_high_bit_depth() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(format.bit_depth, 10);
        assert_eq!(format.pix_fmt, "yuv420p10le");
        assert!(format.is_high_bit_depth());
    }

    #[test]
    fn test_encode_color_args_x265_10bit_hdr() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::X265, &format);
        assert!(args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "yuv420p10le"));
        assert!(args.iter().any(|a| a.contains("profile=main10")));
        assert!(args.iter().any(|a| a.contains("transfer=smpte2084")));
    }

    #[test]
    fn test_psnr_peak_scaling() {
        assert_eq!(psnr_peak(8), 255.0);
        assert_eq!(psnr_peak(10), 1023.0);
    }
}
