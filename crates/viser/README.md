# viser

![viser - Video Encoding Optimizer](https://raw.githubusercontent.com/vbasky/viser/main/docs/banner.png)

**Video Encoding Optimizer** — facade crate.

This crate contains no logic of its own. It re-exports each viser library crate
as a module so you can depend on a single `viser` crate instead of a dozen
`viser-*` crates.

```toml
# everything (default)
viser = "0.10"

# only what you need
viser = { version = "0.10", default-features = false, features = ["quality", "hull"] }
```

```rust
use viser::quality::Metric;
use viser::hull;
```

Each module is gated behind a feature flag of the same name; all are enabled by
default via the `full` feature.

| Module | Crate | Purpose |
|--------|-------|---------|
| `ffmpeg` | [`viser-ffmpeg`](https://crates.io/crates/viser-ffmpeg) | FFmpeg/FFprobe wrapper |
| `quality` | [`viser-quality`](https://crates.io/crates/viser-quality) | VMAF/PSNR/SSIM measurement |
| `hull` | [`viser-hull`](https://crates.io/crates/viser-hull) | Convex hull (Pareto frontier) and BD-Rate |
| `ladder` | [`viser-ladder`](https://crates.io/crates/viser-ladder) | Bitrate ladder selection |
| `shot` | [`viser-shot`](https://crates.io/crates/viser-shot) | Shot/scene detection |
| `complexity` | [`viser-complexity`](https://crates.io/crates/viser-complexity) | Spatial/temporal/DCT complexity analysis |
| `encoding` | [`viser-encoding`](https://crates.io/crates/viser-encoding) | Shared encoding configuration |
| `checkpoint` | [`viser-checkpoint`](https://crates.io/crates/viser-checkpoint) | Checkpoint/resume support |
| `pertitle` | [`viser-pertitle`](https://crates.io/crates/viser-pertitle) | Per-title encoding pipeline |
| `pershot` | [`viser-pershot`](https://crates.io/crates/viser-pershot) | Per-shot encoding with Trellis allocation |
| `persegment` | [`viser-persegment`](https://crates.io/crates/viser-persegment) | Segment-level CRF adaptation |
| `contextaware` | [`viser-contextaware`](https://crates.io/crates/viser-contextaware) | Device-specific ladder generation |
| `compare` | [`viser-compare`](https://crates.io/crates/viser-compare) | Side-by-side comparison player |
| `chart` | [`viser-chart`](https://crates.io/crates/viser-chart) | Chart generation (R-D curves, hull, ladder) |

## Command-line tool

The CLI lives in the separate [`viser-cli`](https://crates.io/crates/viser-cli)
crate, which installs a `viser` binary:

```sh
cargo install viser-cli
```

## License

BSD-2-Clause
