mod bdrate;

pub use bdrate::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::{Codec, Resolution};

/// A single encoding trial result in R-D space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub resolution: Resolution,
    pub codec: Codec,
    pub crf: i32,
    pub bitrate: f64, // kbps
    pub vmaf: f64,    // 0-100
    pub psnr: f64,    // dB (optional)
    pub ssim: f64,    // 0-1 (optional)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hull {
    /// Sorted by bitrate ascending.
    pub points: Vec<Point>,
}

/// Resolution transition point on the hull.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crossover {
    pub from: Resolution,
    pub to: Resolution,
    pub bitrate: f64, // approximate bitrate of crossover
    pub vmaf: f64,    // approximate quality at crossover
}

/// Computes the upper convex hull of the given R-D points.
///
/// Uses Andrew's monotone chain algorithm adapted for R-D optimization.
/// Time complexity: O(n log n).
pub fn compute_upper(points: &[Point]) -> Hull {
    if points.is_empty() {
        return Hull { points: vec![] };
    }

    let mut sorted: Vec<Point> = points.to_vec();
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

    #[test]
    fn test_compute_upper_empty() {
        let hull = compute_upper(&[]);
        assert!(hull.points.is_empty());
    }

    #[test]
    fn test_compute_upper_single() {
        let hull = compute_upper(&[point(1000.0, 90.0)]);
        assert_eq!(hull.points.len(), 1);
    }

    #[test]
    fn test_compute_upper_filters_dominated() {
        let points = vec![
            point(500.0, 80.0),
            point(1000.0, 70.0), // dominated by first point
            point(1500.0, 95.0),
        ];
        let hull = compute_upper(&points);
        // The dominated point (higher bitrate, lower quality) should be removed
        assert_eq!(hull.points.len(), 2);
        assert_eq!(hull.points[0].bitrate, 500.0);
        assert_eq!(hull.points[1].bitrate, 1500.0);
    }
}
