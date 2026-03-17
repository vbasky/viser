// Chart generation - placeholder using plotters crate.
// The full Go implementation uses gonum/plot; this is the Rust equivalent structure.
// Full plotters integration can be filled in later.

use veo_hull::{Hull, Point};
use veo_ladder::Ladder;

#[derive(Debug, Clone)]
pub struct Opts {
    pub title: String,
    pub subtitle: String,
    pub width: f64,
    pub height: f64,
    pub format: String,
    pub max_bitrate: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            title: String::new(),
            subtitle: String::new(),
            width: 9.0,
            height: 5.5,
            format: "png".into(),
            max_bitrate: 0.0,
        }
    }
}

/// Generates an R-D curve chart as PNG bytes.
pub fn rd_curve(points: &[Point], _hull: &Hull, opts: Opts) -> anyhow::Result<Vec<u8>> {
    // TODO: Implement with plotters crate
    let _ = (points, opts);
    Ok(vec![])
}

/// Generates per-codec R-D curves as PNG bytes.
pub fn per_codec_rd_curve(
    _per_codec: &std::collections::HashMap<veo_ffmpeg::Codec, Hull>,
    _bd_rate: f64,
    _opts: Opts,
) -> anyhow::Result<Vec<u8>> {
    // TODO: Implement with plotters crate
    Ok(vec![])
}

/// Generates a ladder bar chart as PNG bytes.
pub fn ladder_chart(_ladder: &Ladder, _opts: Opts) -> anyhow::Result<Vec<u8>> {
    // TODO: Implement with plotters crate
    Ok(vec![])
}

/// Saves chart bytes to a file.
pub fn save_chart(data: &[u8], path: &str) -> anyhow::Result<()> {
    std::fs::write(path, data)?;
    Ok(())
}

pub fn short_codec_name(codec: &str) -> &str {
    match codec {
        "libx264" => "H.264",
        "libx265" => "H.265",
        "libsvtav1" => "AV1",
        "libvpx-vp9" => "VP9",
        _ => codec,
    }
}
