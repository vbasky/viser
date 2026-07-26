//! FFmpeg/FFprobe **engine** for the `viser` video-encoding-optimizer workspace.
//!
//! This crate implements [`viser_engine::VideoEngine`] for FFmpeg/FFprobe and
//! re-exports the shared engine-agnostic types so existing `use viser_ffmpeg::…`
//! imports keep working.
//!
//! Prefer depending on `viser-engine` for types and the [`VideoEngine`] trait when
//! writing new code. Register this backend at startup:
//!
//! ```ignore
//! viser_engine::set_default_engine(viser_ffmpeg::ffmpeg_engine());
//! ```

mod cache;
mod color;
mod encode;
mod engine;
mod hdr;
mod hw_encode;
mod path;
mod probe;
#[cfg(feature = "revelo")]
mod probe_revelo;
mod resolve;
#[cfg(feature = "revelo")]
pub use probe_revelo::probe as probe_revelo;

pub use cache::*;
pub use color::*;
pub use encode::*;
pub use engine::*;
pub use hdr::*;
pub use hw_encode::*;
pub use path::*;
pub use probe::*;
pub use resolve::*;

// ── Engine-agnostic types (defined in `viser-engine`, re-exported for compat) ──
pub use viser_engine::{
    Codec, CodecFamily, DynEngine, EncodeJob, EncodeResult, EncoderBackend, EngineCapabilities,
    FormatInfo, Hdr10Metadata, MasteringDisplay, ProbeResult, Progress, RES_240P, RES_360P,
    RES_480P, RES_720P, RES_1080P, RES_1440P, RES_2160P, RateControlMode, Resolution, SourceFormat,
    StreamInfo, VideoEngine, bit_depth, chunk_plan, codec_supports_bit_depth, hw_pix_fmt,
    psnr_peak, yuv420p_for_depth,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_as_str() {
        assert_eq!(Codec::X264.as_str(), "libx264");
        assert_eq!(Codec::X265.as_str(), "libx265");
        assert_eq!(Codec::SvtAv1.as_str(), "libsvtav1");
        assert_eq!(Codec::Vp9.as_str(), "libvpx-vp9");
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
        assert_eq!("libvpx-vp9".parse::<Codec>().unwrap(), Codec::Vp9);
        assert_eq!("vp9".parse::<Codec>().unwrap(), Codec::Vp9);
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
        assert_eq!("mlvc".parse::<Codec>().unwrap(), Codec::External);
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
        assert_eq!(Codec::External.backend(), EncoderBackend::External);
    }

    #[test]
    fn test_codec_family() {
        assert_eq!(Codec::X264.family(), CodecFamily::H264);
        assert_eq!(Codec::NvencH264.family(), CodecFamily::H264);
        assert_eq!(Codec::X265.family(), CodecFamily::H265);
        assert_eq!(Codec::NvencH265.family(), CodecFamily::H265);
        assert_eq!(Codec::SvtAv1.family(), CodecFamily::Av1);
        assert_eq!(Codec::Vp9.family(), CodecFamily::Vp9);
        assert_eq!(Codec::External.family(), CodecFamily::Other);
    }

    #[test]
    fn test_codec_is_hardware() {
        assert!(!Codec::X264.is_hardware());
        assert!(!Codec::X265.is_hardware());
        assert!(!Codec::SvtAv1.is_hardware());
        assert!(Codec::NvencH264.is_hardware());
        assert!(Codec::QsvH265.is_hardware());
        assert!(Codec::VideoToolboxH264.is_hardware());
        assert!(!Codec::External.is_hardware());
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
            Codec::External,
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
