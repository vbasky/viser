use viser_ffmpeg::Resolution;

/// A fixed (non-optimized) bitrate ladder for comparison.
#[derive(Debug, Clone)]
pub struct FixedLadder {
    pub name: String,
    pub rungs: Vec<FixedRung>,
}

#[derive(Debug, Clone)]
pub struct FixedRung {
    pub resolution: Resolution,
    pub bitrate: f64, // kbps
}

/// Netflix's original fixed bitrate ladder (2015).
pub fn netflix_old() -> FixedLadder {
    FixedLadder {
        name: "Netflix Fixed (2015)".into(),
        rungs: vec![
            FixedRung { resolution: Resolution::new(320, 240), bitrate: 235.0 },
            FixedRung { resolution: Resolution::new(384, 288), bitrate: 375.0 },
            FixedRung { resolution: Resolution::new(512, 384), bitrate: 560.0 },
            FixedRung { resolution: Resolution::new(512, 384), bitrate: 750.0 },
            FixedRung { resolution: Resolution::new(640, 480), bitrate: 1050.0 },
            FixedRung { resolution: Resolution::new(720, 480), bitrate: 1750.0 },
            FixedRung { resolution: Resolution::new(1280, 720), bitrate: 2350.0 },
            FixedRung { resolution: Resolution::new(1280, 720), bitrate: 3000.0 },
            FixedRung { resolution: Resolution::new(1920, 1080), bitrate: 4300.0 },
            FixedRung { resolution: Resolution::new(1920, 1080), bitrate: 5800.0 },
        ],
    }
}

/// Apple's HLS encoding recommendations (approximate, 2024).
pub fn apple_hls() -> FixedLadder {
    FixedLadder {
        name: "Apple HLS (2024)".into(),
        rungs: vec![
            FixedRung { resolution: Resolution::new(416, 234), bitrate: 145.0 },
            FixedRung { resolution: Resolution::new(640, 360), bitrate: 365.0 },
            FixedRung { resolution: Resolution::new(768, 432), bitrate: 730.0 },
            FixedRung { resolution: Resolution::new(960, 540), bitrate: 1100.0 },
            FixedRung { resolution: Resolution::new(1280, 720), bitrate: 2000.0 },
            FixedRung { resolution: Resolution::new(1280, 720), bitrate: 3000.0 },
            FixedRung { resolution: Resolution::new(1920, 1080), bitrate: 4500.0 },
            FixedRung { resolution: Resolution::new(1920, 1080), bitrate: 6000.0 },
            FixedRung { resolution: Resolution::new(1920, 1080), bitrate: 7800.0 },
        ],
    }
}

impl FixedLadder {
    pub fn total_bitrate(&self) -> f64 {
        self.rungs.iter().map(|r| r.bitrate).sum()
    }

    pub fn top_bitrate(&self) -> f64 {
        self.rungs.last().map(|r| r.bitrate).unwrap_or(0.0)
    }
}
