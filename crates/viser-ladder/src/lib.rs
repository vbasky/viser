mod fixed;

pub use fixed::*;

use serde::{Deserialize, Serialize};
use viser_ffmpeg::Resolution;
use viser_hull::{Hull, Point};

/// One level in a bitrate ladder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rung {
    #[serde(flatten)]
    pub point: Point,
    pub index: i32, // rung number (0 = lowest quality)
}

/// Ordered set of rungs from lowest to highest quality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ladder {
    pub rungs: Vec<Rung>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opts {
    pub num_rungs: i32,   // target number of rungs (e.g., 6)
    pub min_bitrate: f64, // minimum bitrate in kbps
    pub max_bitrate: f64, // maximum bitrate in kbps
    pub min_vmaf: f64,    // minimum acceptable quality
    pub max_vmaf: f64,    // maximum quality target
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            num_rungs: 6,
            min_bitrate: 200.0,
            max_bitrate: 8000.0,
            min_vmaf: 40.0,
            max_vmaf: 97.0,
        }
    }
}

/// Picks the best N rungs from the convex hull to form a bitrate ladder.
pub fn select(h: &Hull, opts: &Opts) -> Ladder {
    if h.points.is_empty() || opts.num_rungs <= 0 {
        return Ladder { rungs: vec![] };
    }

    // Build crossover map
    let crossover_min = build_crossover_map(h);

    // Filter hull points by constraints + crossover enforcement
    let mut candidates: Vec<Point> = Vec::new();
    for p in &h.points {
        if p.bitrate < opts.min_bitrate || p.bitrate > opts.max_bitrate {
            continue;
        }
        if p.vmaf < opts.min_vmaf {
            continue;
        }
        if let Some(&min_br) = crossover_min.get(&p.resolution) {
            if p.bitrate < min_br {
                continue;
            }
        }
        candidates.push(p.clone());
    }

    if candidates.is_empty() {
        return Ladder { rungs: vec![] };
    }

    if candidates.len() <= opts.num_rungs as usize {
        return to_ladder(candidates);
    }

    // Determine VMAF range from candidates
    let min_q = candidates.first().map(|p| p.vmaf).unwrap_or(0.0);
    let mut max_q = candidates.last().map(|p| p.vmaf).unwrap_or(100.0);
    if opts.max_vmaf > 0.0 && max_q > opts.max_vmaf {
        max_q = opts.max_vmaf;
    }
    let min_q = min_q.min(max_q);

    // Generate evenly-spaced quality targets
    let num = opts.num_rungs as usize;
    let targets: Vec<f64> = if num == 1 {
        vec![(min_q + max_q) / 2.0]
    } else {
        let step = (max_q - min_q) / (num - 1) as f64;
        (0..num).map(|i| min_q + step * i as f64).collect()
    };

    // Greedy selection: for each target, find closest unused candidate
    let mut used = vec![false; candidates.len()];
    let mut selected = Vec::new();

    for target in &targets {
        let mut best_idx = None;
        let mut best_dist = f64::MAX;

        for (i, p) in candidates.iter().enumerate() {
            if used[i] {
                continue;
            }
            let dist = (p.vmaf - target).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            used[idx] = true;
            selected.push(candidates[idx].clone());
        }
    }

    to_ladder(selected)
}

fn build_crossover_map(h: &Hull) -> std::collections::HashMap<Resolution, f64> {
    let mut crossovers = std::collections::HashMap::new();
    for co in h.crossovers() {
        crossovers.insert(co.to, co.bitrate);
    }
    crossovers
}

fn to_ladder(mut points: Vec<Point>) -> Ladder {
    points.sort_by(|a, b| a.bitrate.partial_cmp(&b.bitrate).unwrap());
    let rungs =
        points.into_iter().enumerate().map(|(i, p)| Rung { point: p, index: i as i32 }).collect();
    Ladder { rungs }
}

impl Ladder {
    pub fn bitrate_range(&self) -> (f64, f64) {
        if self.rungs.is_empty() {
            return (0.0, 0.0);
        }
        (self.rungs.first().unwrap().point.bitrate, self.rungs.last().unwrap().point.bitrate)
    }

    pub fn quality_range(&self) -> (f64, f64) {
        if self.rungs.is_empty() {
            return (0.0, 0.0);
        }
        (self.rungs.first().unwrap().point.vmaf, self.rungs.last().unwrap().point.vmaf)
    }

    pub fn savings(&self, fixed_bitrate: f64) -> f64 {
        if self.rungs.is_empty() || fixed_bitrate <= 0.0 {
            return 0.0;
        }
        let top = &self.rungs.last().unwrap().point;
        if top.bitrate >= fixed_bitrate {
            return 0.0;
        }
        (1.0 - top.bitrate / fixed_bitrate) * 100.0
    }
}
