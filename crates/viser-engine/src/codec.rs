use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported video codec identifier.
///
/// Software encoders (libx264, libx265, libsvtav1, libvpx-vp9) are always
/// available under the FFmpeg backend. Hardware encoder variants require
/// FFmpeg built with the matching SDK and a GPU with the matching ASIC at
/// runtime; availability is detected via `ffmpeg -encoders`.
///
/// [`Codec::External`] is a placeholder for non-FFmpeg engines (neural codecs
/// such as MLVC). The active [`crate::VideoEngine`] decides how to interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Codec {
    /// H.264/AVC via `libx264`.
    #[serde(rename = "libx264")]
    X264,
    /// H.265/HEVC via `libx265`.
    #[serde(rename = "libx265")]
    X265,
    /// AV1 via `libsvtav1` (SVT-AV1).
    #[serde(rename = "libsvtav1")]
    SvtAv1,
    /// VP9 via `libvpx-vp9`.
    #[serde(rename = "libvpx-vp9")]
    Vp9,

    // ── Hardware encoders (H.264) ──
    /// NVIDIA NVENC H.264 (`h264_nvenc`).
    #[serde(rename = "h264_nvenc")]
    NvencH264,
    /// Intel QuickSync H.264 (`h264_qsv`).
    #[serde(rename = "h264_qsv")]
    QsvH264,
    /// Apple VideoToolbox H.264 (`h264_videotoolbox`).
    #[serde(rename = "h264_videotoolbox")]
    VideoToolboxH264,
    /// Linux VAAPI H.264 (`h264_vaapi`).
    #[serde(rename = "h264_vaapi")]
    VaapiH264,
    /// AMD AMF H.264 (`h264_amf`).
    #[serde(rename = "h264_amf")]
    AmfH264,

    // ── Hardware encoders (H.265/HEVC) ──
    /// NVIDIA NVENC HEVC (`hevc_nvenc`).
    #[serde(rename = "hevc_nvenc")]
    NvencH265,
    /// Intel QuickSync HEVC (`hevc_qsv`).
    #[serde(rename = "hevc_qsv")]
    QsvH265,
    /// Apple VideoToolbox HEVC (`hevc_videotoolbox`).
    #[serde(rename = "hevc_videotoolbox")]
    VideoToolboxH265,
    /// Linux VAAPI HEVC (`hevc_vaapi`).
    #[serde(rename = "hevc_vaapi")]
    VaapiH265,
    /// AMD AMF HEVC (`hevc_amf`).
    #[serde(rename = "hevc_amf")]
    AmfH265,

    // ── Hardware encoders (AV1) ──
    /// NVIDIA NVENC AV1 (`av1_nvenc`) — Ada/Blackwell and newer.
    #[serde(rename = "av1_nvenc")]
    NvencAv1,
    /// Intel QuickSync AV1 (`av1_qsv`) — Arc/Battlemage and newer.
    #[serde(rename = "av1_qsv")]
    QsvAv1,
    /// Linux VAAPI AV1 (`av1_vaapi`) — Arc/Battlemage, RDNA3+ and newer.
    #[serde(rename = "av1_vaapi")]
    VaapiAv1,
    /// AMD AMF AV1 (`av1_amf`) — RDNA3+ and newer.
    #[serde(rename = "av1_amf")]
    AmfAv1,

    /// Non-FFmpeg / external engine codec (e.g. neural codecs such as MLVC).
    ///
    /// The active [`crate::VideoEngine`] interprets this; FFmpeg rejects it.
    #[serde(rename = "external")]
    External,
}

/// Hardware encoder backend (GPU vendor / API).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    /// Software encoder (libx264, libx265, libsvtav1, libvpx-vp9).
    Software,
    /// NVIDIA NVENC.
    Nvenc,
    /// Intel QuickSync.
    Qsv,
    /// Apple VideoToolbox.
    VideoToolbox,
    /// Linux VAAPI.
    Vaapi,
    /// AMD AMF.
    Amf,
    /// Non-FFmpeg / external engine backend.
    External,
}

/// Codec family (compression standard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecFamily {
    /// H.264/AVC.
    H264,
    /// H.265/HEVC.
    H265,
    /// AV1.
    Av1,
    /// VP9.
    Vp9,
    /// External / learned / proprietary family.
    Other,
}

impl Codec {
    /// Encoder name for this codec (e.g. `"libx264"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Codec::X264 => "libx264",
            Codec::X265 => "libx265",
            Codec::SvtAv1 => "libsvtav1",
            Codec::Vp9 => "libvpx-vp9",
            Codec::NvencH264 => "h264_nvenc",
            Codec::QsvH264 => "h264_qsv",
            Codec::VideoToolboxH264 => "h264_videotoolbox",
            Codec::VaapiH264 => "h264_vaapi",
            Codec::AmfH264 => "h264_amf",
            Codec::NvencH265 => "hevc_nvenc",
            Codec::QsvH265 => "hevc_qsv",
            Codec::VideoToolboxH265 => "hevc_videotoolbox",
            Codec::VaapiH265 => "hevc_vaapi",
            Codec::AmfH265 => "hevc_amf",
            Codec::NvencAv1 => "av1_nvenc",
            Codec::QsvAv1 => "av1_qsv",
            Codec::VaapiAv1 => "av1_vaapi",
            Codec::AmfAv1 => "av1_amf",
            Codec::External => "external",
        }
    }

    /// Hardware / software encoder backend for this codec.
    pub fn backend(&self) -> EncoderBackend {
        match self {
            Codec::X264 | Codec::X265 | Codec::SvtAv1 | Codec::Vp9 => EncoderBackend::Software,
            Codec::NvencH264 | Codec::NvencH265 | Codec::NvencAv1 => EncoderBackend::Nvenc,
            Codec::QsvH264 | Codec::QsvH265 | Codec::QsvAv1 => EncoderBackend::Qsv,
            Codec::VideoToolboxH264 | Codec::VideoToolboxH265 => EncoderBackend::VideoToolbox,
            Codec::VaapiH264 | Codec::VaapiH265 | Codec::VaapiAv1 => EncoderBackend::Vaapi,
            Codec::AmfH264 | Codec::AmfH265 | Codec::AmfAv1 => EncoderBackend::Amf,
            Codec::External => EncoderBackend::External,
        }
    }

    /// Codec family (compression standard).
    pub fn family(&self) -> CodecFamily {
        match self {
            Codec::X264
            | Codec::NvencH264
            | Codec::QsvH264
            | Codec::VideoToolboxH264
            | Codec::VaapiH264
            | Codec::AmfH264 => CodecFamily::H264,
            Codec::X265
            | Codec::NvencH265
            | Codec::QsvH265
            | Codec::VideoToolboxH265
            | Codec::VaapiH265
            | Codec::AmfH265 => CodecFamily::H265,
            Codec::SvtAv1 | Codec::NvencAv1 | Codec::QsvAv1 | Codec::VaapiAv1 | Codec::AmfAv1 => {
                CodecFamily::Av1
            }
            Codec::Vp9 => CodecFamily::Vp9,
            Codec::External => CodecFamily::Other,
        }
    }

    /// Whether this codec uses a hardware encoder backend.
    pub fn is_hardware(&self) -> bool {
        matches!(
            self.backend(),
            EncoderBackend::Nvenc
                | EncoderBackend::Qsv
                | EncoderBackend::VideoToolbox
                | EncoderBackend::Vaapi
                | EncoderBackend::Amf
        )
    }

    /// Whether this codec is a software encoder.
    pub fn is_software(&self) -> bool {
        matches!(self.backend(), EncoderBackend::Software)
    }

    /// Whether this codec is handled by an external (non-FFmpeg) engine.
    pub fn is_external(&self) -> bool {
        matches!(self.backend(), EncoderBackend::External)
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Codec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "libx264" | "x264" | "h264" => Ok(Codec::X264),
            "libx265" | "x265" | "h265" | "hevc" => Ok(Codec::X265),
            "libsvtav1" | "svtav1" | "av1" => Ok(Codec::SvtAv1),
            "libvpx-vp9" | "vp9" | "libvpx" => Ok(Codec::Vp9),
            // NVENC
            "h264_nvenc" | "nvenc" | "nvenc_h264" => Ok(Codec::NvencH264),
            "hevc_nvenc" | "nvenc_h265" | "nvenc_hevc" => Ok(Codec::NvencH265),
            // QuickSync
            "h264_qsv" | "qsv" | "qsv_h264" => Ok(Codec::QsvH264),
            "hevc_qsv" | "qsv_h265" | "qsv_hevc" => Ok(Codec::QsvH265),
            // VideoToolbox
            "h264_videotoolbox" | "vt" | "vt_h264" | "videotoolbox" => Ok(Codec::VideoToolboxH264),
            "hevc_videotoolbox" | "vt_h265" | "vt_hevc" => Ok(Codec::VideoToolboxH265),
            // VAAPI
            "h264_vaapi" | "vaapi" | "vaapi_h264" => Ok(Codec::VaapiH264),
            "hevc_vaapi" | "vaapi_h265" | "vaapi_hevc" => Ok(Codec::VaapiH265),
            // AMF
            "h264_amf" | "amf" | "amf_h264" => Ok(Codec::AmfH264),
            "hevc_amf" | "amf_h265" | "amf_hevc" => Ok(Codec::AmfH265),
            // AV1 hardware
            "av1_nvenc" | "nvenc_av1" => Ok(Codec::NvencAv1),
            "av1_qsv" | "qsv_av1" => Ok(Codec::QsvAv1),
            "av1_vaapi" | "vaapi_av1" => Ok(Codec::VaapiAv1),
            "av1_amf" | "amf_av1" => Ok(Codec::AmfAv1),
            // External / neural codec aliases
            "external" | "mlvc" | "mlvc-s" => Ok(Codec::External),
            _ => Err(anyhow::anyhow!("unknown codec: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_roundtrip_common() {
        for s in ["libx264", "libx265", "libsvtav1", "libvpx-vp9", "h264_nvenc", "av1_qsv"] {
            let c: Codec = s.parse().unwrap();
            assert_eq!(c.as_str(), s);
        }
    }

    #[test]
    fn external_codec() {
        let c: Codec = "mlvc".parse().unwrap();
        assert!(c.is_external());
        assert_eq!(c.family(), CodecFamily::Other);
        assert_eq!(c.as_str(), "external");
    }
}
