use serde::{Deserialize, Serialize};
use std::fmt;

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
