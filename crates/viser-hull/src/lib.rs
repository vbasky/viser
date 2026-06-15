//! Convex hull (Pareto frontier) and Bjontegaard Delta Rate (BD-Rate) computation.
//!
//! Computes the upper convex hull of rate-distortion (R-D) points and detects the
//! bitrates at which the optimal encoding resolution changes (crossovers). Also provides
//! BD-Rate, the standard metric for comparing the efficiency of two R-D curves.
//!
//! Part of the `viser` video-encoding-optimizer workspace.

mod bdrate;

pub use bdrate::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::{Codec, Resolution};

/// A single encoding trial result in R-D space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    /// Encoding resolution of this trial.
    pub resolution: Resolution,
    /// Codec used for this trial.
    pub codec: Codec,
    /// Constant Rate Factor (quality/rate setting) used for the encode.
    pub crf: i32,
    /// Measured bitrate in kbps.
    pub bitrate: f64, // kbps
    /// Measured VMAF quality score (0-100).
    pub vmaf: f64, // 0-100
    /// Measured PSNR in dB (optional; 0 if unmeasured).
    pub psnr: f64, // dB (optional)
    /// Measured SSIM (0-1, optional; 0 if unmeasured).
    pub ssim: f64, // 0-1 (optional)
}

/// Upper convex hull of R-D points, i.e. the Pareto-optimal frontier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hull {
    /// Hull points, sorted by bitrate ascending.
    pub points: Vec<Point>,
}

/// Resolution transition point on the hull.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crossover {
    /// Resolution optimal below the crossover bitrate.
    pub from: Resolution,
    /// Resolution optimal above the crossover bitrate.
    pub to: Resolution,
    /// Approximate bitrate (kbps) at which the optimal resolution changes.
    pub bitrate: f64, // approximate bitrate of crossover
    /// Approximate VMAF quality at the crossover.
    pub vmaf: f64, // approximate quality at crossover
}

/// Computes the upper convex hull of the given R-D points.
///
/// Uses Andrew's monotone chain algorithm adapted for R-D optimization.
/// Time complexity: O(n log n).
pub fn compute_upper(points: &[Point]) -> Hull {
    let mut sorted: Vec<Point> =
        points.iter().filter(|p| p.bitrate.is_finite() && p.vmaf.is_finite()).cloned().collect();
    if sorted.is_empty() {
        return Hull { points: vec![] };
    }

    sorted.sort_by(|a, b| {
        a.bitrate.partial_cmp(&b.bitrate).unwrap().then(b.vmaf.partial_cmp(&a.vmaf).unwrap())
    });

    let mut hull: Vec<Point> = Vec::new();
    for p in sorted {
        while hull.len() >= 2 && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], &p) >= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }

    Hull { points: hull }
}

/// Computes a separate upper hull for each codec.
pub fn compute_per_codec(points: &[Point]) -> std::collections::HashMap<Codec, Hull> {
    let mut by_codec: std::collections::HashMap<Codec, Vec<Point>> =
        std::collections::HashMap::new();
    for p in points {
        by_codec.entry(p.codec).or_default().push(p.clone());
    }

    by_codec.into_iter().map(|(codec, pts)| (codec, compute_upper(&pts))).collect()
}

impl Hull {
    /// Returns the bitrate values at which the optimal resolution changes.
    pub fn crossovers(&self) -> Vec<Crossover> {
        if self.points.len() < 2 {
            return vec![];
        }

        let mut crossovers = Vec::new();
        for i in 1..self.points.len() {
            let prev = &self.points[i - 1];
            let curr = &self.points[i];
            if prev.resolution != curr.resolution {
                crossovers.push(Crossover {
                    from: prev.resolution,
                    to: curr.resolution,
                    bitrate: (prev.bitrate + curr.bitrate) / 2.0,
                    vmaf: (prev.vmaf + curr.vmaf) / 2.0,
                });
            }
        }
        crossovers
    }
}

/// Cross product of vectors OA and OB for upper hull construction.
fn cross(o: &Point, a: &Point, b: &Point) -> f64 {
    (a.bitrate - o.bitrate) * (b.vmaf - o.vmaf) - (a.vmaf - o.vmaf) * (b.bitrate - o.bitrate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::{Codec, Resolution};

    fn point(bitrate: f64, vmaf: f64) -> Point {
        Point {
            resolution: Resolution::new(1920, 1080),
            codec: Codec::X264,
            crf: 23,
            bitrate,
            vmaf,
            psnr: 0.0,
            ssim: 0.0,
        }
    }

    fn point_with_resolution(bitrate: f64, vmaf: f64, width: i32, height: i32) -> Point {
        Point {
            resolution: Resolution::new(width, height),
            codec: Codec::X264,
            crf: 23,
            bitrate,
            vmaf,
            psnr: 0.0,
            ssim: 0.0,
        }
    }

    #[test]
    fn test_compute_upper_empty() {
        let hull = compute_upper(&[]);
        assert!(hull.points.is_empty());
    }

    #[test]
    fn test_compute_upper_single() {
        let hull = compute_upper(&[point(1000.0, 90.0)]);
        assert_eq!(hull.points.len(), 1);
        assert_eq!(hull.points[0].bitrate, 1000.0);
    }

    #[test]
    fn test_compute_upper_filters_dominated() {
        let points = vec![
            point(500.0, 80.0),
            point(1000.0, 70.0), // dominated: same quality for more bitrate? Wait no - 1000.0 bitrate higher but vmaf lower. So 500@80 dominates 1000@70.
            point(1500.0, 95.0),
        ];
        let hull = compute_upper(&points);
        assert_eq!(hull.points.len(), 2);
        assert_eq!(hull.points[0].bitrate, 500.0);
        assert_eq!(hull.points[1].bitrate, 1500.0);
    }

    #[test]
    fn test_compute_upper_monotonic() {
        let points =
            vec![point(100.0, 50.0), point(500.0, 70.0), point(1500.0, 90.0), point(5000.0, 98.0)];
        let hull = compute_upper(&points);
        assert_eq!(hull.points.len(), 4);
    }

    #[test]
    fn test_compute_upper_interior_removed() {
        let pts = vec![
            point(100.0, 40.0),
            point(300.0, 80.0),
            point(500.0, 70.0),
            point(800.0, 85.0),
            point(2000.0, 95.0),
        ];
        let hull = compute_upper(&pts);
        // 500@70 is interior (below the line from 300@80 to 800@85) and should be filtered
        assert!(hull.points.iter().any(|p| p.bitrate == 100.0));
        assert!(hull.points.iter().any(|p| p.bitrate == 300.0));
        assert!(hull.points.iter().any(|p| p.bitrate == 800.0));
        assert!(hull.points.iter().any(|p| p.bitrate == 2000.0));
        assert!(!hull.points.iter().any(|p| p.bitrate == 500.0));
        assert_eq!(hull.points.len(), 4);
    }

    #[test]
    fn test_compute_upper_unsorted_input() {
        let points =
            vec![point(5000.0, 98.0), point(100.0, 50.0), point(1500.0, 90.0), point(500.0, 70.0)];
        let hull = compute_upper(&points);
        assert!(hull.points.windows(2).all(|w| w[0].bitrate <= w[1].bitrate));
    }

    #[test]
    fn test_compute_upper_filters_non_finite() {
        let points = vec![
            point(100.0, 50.0),
            point(500.0, f64::NAN),
            point(f64::INFINITY, 90.0),
            point(1000.0, 95.0),
            point(2000.0, f64::NEG_INFINITY),
        ];
        let hull = compute_upper(&points);
        // NaN/Inf points are dropped; valid points form the hull.
        assert_eq!(hull.points.len(), 2);
        assert_eq!(hull.points[0].bitrate, 100.0);
        assert_eq!(hull.points[1].bitrate, 1000.0);
    }

    #[test]
    fn test_compute_upper_all_non_finite_returns_empty() {
        let points = vec![point(f64::NAN, 50.0), point(100.0, f64::NAN)];
        let hull = compute_upper(&points);
        assert!(hull.points.is_empty());
    }

    #[test]
    fn test_compute_per_codec() {
        let points = vec![
            Point {
                resolution: Resolution::new(1920, 1080),
                codec: Codec::X264,
                crf: 23,
                bitrate: 1000.0,
                vmaf: 90.0,
                psnr: 0.0,
                ssim: 0.0,
            },
            Point {
                resolution: Resolution::new(1920, 1080),
                codec: Codec::X265,
                crf: 28,
                bitrate: 800.0,
                vmaf: 90.0,
                psnr: 0.0,
                ssim: 0.0,
            },
            Point {
                resolution: Resolution::new(1920, 1080),
                codec: Codec::X264,
                crf: 23,
                bitrate: 2000.0,
                vmaf: 95.0,
                psnr: 0.0,
                ssim: 0.0,
            },
        ];
        let hulls = compute_per_codec(&points);
        assert_eq!(hulls.len(), 2);
        assert!(hulls.contains_key(&Codec::X264));
        assert!(hulls.contains_key(&Codec::X265));
        assert_eq!(hulls[&Codec::X264].points.len(), 2);
        assert_eq!(hulls[&Codec::X265].points.len(), 1);
    }

    #[test]
    fn test_crossovers_empty() {
        let hull = Hull { points: vec![] };
        assert!(hull.crossovers().is_empty());
    }

    #[test]
    fn test_crossovers_no_change() {
        let hull = Hull { points: vec![point(500.0, 80.0), point(1000.0, 90.0)] };
        assert!(hull.crossovers().is_empty());
    }

    #[test]
    fn test_crossovers_detected() {
        let hull = Hull {
            points: vec![
                point_with_resolution(500.0, 70.0, 1280, 720),
                point_with_resolution(1000.0, 85.0, 1920, 1080),
                point_with_resolution(2000.0, 95.0, 1920, 1080),
            ],
        };
        let xs = hull.crossovers();
        assert_eq!(xs.len(), 1);
        assert_eq!(xs[0].from, Resolution::new(1280, 720));
        assert_eq!(xs[0].to, Resolution::new(1920, 1080));
        assert!((xs[0].bitrate - 750.0).abs() < 1e-9);
        assert!((xs[0].vmaf - 77.5).abs() < 1e-9);
    }

    #[test]
    fn test_bd_rate_needs_at_least_4() {
        let a = vec![point(100.0, 70.0), point(500.0, 80.0), point(1000.0, 90.0)];
        let b = vec![point(100.0, 70.0), point(500.0, 80.0), point(1000.0, 90.0)];
        assert!(bd_rate(&a, &b).is_err());
    }

    #[test]
    fn test_bd_rate_identical_curves() {
        let a =
            vec![point(100.0, 60.0), point(300.0, 70.0), point(800.0, 80.0), point(2000.0, 90.0)];
        let b = a.clone();
        let result = bd_rate(&a, &b).unwrap();
        assert!((result).abs() < 1.0, "identical curves should give ~0% BD-Rate, got {result}");
    }

    #[test]
    fn test_bd_rate_negative_efficient() {
        // Curve B has lower bitrates for same quality -> negative BD-Rate
        let a =
            vec![point(200.0, 60.0), point(500.0, 70.0), point(1200.0, 80.0), point(3000.0, 90.0)];
        let b =
            vec![point(150.0, 60.0), point(400.0, 70.0), point(1000.0, 80.0), point(2500.0, 90.0)];
        let result = bd_rate(&a, &b).unwrap();
        assert!(result < 0.0, "more efficient codec should have negative BD-Rate, got {result}");
    }

    #[test]
    fn test_bd_rate_no_overlap() {
        let a =
            vec![point(100.0, 90.0), point(300.0, 92.0), point(800.0, 95.0), point(2000.0, 98.0)];
        let b =
            vec![point(100.0, 50.0), point(300.0, 55.0), point(800.0, 60.0), point(2000.0, 65.0)];
        assert!(bd_rate(&a, &b).is_err());
    }
}
