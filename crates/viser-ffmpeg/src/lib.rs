//! FFmpeg/FFprobe wrapper for the `viser` video-encoding-optimizer workspace.
//!
//! Provides typed primitives (codecs, resolutions, rate-control modes) plus
//! functions to encode (`encode`), probe media (`probe`), and resolve the
//! `ffmpeg`/`ffprobe` binary paths. A `ProbeCache` deduplicates probe calls.

mod cache;
mod encode;
mod hw_encode;
mod path;
mod probe;
#[cfg(feature = "revelo")]
mod probe_revelo;
#[cfg(feature = "revelo")]
pub use probe_revelo::probe as probe_revelo;

pub use cache::*;
pub use encode::*;
pub use hw_encode::*;
pub use path::*;
pub use probe::*;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported video codec.
///
/// Software encoders (libx264, libx265, libsvtav1) are always available.
/// Hardware encoder variants require FFmpeg built with the matching SDK
/// and a GPU with the matching ASIC at runtime; availability is detected
/// via `ffmpeg -encoders`.
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
    // Apple VideoToolbox has no AV1 encoder, so there is no `av1_videotoolbox`.
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
}

/// Hardware encoder backend (GPU vendor / API).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    /// Software encoder (libx264, libx265, libsvtav1).
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
}

impl Codec {
    /// FFmpeg encoder name for this codec (e.g. `"libx264"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Codec::X264 => "libx264",
            Codec::X265 => "libx265",
            Codec::SvtAv1 => "libsvtav1",
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
        }
    }

    /// Hardware encoder backend for this codec.
    pub fn backend(&self) -> EncoderBackend {
        match self {
            Codec::X264 | Codec::X265 | Codec::SvtAv1 => EncoderBackend::Software,
            Codec::NvencH264 | Codec::NvencH265 | Codec::NvencAv1 => EncoderBackend::Nvenc,
            Codec::QsvH264 | Codec::QsvH265 | Codec::QsvAv1 => EncoderBackend::Qsv,
            Codec::VideoToolboxH264 | Codec::VideoToolboxH265 => EncoderBackend::VideoToolbox,
            Codec::VaapiH264 | Codec::VaapiH265 | Codec::VaapiAv1 => EncoderBackend::Vaapi,
            Codec::AmfH264 | Codec::AmfH265 | Codec::AmfAv1 => EncoderBackend::Amf,
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
        }
    }

    /// Whether this codec uses a hardware encoder backend.
    pub fn is_hardware(&self) -> bool {
        !matches!(self.backend(), EncoderBackend::Software)
    }

    /// Whether this codec is a software encoder.
    pub fn is_software(&self) -> bool {
        matches!(self.backend(), EncoderBackend::Software)
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
            _ => Err(anyhow::anyhow!("unknown codec: {s}")),
        }
    }
}

/// Video resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resolution {
    /// Pixel width.
    pub width: i32,
    /// Pixel height.
    pub height: i32,
}

impl Resolution {
    /// Creates a resolution from width and height in pixels.
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }

    /// Human-friendly label like "1080p", "720p", etc.
    pub fn label(&self) -> String {
        match self.height {
            h if h >= 2160 => "2160p".into(),
            h if h >= 1440 => "1440p".into(),
            h if h >= 1080 => "1080p".into(),
            h if h >= 720 => "720p".into(),
            h if h >= 480 => "480p".into(),
            h if h >= 360 => "360p".into(),
            h if h >= 240 => "240p".into(),
            h => format!("{h}p"),
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl std::str::FromStr for Resolution {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "2160p" | "4k" => Ok(RES_2160P),
            "1440p" => Ok(RES_1440P),
            "1080p" => Ok(RES_1080P),
            "720p" => Ok(RES_720P),
            "480p" => Ok(RES_480P),
            "360p" => Ok(RES_360P),
            "240p" => Ok(RES_240P),
            other => {
                if let Some((w, h)) = other.split_once('x') {
                    Ok(Resolution::new(w.parse()?, h.parse()?))
                } else {
                    Err(anyhow::anyhow!("invalid resolution: {other}"))
                }
            }
        }
    }
}

/// 3840x2160 (4K UHD), 16:9.
pub const RES_2160P: Resolution = Resolution::new(3840, 2160);
/// 2560x1440 (QHD), 16:9.
pub const RES_1440P: Resolution = Resolution::new(2560, 1440);
/// 1920x1080 (Full HD), 16:9.
pub const RES_1080P: Resolution = Resolution::new(1920, 1080);
/// 1280x720 (HD), 16:9.
pub const RES_720P: Resolution = Resolution::new(1280, 720);
/// 854x480 (SD), 16:9.
pub const RES_480P: Resolution = Resolution::new(854, 480);
/// 640x360, 16:9.
pub const RES_360P: Resolution = Resolution::new(640, 360);
/// 426x240, 16:9.
pub const RES_240P: Resolution = Resolution::new(426, 240);

/// Rate control mode for encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RateControlMode {
    /// Constant rate factor (default).
    #[default]
    Crf,
    /// CRF with VBV/decoder-model bitrate cap.
    CappedCrf,
    /// Fixed quantizer (Netflix-style, no R-D optimization).
    Qp,
    /// 2-pass variable bitrate (for final delivery encodes).
    Vbr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_as_str() {
        assert_eq!(Codec::X264.as_str(), "libx264");
        assert_eq!(Codec::X265.as_str(), "libx265");
        assert_eq!(Codec::SvtAv1.as_str(), "libsvtav1");
        assert_eq!(Codec::NvencH264.as_str(), "h264_nvenc");
        assert_eq!(Codec::QsvH264.as_str(), "h264_qsv");
        assert_eq!(Codec::VideoToolboxH264.as_str(), "h264_videotoolbox");
        assert_eq!(Codec::VaapiH264.as_str(), "h264_vaapi");
        assert_eq!(Codec::AmfH264.as_str(), "h264_amf");
        assert_eq!(Codec::NvencH265.as_str(), "hevc_nvenc");
    }

    #[test]
    fn test_codec_display() {
        assert_eq!(format!("{}", Codec::X264), "libx264");
        assert_eq!(format!("{}", Codec::NvencH264), "h264_nvenc");
    }

    #[test]
    fn test_codec_from_str() {
        assert_eq!("libx264".parse::<Codec>().unwrap(), Codec::X264);
        assert_eq!("x264".parse::<Codec>().unwrap(), Codec::X264);
        assert_eq!("h264".parse::<Codec>().unwrap(), Codec::X264);
        assert_eq!("libx265".parse::<Codec>().unwrap(), Codec::X265);
        assert_eq!("x265".parse::<Codec>().unwrap(), Codec::X265);
        assert_eq!("h265".parse::<Codec>().unwrap(), Codec::X265);
        assert_eq!("hevc".parse::<Codec>().unwrap(), Codec::X265);
        assert_eq!("libsvtav1".parse::<Codec>().unwrap(), Codec::SvtAv1);
        assert_eq!("svtav1".parse::<Codec>().unwrap(), Codec::SvtAv1);
        assert_eq!("av1".parse::<Codec>().unwrap(), Codec::SvtAv1);
        assert_eq!("h264_nvenc".parse::<Codec>().unwrap(), Codec::NvencH264);
        assert_eq!("nvenc".parse::<Codec>().unwrap(), Codec::NvencH264);
        assert_eq!("hevc_nvenc".parse::<Codec>().unwrap(), Codec::NvencH265);
        assert_eq!("h264_qsv".parse::<Codec>().unwrap(), Codec::QsvH264);
        assert_eq!("qsv".parse::<Codec>().unwrap(), Codec::QsvH264);
        assert_eq!("vt".parse::<Codec>().unwrap(), Codec::VideoToolboxH264);
        assert_eq!("h264_vaapi".parse::<Codec>().unwrap(), Codec::VaapiH264);
        assert_eq!("vaapi".parse::<Codec>().unwrap(), Codec::VaapiH264);
        assert_eq!("h264_amf".parse::<Codec>().unwrap(), Codec::AmfH264);
        assert_eq!("amf".parse::<Codec>().unwrap(), Codec::AmfH264);
        assert!("unknown".parse::<Codec>().is_err());
    }

    #[test]
    fn test_codec_backend() {
        assert_eq!(Codec::X264.backend(), EncoderBackend::Software);
        assert_eq!(Codec::NvencH264.backend(), EncoderBackend::Nvenc);
        assert_eq!(Codec::QsvH264.backend(), EncoderBackend::Qsv);
        assert_eq!(Codec::VideoToolboxH264.backend(), EncoderBackend::VideoToolbox);
        assert_eq!(Codec::VaapiH264.backend(), EncoderBackend::Vaapi);
        assert_eq!(Codec::AmfH264.backend(), EncoderBackend::Amf);
    }

    #[test]
    fn test_codec_family() {
        assert_eq!(Codec::X264.family(), CodecFamily::H264);
        assert_eq!(Codec::NvencH264.family(), CodecFamily::H264);
        assert_eq!(Codec::X265.family(), CodecFamily::H265);
        assert_eq!(Codec::NvencH265.family(), CodecFamily::H265);
        assert_eq!(Codec::SvtAv1.family(), CodecFamily::Av1);
    }

    #[test]
    fn test_codec_is_hardware() {
        assert!(!Codec::X264.is_hardware());
        assert!(!Codec::X265.is_hardware());
        assert!(!Codec::SvtAv1.is_hardware());
        assert!(Codec::NvencH264.is_hardware());
        assert!(Codec::QsvH265.is_hardware());
        assert!(Codec::VideoToolboxH264.is_hardware());
    }

    #[test]
    fn test_codec_is_software() {
        assert!(Codec::X264.is_software());
        assert!(!Codec::NvencH264.is_software());
    }

    #[test]
    fn test_codec_serde_roundtrip() {
        for codec in &[
            Codec::X264,
            Codec::X265,
            Codec::SvtAv1,
            Codec::NvencH264,
            Codec::NvencH265,
            Codec::QsvH264,
        ] {
            let json = serde_json::to_string(codec).unwrap();
            let back: Codec = serde_json::from_str(&json).unwrap();
            assert_eq!(*codec, back);
        }
    }

    #[test]
    fn test_av1_hw_codec_as_str() {
        assert_eq!(Codec::NvencAv1.as_str(), "av1_nvenc");
        assert_eq!(Codec::QsvAv1.as_str(), "av1_qsv");
        assert_eq!(Codec::VaapiAv1.as_str(), "av1_vaapi");
        assert_eq!(Codec::AmfAv1.as_str(), "av1_amf");
    }

    #[test]
    fn test_av1_hw_codec_from_str() {
        assert_eq!("av1_nvenc".parse::<Codec>().unwrap(), Codec::NvencAv1);
        assert_eq!("nvenc_av1".parse::<Codec>().unwrap(), Codec::NvencAv1);
        assert_eq!("av1_qsv".parse::<Codec>().unwrap(), Codec::QsvAv1);
        assert_eq!("av1_vaapi".parse::<Codec>().unwrap(), Codec::VaapiAv1);
        assert_eq!("av1_amf".parse::<Codec>().unwrap(), Codec::AmfAv1);
    }

    #[test]
    fn test_av1_hw_codec_backend_and_family() {
        for codec in &[Codec::NvencAv1, Codec::QsvAv1, Codec::VaapiAv1, Codec::AmfAv1] {
            assert_eq!(codec.family(), CodecFamily::Av1);
            assert!(codec.is_hardware());
        }
        assert_eq!(Codec::NvencAv1.backend(), EncoderBackend::Nvenc);
        assert_eq!(Codec::QsvAv1.backend(), EncoderBackend::Qsv);
        assert_eq!(Codec::VaapiAv1.backend(), EncoderBackend::Vaapi);
        assert_eq!(Codec::AmfAv1.backend(), EncoderBackend::Amf);
    }

    #[test]
    fn test_codec_eq() {
        assert_eq!(Codec::X264, Codec::X264);
        assert_ne!(Codec::X264, Codec::X265);
        assert_ne!(Codec::X264, Codec::NvencH264);
    }

    #[test]
    fn test_codec_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Codec::X264);
        set.insert(Codec::X264);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_resolution_new() {
        let r = Resolution::new(1920, 1080);
        assert_eq!(r.width, 1920);
        assert_eq!(r.height, 1080);
    }

    #[test]
    fn test_resolution_label() {
        assert_eq!(Resolution::new(3840, 2160).label(), "2160p");
        assert_eq!(Resolution::new(2560, 1440).label(), "1440p");
        assert_eq!(Resolution::new(1920, 1080).label(), "1080p");
        assert_eq!(Resolution::new(1280, 720).label(), "720p");
        assert_eq!(Resolution::new(854, 480).label(), "480p");
        assert_eq!(Resolution::new(640, 360).label(), "360p");
        assert_eq!(Resolution::new(426, 240).label(), "240p");
        assert_eq!(Resolution::new(320, 200).label(), "200p");
    }

    #[test]
    fn test_resolution_display() {
        assert_eq!(format!("{}", Resolution::new(1920, 1080)), "1920x1080");
        assert_eq!(format!("{}", Resolution::new(640, 360)), "640x360");
    }

    #[test]
    fn test_resolution_from_str() {
        assert_eq!("1080p".parse::<Resolution>().unwrap(), RES_1080P);
        assert_eq!("720p".parse::<Resolution>().unwrap(), RES_720P);
        assert_eq!("480p".parse::<Resolution>().unwrap(), RES_480P);
        assert_eq!("360p".parse::<Resolution>().unwrap(), RES_360P);
        assert_eq!("240p".parse::<Resolution>().unwrap(), RES_240P);
        assert_eq!("1440p".parse::<Resolution>().unwrap(), RES_1440P);
        assert_eq!("2160p".parse::<Resolution>().unwrap(), RES_2160P);
        assert_eq!("4k".parse::<Resolution>().unwrap(), RES_2160P);
        assert_eq!("1920x1080".parse::<Resolution>().unwrap(), RES_1080P);
        assert_eq!("640x360".parse::<Resolution>().unwrap(), RES_360P);
        assert!("invalid".parse::<Resolution>().is_err());
    }

    #[test]
    fn test_resolution_serde_roundtrip() {
        let r = RES_1080P;
        let json = serde_json::to_string(&r).unwrap();
        let back: Resolution = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn test_resolution_const_equality() {
        assert_eq!(RES_2160P, Resolution::new(3840, 2160));
        assert_eq!(RES_1440P, Resolution::new(2560, 1440));
        assert_eq!(RES_1080P, Resolution::new(1920, 1080));
        assert_eq!(RES_720P, Resolution::new(1280, 720));
        assert_eq!(RES_480P, Resolution::new(854, 480));
        assert_eq!(RES_360P, Resolution::new(640, 360));
        assert_eq!(RES_240P, Resolution::new(426, 240));
    }

    #[test]
    fn test_rate_control_mode_default() {
        assert_eq!(RateControlMode::default(), RateControlMode::Crf);
    }

    #[test]
    fn test_rate_control_mode_serde() {
        let json = serde_json::to_string(&RateControlMode::Crf).unwrap();
        assert_eq!(json, "\"crf\"");
        let back: RateControlMode = serde_json::from_str("\"vbr\"").unwrap();
        assert_eq!(back, RateControlMode::Vbr);
    }

    #[test]
    fn test_ffmpeg_path_default() {
        let path = ffmpeg_path();
        assert!(!path.is_empty());
    }

    #[test]
    fn test_ffprobe_path_default() {
        let path = ffprobe_path();
        assert!(!path.is_empty());
    }

    #[test]
    fn test_ffmpeg_path_respects_env() {
        // SAFETY: test-only env var manipulation, single-threaded test
        let old = std::env::var("VISER_FFMPEG").ok();
        unsafe {
            std::env::set_var("VISER_FFMPEG", "/custom/ffmpeg");
        }
        assert_eq!(ffmpeg_path(), "/custom/ffmpeg");
        unsafe {
            match old {
                Some(v) => std::env::set_var("VISER_FFMPEG", v),
                None => std::env::remove_var("VISER_FFMPEG"),
            }
        }
    }

    #[test]
    fn test_ffprobe_path_respects_env() {
        // SAFETY: test-only env var manipulation, single-threaded test
        let old = std::env::var("VISER_FFPROBE").ok();
        unsafe {
            std::env::set_var("VISER_FFPROBE", "/custom/ffprobe");
        }
        assert_eq!(ffprobe_path(), "/custom/ffprobe");
        unsafe {
            match old {
                Some(v) => std::env::set_var("VISER_FFPROBE", v),
                None => std::env::remove_var("VISER_FFPROBE"),
            }
        }
    }
}
