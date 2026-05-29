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
