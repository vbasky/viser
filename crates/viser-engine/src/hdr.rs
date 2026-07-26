use serde::{Deserialize, Serialize};

/// SMPTE ST 2086 mastering-display colour volume.
///
/// Chromaticity coordinates are stored in units of `1/50000` and luminance in
/// units of `0.0001 cd/m²` — the integer encoding classical encoders (x265)
/// expect for `master-display` parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasteringDisplay {
    /// Green primary x, in 1/50000 units.
    pub green_x: u32,
    /// Green primary y, in 1/50000 units.
    pub green_y: u32,
    /// Blue primary x, in 1/50000 units.
    pub blue_x: u32,
    /// Blue primary y, in 1/50000 units.
    pub blue_y: u32,
    /// Red primary x, in 1/50000 units.
    pub red_x: u32,
    /// Red primary y, in 1/50000 units.
    pub red_y: u32,
    /// White point x, in 1/50000 units.
    pub white_x: u32,
    /// White point y, in 1/50000 units.
    pub white_y: u32,
    /// Maximum display luminance, in 0.0001 cd/m² units.
    pub max_luminance: u32,
    /// Minimum display luminance, in 0.0001 cd/m² units.
    pub min_luminance: u32,
}

const CHROMA_UNIT: f64 = 50000.0;
const LUMA_UNIT: f64 = 10000.0;

impl MasteringDisplay {
    /// Formats as an x265 `master-display` value.
    pub fn to_x265_string(&self) -> String {
        format!(
            "G({},{})B({},{})R({},{})WP({},{})L({},{})",
            self.green_x,
            self.green_y,
            self.blue_x,
            self.blue_y,
            self.red_x,
            self.red_y,
            self.white_x,
            self.white_y,
            self.max_luminance,
            self.min_luminance,
        )
    }

    /// Formats as an SVT-AV1 `mastering-display` value (real chromaticity / cd/m²).
    pub fn to_svtav1_string(&self) -> String {
        let c = |u: u32| trim_float(u as f64 / CHROMA_UNIT);
        let l = |u: u32| trim_float(u as f64 / LUMA_UNIT);
        format!(
            "G({},{})B({},{})R({},{})WP({},{})L({},{})",
            c(self.green_x),
            c(self.green_y),
            c(self.blue_x),
            c(self.blue_y),
            c(self.red_x),
            c(self.red_y),
            c(self.white_x),
            c(self.white_y),
            l(self.max_luminance),
            l(self.min_luminance),
        )
    }
}

fn trim_float(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// HDR10 static metadata extracted from a source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hdr10Metadata {
    /// Mastering-display colour volume, when present.
    pub mastering_display: Option<MasteringDisplay>,
    /// Maximum content light level (MaxCLL), in cd/m².
    pub max_cll: Option<u32>,
    /// Maximum frame-average light level (MaxFALL), in cd/m².
    pub max_fall: Option<u32>,
}

impl Hdr10Metadata {
    /// Returns `true` when no usable metadata was found.
    pub fn is_empty(&self) -> bool {
        self.mastering_display.is_none() && self.max_cll.is_none()
    }
}
