use std::collections::HashMap;

use plotters::prelude::*;
use viser_ffmpeg::Codec;
use viser_hull::{Hull, Point};
use viser_ladder::Ladder;

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

const COLORS: [RGBColor; 6] = [
    RGBColor(31, 119, 180),  // blue
    RGBColor(255, 127, 14),  // orange
    RGBColor(44, 160, 44),   // green
    RGBColor(214, 39, 40),   // red
    RGBColor(148, 103, 189), // purple
    RGBColor(140, 86, 75),   // brown
];

fn pixel_dims(opts: &Opts) -> (u32, u32) {
    // Convert inches to pixels at 100 DPI
    ((opts.width * 100.0) as u32, (opts.height * 100.0) as u32)
}

fn bitrate_range(points: &[Point], max_override: f64) -> (f64, f64) {
    let min = points.iter().map(|p| p.bitrate).fold(f64::MAX, f64::min) * 0.9;
    let max = if max_override > 0.0 {
        max_override
    } else {
        points.iter().map(|p| p.bitrate).fold(0.0_f64, f64::max) * 1.1
    };
    (min.max(0.0), max)
}

fn vmaf_range(points: &[Point]) -> (f64, f64) {
    let min = points.iter().map(|p| p.vmaf).fold(f64::MAX, f64::min) - 2.0;
    let max = points.iter().map(|p| p.vmaf).fold(0.0_f64, f64::max) + 2.0;
    (min.max(0.0), max.min(100.0))
}

fn chart_title(opts: &Opts, fallback: &str) -> String {
    if opts.title.is_empty() {
        fallback.to_string()
    } else if opts.subtitle.is_empty() {
        opts.title.clone()
    } else {
        format!("{}\n{}", opts.title, opts.subtitle)
    }
}

/// Generates an R-D curve chart as PNG bytes.
///
/// Plots all encoding trial points colored by resolution, with the convex hull
/// drawn as a connected line. Resolution crossover points are marked.
pub fn rd_curve(points: &[Point], hull: &Hull, opts: Opts) -> anyhow::Result<Vec<u8>> {
    if points.is_empty() {
        return Ok(vec![]);
    }

    let (w, h) = pixel_dims(&opts);
    let mut buf = vec![0u8; (w * h * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut buf, (w, h)).into_drawing_area();
        root.fill(&WHITE)?;

        let (br_min, br_max) = bitrate_range(points, opts.max_bitrate);
        let (vm_min, vm_max) = vmaf_range(points);

        let title = chart_title(&opts, "Rate-Distortion Curve");
        let mut chart = ChartBuilder::on(&root)
            .caption(&title, ("sans-serif", 20).into_font())
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(br_min..br_max, vm_min..vm_max)?;

        chart.configure_mesh().x_desc("Bitrate (kbps)").y_desc("VMAF").draw()?;

        // Group points by resolution for coloring
        let mut by_res: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
        let mut res_map: HashMap<String, usize> = HashMap::new();
        for p in points {
            let label = p.resolution.label();
            let idx = if let Some(&i) = res_map.get(&label) {
                i
            } else {
                let i = by_res.len();
                res_map.insert(label.clone(), i);
                by_res.push((label, Vec::new()));
                i
            };
            by_res[idx].1.push((p.bitrate, p.vmaf));
        }

        // Draw scatter points per resolution
        for (i, (label, pts)) in by_res.iter().enumerate() {
            let color = COLORS[i % COLORS.len()];
            chart
                .draw_series(pts.iter().map(|&(x, y)| Circle::new((x, y), 4, color.filled())))?
                .label(label)
                .legend(move |(x, y)| Circle::new((x, y), 4, color.filled()));
        }

        // Draw convex hull line
        if hull.points.len() >= 2 {
            let hull_pts: Vec<(f64, f64)> =
                hull.points.iter().map(|p| (p.bitrate, p.vmaf)).collect();
            chart.draw_series(LineSeries::new(hull_pts, BLACK.stroke_width(2)))?;
        }

        // Mark crossover points
        let crossovers = hull.crossovers();
        if !crossovers.is_empty() {
            chart.draw_series(
                crossovers
                    .iter()
                    .map(|co| TriangleMarker::new((co.bitrate, co.vmaf), 6, RED.filled())),
            )?;
        }

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.8))
            .border_style(BLACK)
            .position(SeriesLabelPosition::LowerRight)
            .draw()?;

        root.present()?;
    }

    encode_png(&buf, w, h)
}

/// Generates per-codec R-D curves as PNG bytes.
///
/// Each codec gets its own colored hull line with scatter points.
/// BD-Rate savings are shown in the subtitle if non-zero.
pub fn per_codec_rd_curve(
    per_codec: &HashMap<Codec, Hull>,
    bd_rate: f64,
    opts: Opts,
) -> anyhow::Result<Vec<u8>> {
    if per_codec.is_empty() {
        return Ok(vec![]);
    }

    let all_points: Vec<&Point> = per_codec.values().flat_map(|h| &h.points).collect();
    if all_points.is_empty() {
        return Ok(vec![]);
    }

    let (w, h) = pixel_dims(&opts);
    let mut buf = vec![0u8; (w * h * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut buf, (w, h)).into_drawing_area();
        root.fill(&WHITE)?;

        let (br_min, br_max) = bitrate_range_refs(&all_points, opts.max_bitrate);
        let (vm_min, vm_max) = vmaf_range_refs(&all_points);

        let default_title = if bd_rate != 0.0 {
            format!("Per-Codec R-D Curves (BD-Rate: {bd_rate:+.1}%)")
        } else {
            "Per-Codec R-D Curves".to_string()
        };
        let title = chart_title(&opts, &default_title);

        let mut chart = ChartBuilder::on(&root)
            .caption(&title, ("sans-serif", 20).into_font())
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(br_min..br_max, vm_min..vm_max)?;

        chart.configure_mesh().x_desc("Bitrate (kbps)").y_desc("VMAF").draw()?;

        // Sort codecs for deterministic ordering
        let mut codecs: Vec<Codec> = per_codec.keys().copied().collect();
        codecs.sort_by_key(|c| c.as_str().to_string());

        for (i, codec) in codecs.iter().enumerate() {
            let hull = &per_codec[codec];
            if hull.points.is_empty() {
                continue;
            }
            let color = COLORS[i % COLORS.len()];
            let label = short_codec_name(codec.as_str()).to_string();

            // Scatter points
            chart.draw_series(
                hull.points.iter().map(|p| Circle::new((p.bitrate, p.vmaf), 4, color.filled())),
            )?;

            // Hull line
            let line_pts: Vec<(f64, f64)> =
                hull.points.iter().map(|p| (p.bitrate, p.vmaf)).collect();
            chart
                .draw_series(LineSeries::new(line_pts, color.stroke_width(2)))?
                .label(&label)
                .legend(move |(x, y)| {
                    PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
                });
        }

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.8))
            .border_style(BLACK)
            .position(SeriesLabelPosition::LowerRight)
            .draw()?;

        root.present()?;
    }

    encode_png(&buf, w, h)
}

/// Generates a ladder visualization as PNG bytes.
///
/// Horizontal bar chart where each rung shows its bitrate as bar width,
/// labeled with resolution, codec, VMAF, and CRF.
pub fn ladder_chart(ladder: &Ladder, opts: Opts) -> anyhow::Result<Vec<u8>> {
    if ladder.rungs.is_empty() {
        return Ok(vec![]);
    }

    let (w, h) = pixel_dims(&opts);
    let mut buf = vec![0u8; (w * h * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut buf, (w, h)).into_drawing_area();
        root.fill(&WHITE)?;

        let max_br = ladder.rungs.iter().map(|r| r.point.bitrate).fold(0.0_f64, f64::max) * 1.15;
        let n = ladder.rungs.len() as f64;

        let title = chart_title(&opts, "Bitrate Ladder");
        let mut chart = ChartBuilder::on(&root)
            .caption(&title, ("sans-serif", 20).into_font())
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(100)
            .build_cartesian_2d(0.0..max_br, 0.0..n)?;

        chart.configure_mesh().x_desc("Bitrate (kbps)").disable_y_mesh().disable_y_axis().draw()?;

        // Draw horizontal bars
        let bar_height = 0.7;
        for (i, rung) in ladder.rungs.iter().enumerate() {
            let p = &rung.point;
            let y_center = i as f64 + 0.5;
            let y_low = y_center - bar_height / 2.0;
            let y_high = y_center + bar_height / 2.0;

            let color = COLORS[i % COLORS.len()];

            // Bar
            chart.draw_series(std::iter::once(Rectangle::new(
                [(0.0, y_low), (p.bitrate, y_high)],
                color.filled(),
            )))?;

            // Label on the left: resolution + codec
            let label = format!("{} {}", p.resolution.label(), short_codec_name(p.codec.as_str()));
            chart.draw_series(std::iter::once(Text::new(
                label,
                (-5.0_f64, y_center),
                ("sans-serif", 13).into_font().color(&BLACK).transform(FontTransform::None),
            )))?;

            // Label on the bar: bitrate + VMAF
            let info = format!("{:.0} kbps  VMAF {:.1}  CRF {}", p.bitrate, p.vmaf, p.crf);
            let text_x = p.bitrate + max_br * 0.01;
            chart.draw_series(std::iter::once(Text::new(
                info,
                (text_x, y_center),
                ("sans-serif", 12).into_font().color(&BLACK).transform(FontTransform::None),
            )))?;
        }

        root.present()?;
    }

    encode_png(&buf, w, h)
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

// --- helpers ---

fn bitrate_range_refs(points: &[&Point], max_override: f64) -> (f64, f64) {
    let min = points.iter().map(|p| p.bitrate).fold(f64::MAX, f64::min) * 0.9;
    let max = if max_override > 0.0 {
        max_override
    } else {
        points.iter().map(|p| p.bitrate).fold(0.0_f64, f64::max) * 1.1
    };
    (min.max(0.0), max)
}

fn vmaf_range_refs(points: &[&Point]) -> (f64, f64) {
    let min = points.iter().map(|p| p.vmaf).fold(f64::MAX, f64::min) - 2.0;
    let max = points.iter().map(|p| p.vmaf).fold(0.0_f64, f64::max) + 2.0;
    (min.max(0.0), max.min(100.0))
}

/// Encode raw RGB buffer to PNG bytes.
fn encode_png(rgb_buf: &[u8], width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut png_data), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgb_buf)?;
    }
    Ok(png_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::Resolution;

    fn test_point(res: Resolution, codec: Codec, crf: i32, bitrate: f64, vmaf: f64) -> Point {
        Point { resolution: res, codec, crf, bitrate, vmaf, psnr: 0.0, ssim: 0.0 }
    }

    #[test]
    fn rd_curve_empty_returns_empty() {
        let hull = Hull { points: vec![] };
        let result = rd_curve(&[], &hull, Opts::default()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn rd_curve_produces_png() {
        let points = vec![
            test_point(Resolution::new(1280, 720), Codec::X264, 23, 500.0, 80.0),
            test_point(Resolution::new(1280, 720), Codec::X264, 28, 300.0, 70.0),
            test_point(Resolution::new(1920, 1080), Codec::X264, 23, 1200.0, 92.0),
            test_point(Resolution::new(1920, 1080), Codec::X264, 28, 800.0, 85.0),
        ];
        let hull = viser_hull::compute_upper(&points);
        let result = rd_curve(&points, &hull, Opts::default()).unwrap();
        assert!(!result.is_empty());
        // Check PNG magic bytes
        assert_eq!(&result[..4], &[137, 80, 78, 71]);
    }

    #[test]
    fn per_codec_rd_curve_produces_png() {
        let points = vec![
            test_point(Resolution::new(1280, 720), Codec::X264, 23, 600.0, 82.0),
            test_point(Resolution::new(1280, 720), Codec::X265, 23, 400.0, 84.0),
            test_point(Resolution::new(1280, 720), Codec::X264, 28, 350.0, 72.0),
            test_point(Resolution::new(1280, 720), Codec::X265, 28, 250.0, 74.0),
        ];
        let per_codec = viser_hull::compute_per_codec(&points);
        let result = per_codec_rd_curve(&per_codec, -15.2, Opts::default()).unwrap();
        assert!(!result.is_empty());
        assert_eq!(&result[..4], &[137, 80, 78, 71]);
    }

    #[test]
    fn ladder_chart_produces_png() {
        let ladder = Ladder {
            rungs: vec![
                viser_ladder::Rung {
                    point: test_point(Resolution::new(640, 360), Codec::X264, 32, 250.0, 55.0),
                    index: 0,
                },
                viser_ladder::Rung {
                    point: test_point(Resolution::new(1280, 720), Codec::X264, 26, 800.0, 78.0),
                    index: 1,
                },
                viser_ladder::Rung {
                    point: test_point(Resolution::new(1920, 1080), Codec::X265, 23, 2000.0, 92.0),
                    index: 2,
                },
            ],
        };
        let result = ladder_chart(&ladder, Opts::default()).unwrap();
        assert!(!result.is_empty());
        assert_eq!(&result[..4], &[137, 80, 78, 71]);
    }
}
