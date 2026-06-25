//! HDR10 static metadata: mastering-display colour volume (SMPTE ST 2086) and
//! content light level (MaxCLL / MaxFALL, CTA-861.3).
//!
//! ffprobe does not surface these as top-level stream fields; they ride as
//! per-frame side data (HEVC SEI / AV1 OBU metadata). [`probe_hdr10_metadata`]
//! reads the first video frame's `side_data_list` and normalises the values
//! into the integer units x265 expects for its `master-display` / `max-cll`
//! parameters, so HDR10 signalling survives a re-encode.

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::ffprobe_path;

/// SMPTE ST 2086 mastering-display colour volume.
///
/// Chromaticity coordinates are stored in units of `1/50000` and luminance in
/// units of `0.0001 cd/m²` — the exact integer encoding x265's `master-display`
/// parameter consumes.
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

impl MasteringDisplay {
    /// Formats as an x265 `master-display` value:
    /// `G(gx,gy)B(bx,by)R(rx,ry)WP(wx,wy)L(max,min)`, with chromaticity in
    /// 1/50000 units and luminance in 0.0001 cd/m² units (the stored encoding).
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

    /// Formats as an SVT-AV1 `mastering-display` value. SVT-AV1 shares x265's
    /// `G()B()R()WP()L()` grammar but expects **real** chromaticity coordinates
    /// in `[0,1]` and luminance in cd/m² — not x265's scaled integers. Feeding
    /// it the x265 integers makes SVT-AV1 clip the values to garbage.
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

/// Formats a float with up to 6 decimals, trimming trailing zeros (and a bare
/// trailing `.`), so `0.265000 → 0.265` and `1000.0 → 1000`.
fn trim_float(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// HDR10 static metadata extracted from a source's first video frame.
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

/// Probes the first video frame for HDR10 static metadata.
///
/// Returns `Ok(None)` when the source carries no mastering-display or
/// content-light side data (e.g. SDR or HLG content). Decodes a single frame,
/// so the cost is negligible.
pub async fn probe_hdr10_metadata(path: &str) -> anyhow::Result<Option<Hdr10Metadata>> {
    let output = Command::new(ffprobe_path())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-read_intervals",
            "%+#1",
            "-show_frames",
            "-print_format",
            "json",
            path,
        ])
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe (hdr10) failed for {path}: {stderr}");
    }

    let parsed: FramesJson = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse ffprobe hdr10 output: {e}"))?;

    Ok(parse_hdr10(&parsed))
}

const MASTERING_DISPLAY: &str = "Mastering display metadata";
const CONTENT_LIGHT: &str = "Content light level metadata";

fn parse_hdr10(frames: &FramesJson) -> Option<Hdr10Metadata> {
    let side_data = &frames.frames.first()?.side_data_list;
    let mut md = Hdr10Metadata::default();

    for sd in side_data {
        match sd.side_data_type.as_str() {
            MASTERING_DISPLAY => md.mastering_display = parse_mastering_display(sd),
            CONTENT_LIGHT => {
                md.max_cll = value_to_u32(&sd.max_content);
                md.max_fall = value_to_u32(&sd.max_average);
            }
            _ => {}
        }
    }

    if md.is_empty() { None } else { Some(md) }
}

/// Chromaticity coordinates are reported as fractions of 1 (e.g. `"13250/50000"`)
/// and scale to 1/50000 units; luminance (e.g. `"10000000/10000"`) scales to
/// 0.0001 cd/m² units.
const CHROMA_UNIT: f64 = 50000.0;
const LUMA_UNIT: f64 = 10000.0;

fn parse_mastering_display(sd: &SideData) -> Option<MasteringDisplay> {
    Some(MasteringDisplay {
        green_x: scaled(&sd.green_x, CHROMA_UNIT)?,
        green_y: scaled(&sd.green_y, CHROMA_UNIT)?,
        blue_x: scaled(&sd.blue_x, CHROMA_UNIT)?,
        blue_y: scaled(&sd.blue_y, CHROMA_UNIT)?,
        red_x: scaled(&sd.red_x, CHROMA_UNIT)?,
        red_y: scaled(&sd.red_y, CHROMA_UNIT)?,
        white_x: scaled(&sd.white_point_x, CHROMA_UNIT)?,
        white_y: scaled(&sd.white_point_y, CHROMA_UNIT)?,
        max_luminance: scaled(&sd.max_luminance, LUMA_UNIT)?,
        min_luminance: scaled(&sd.min_luminance, LUMA_UNIT)?,
    })
}

/// Parses a `"num/den"` rational (or plain decimal) into `f64`.
fn rational(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((num, den)) = s.split_once('/') {
        let num: f64 = num.trim().parse().ok()?;
        let den: f64 = den.trim().parse().ok()?;
        if den == 0.0 {
            return None;
        }
        Some(num / den)
    } else {
        s.parse().ok()
    }
}

/// Parses a rational and scales it into the integer unit x265 expects.
fn scaled(s: &str, unit: f64) -> Option<u32> {
    rational(s).map(|v| (v * unit).round().max(0.0) as u32)
}

/// ffprobe emits MaxCLL/MaxFALL as JSON numbers; older builds use strings.
fn value_to_u32(v: &Option<serde_json::Value>) -> Option<u32> {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|x| x as u32),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

#[derive(Deserialize)]
struct FramesJson {
    #[serde(default)]
    frames: Vec<FrameJson>,
}

#[derive(Deserialize)]
struct FrameJson {
    #[serde(default)]
    side_data_list: Vec<SideData>,
}

#[derive(Deserialize)]
struct SideData {
    #[serde(default)]
    side_data_type: String,
    // Mastering-display fields (rational strings).
    #[serde(default)]
    red_x: String,
    #[serde(default)]
    red_y: String,
    #[serde(default)]
    green_x: String,
    #[serde(default)]
    green_y: String,
    #[serde(default)]
    blue_x: String,
    #[serde(default)]
    blue_y: String,
    #[serde(default)]
    white_point_x: String,
    #[serde(default)]
    white_point_y: String,
    #[serde(default)]
    min_luminance: String,
    #[serde(default)]
    max_luminance: String,
    // Content-light fields (numbers, occasionally strings).
    #[serde(default)]
    max_content: Option<serde_json::Value>,
    #[serde(default)]
    max_average: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // A representative ffprobe `-show_frames` payload for BT.2020 HDR10 content
    // mastered on a P3-D65 display at 1000 nits, MaxCLL 1000 / MaxFALL 400.
    const SAMPLE: &str = r#"{
        "frames": [{
            "side_data_list": [
                {
                    "side_data_type": "Mastering display metadata",
                    "red_x": "34000/50000", "red_y": "16000/50000",
                    "green_x": "13250/50000", "green_y": "34500/50000",
                    "blue_x": "7500/50000", "blue_y": "3000/50000",
                    "white_point_x": "15635/50000", "white_point_y": "16450/50000",
                    "min_luminance": "50/10000", "max_luminance": "10000000/10000"
                },
                {
                    "side_data_type": "Content light level metadata",
                    "max_content": 1000, "max_average": 400
                }
            ]
        }]
    }"#;

    #[test]
    fn test_parse_full_hdr10() {
        let frames: FramesJson = serde_json::from_str(SAMPLE).unwrap();
        let md = parse_hdr10(&frames).expect("metadata present");
        let display = md.mastering_display.expect("mastering display present");
        assert_eq!(display.green_x, 13250);
        assert_eq!(display.green_y, 34500);
        assert_eq!(display.red_x, 34000);
        assert_eq!(display.white_x, 15635);
        assert_eq!(display.max_luminance, 10_000_000);
        assert_eq!(display.min_luminance, 50);
        assert_eq!(md.max_cll, Some(1000));
        assert_eq!(md.max_fall, Some(400));
    }

    #[test]
    fn test_to_x265_format() {
        let frames: FramesJson = serde_json::from_str(SAMPLE).unwrap();
        let md = parse_hdr10(&frames).unwrap().mastering_display.unwrap();
        assert_eq!(
            md.to_x265_string(),
            "G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50)"
        );
    }

    #[test]
    fn test_to_svtav1_format() {
        let frames: FramesJson = serde_json::from_str(SAMPLE).unwrap();
        let md = parse_hdr10(&frames).unwrap().mastering_display.unwrap();
        // Real chromaticity coordinates (0..1) and luminance in cd/m².
        assert_eq!(
            md.to_svtav1_string(),
            "G(0.265,0.69)B(0.15,0.06)R(0.68,0.32)WP(0.3127,0.329)L(1000,0.005)"
        );
    }

    #[test]
    fn test_trim_float() {
        assert_eq!(trim_float(0.265), "0.265");
        assert_eq!(trim_float(1000.0), "1000");
        assert_eq!(trim_float(0.005), "0.005");
        assert_eq!(trim_float(0.0), "0");
    }

    #[test]
    fn test_sdr_returns_none() {
        let frames: FramesJson =
            serde_json::from_str(r#"{"frames":[{"side_data_list":[]}]}"#).unwrap();
        assert!(parse_hdr10(&frames).is_none());
    }

    #[test]
    fn test_content_light_only() {
        let json = r#"{"frames":[{"side_data_list":[
            {"side_data_type":"Content light level metadata","max_content":600,"max_average":120}
        ]}]}"#;
        let frames: FramesJson = serde_json::from_str(json).unwrap();
        let md = parse_hdr10(&frames).expect("cll present");
        assert!(md.mastering_display.is_none());
        assert_eq!(md.max_cll, Some(600));
        assert_eq!(md.max_fall, Some(120));
    }

    #[test]
    fn test_content_light_string_values() {
        let json = r#"{"frames":[{"side_data_list":[
            {"side_data_type":"Content light level metadata","max_content":"600","max_average":"120"}
        ]}]}"#;
        let frames: FramesJson = serde_json::from_str(json).unwrap();
        let md = parse_hdr10(&frames).unwrap();
        assert_eq!(md.max_cll, Some(600));
        assert_eq!(md.max_fall, Some(120));
    }

    #[test]
    fn test_no_frames_returns_none() {
        let frames: FramesJson = serde_json::from_str(r#"{"frames":[]}"#).unwrap();
        assert!(parse_hdr10(&frames).is_none());
    }

    #[test]
    fn test_rational_parsing() {
        assert_eq!(rational("1/2"), Some(0.5));
        assert_eq!(rational("100"), Some(100.0));
        assert_eq!(rational("5/0"), None);
        assert_eq!(rational(""), None);
    }

    #[test]
    fn test_is_empty() {
        assert!(Hdr10Metadata::default().is_empty());
        let md = Hdr10Metadata { max_cll: Some(1000), ..Default::default() };
        assert!(!md.is_empty());
    }
}
