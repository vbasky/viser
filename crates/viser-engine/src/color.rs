use crate::{Codec, EncoderBackend, Hdr10Metadata, StreamInfo};

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
    /// HDR10 static metadata (mastering display + MaxCLL/MaxFALL), when probed.
    pub hdr10: Option<Hdr10Metadata>,
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
            hdr10: None,
        }
    }

    /// Returns `true` when the source should be encoded at more than 8 bits per sample.
    pub fn is_high_bit_depth(&self) -> bool {
        self.bit_depth > 8
    }

    /// Attaches pre-probed HDR10 metadata.
    pub fn with_hdr10(mut self, hdr10: Hdr10Metadata) -> Self {
        self.hdr10 = Some(hdr10);
        self
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

/// Returns whether a codec can encode at the requested bit depth.
pub fn codec_supports_bit_depth(codec: Codec, depth: u8) -> bool {
    if depth <= 8 {
        return true;
    }
    match codec {
        Codec::X264 | Codec::X265 | Codec::SvtAv1 | Codec::Vp9 => true,
        Codec::NvencH265 | Codec::QsvH265 | Codec::AmfH265 | Codec::VideoToolboxH265 => true,
        Codec::NvencAv1 | Codec::QsvAv1 | Codec::AmfAv1 => true,
        Codec::VaapiH265 | Codec::VaapiAv1 => true,
        Codec::NvencH264
        | Codec::QsvH264
        | Codec::AmfH264
        | Codec::VideoToolboxH264
        | Codec::VaapiH264 => false,
        Codec::External => true, // external engines declare their own limits
    }
}

/// Returns the hardware-appropriate high-bit-depth pixel format for a given
/// codec and source format, or `None` when not applicable.
pub fn hw_pix_fmt(format: &SourceFormat, codec: Codec) -> Option<String> {
    if !format.is_high_bit_depth() {
        return None;
    }
    match codec.backend() {
        EncoderBackend::Nvenc
        | EncoderBackend::Qsv
        | EncoderBackend::Amf
        | EncoderBackend::VideoToolbox => {
            if format.bit_depth == 10 {
                Some("p010le".into())
            } else {
                None
            }
        }
        EncoderBackend::Vaapi | EncoderBackend::Software | EncoderBackend::External => None,
    }
}
