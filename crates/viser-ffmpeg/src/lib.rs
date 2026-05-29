mod cache;
mod encode;
mod path;
mod probe;

pub use cache::*;
pub use encode::*;
pub use path::*;
pub use probe::*;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported video codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Codec {
    #[serde(rename = "libx264")]
    X264,
    #[serde(rename = "libx265")]
    X265,
    #[serde(rename = "libsvtav1")]
    SvtAv1,
}

impl Codec {
    pub fn as_str(&self) -> &'static str {
        match self {
            Codec::X264 => "libx264",
            Codec::X265 => "libx265",
            Codec::SvtAv1 => "libsvtav1",
        }
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
            _ => Err(anyhow::anyhow!("unknown codec: {s}")),
        }
    }
}

/// Video resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
}

impl Resolution {
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

/// Common resolutions (16:9 aspect ratio).
pub const RES_2160P: Resolution = Resolution::new(3840, 2160);
pub const RES_1440P: Resolution = Resolution::new(2560, 1440);
pub const RES_1080P: Resolution = Resolution::new(1920, 1080);
pub const RES_720P: Resolution = Resolution::new(1280, 720);
pub const RES_480P: Resolution = Resolution::new(854, 480);
pub const RES_360P: Resolution = Resolution::new(640, 360);
pub const RES_240P: Resolution = Resolution::new(426, 240);

/// Rate control mode for encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RateControlMode {
    /// Constant rate factor (default).
    #[default]
    Crf,
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
    }

    #[test]
    fn test_codec_display() {
        assert_eq!(format!("{}", Codec::X264), "libx264");
        assert_eq!(format!("{}", Codec::X265), "libx265");
        assert_eq!(format!("{}", Codec::SvtAv1), "libsvtav1");
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
        assert!("unknown".parse::<Codec>().is_err());
    }

    #[test]
    fn test_codec_serde_roundtrip() {
        for codec in &[Codec::X264, Codec::X265, Codec::SvtAv1] {
            let json = serde_json::to_string(codec).unwrap();
            let back: Codec = serde_json::from_str(&json).unwrap();
            assert_eq!(*codec, back);
        }
    }

    #[test]
    fn test_codec_eq() {
        assert_eq!(Codec::X264, Codec::X264);
        assert_ne!(Codec::X264, Codec::X265);
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
        unsafe { std::env::set_var("VISER_FFMPEG", "/custom/ffmpeg"); }
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
        unsafe { std::env::set_var("VISER_FFPROBE", "/custom/ffprobe"); }
        assert_eq!(ffprobe_path(), "/custom/ffprobe");
        unsafe {
            match old {
                Some(v) => std::env::set_var("VISER_FFPROBE", v),
                None => std::env::remove_var("VISER_FFPROBE"),
            }
        }
    }
}
