# VEO - Video Encoding Optimizer

VEO analyzes video content and computes optimal encoding parameters using
perceptual quality measurement (VMAF) and convex hull analysis. Instead of
applying a one-size-fits-all bitrate ladder, VEO tailors encoding decisions
to the content, producing better quality at lower bitrates.

**Acknowledgment:** VEO builds on decades of research in rate-distortion theory,
perceptual quality measurement, and content-adaptive streaming. Thank you to the
engineers and researchers at Netflix, Beamr, Fraunhofer, Mux, and the broader
video encoding community whose published work, open-source tools, and
foundational science inform every part of this project.

## Optimization Methods

VEO provides four encoding optimization methods, each suited to different
use cases and levels of granularity:

| Method | Granularity | Best For | Description |
|--------|-------------|----------|-------------|
| [Per-Title](docs/per-title-encoding.md) | Whole video | VOD catalogs | Computes a custom bitrate ladder per video using convex hull analysis across resolutions, codecs, and quality levels |
| [Per-Shot](docs/per-shot-encoding.md) | Shot (2-30s) | Feature films, episodic | Detects scene boundaries and allocates bits across shots using Trellis optimization - complex scenes get more bits, simple scenes get fewer |
| [Segment-Level CRF](docs/segment-level-adaptation.md) | 1-second segments | Variable complexity content | Adapts CRF per temporal segment with closed-loop VMAF verification to maintain consistent quality |
| [Context-Aware](docs/content-adaptive-encoding.md) | Per device class | Multi-device streaming | Generates device-specific ladders (mobile/desktop/TV) with appropriate resolution caps, codecs, and VMAF models |

All methods can be combined with the [comparison player](docs/comparison-player.md)
for visual QA of results.

## Table of Contents

- [Quick Start](#quick-start)
- [Usage](#usage)
- [Documentation](#documentation)
- [Supported Codecs](#supported-codecs)
- [Project Structure](#project-structure)
- [Status](#status)
- [License](#license)

## Quick Start

```bash
# Build VEO
cargo build --release

# The binary is at target/release/veo (or veo.exe on Windows)

# Run your first per-title analysis
./target/release/veo per-title analyze -i assets/sd/akiyo_cif.y4m \
  --resolutions 240p --codecs libx264 --preset ultrafast
```

### Prerequisites

- **Rust 1.85+** (edition 2024) - install via [rustup](https://rustup.rs/)
- **FFmpeg with libvmaf** - build from source or use a package manager
- FFmpeg/FFprobe must be on `PATH`, or set `VEO_FFMPEG` / `VEO_FFPROBE` env vars

## Usage

### Per-title analysis

```bash
veo per-title analyze -i video.y4m \
  --codecs libx264,libsvtav1 \
  --resolutions 480p,720p,1080p \
  --parallel 4 -o results.json
```

Options: `--mode crf|qp`, `--dry-run`, `--rungs 6`, `--min-bitrate 200`, `--max-bitrate 8000`

### Per-shot analysis

```bash
veo per-shot detect -i video.y4m --threshold 10
veo per-shot analyze -i video.y4m --target-bitrate 2000
```

### Segment-level CRF adaptation

```bash
veo per-segment analyze -i video.y4m --target-vmaf 93 --codec libx264
```

### Context-aware encoding

```bash
veo context-aware analyze -i video.y4m --devices mobile,desktop,tv
```

### Visual QA with comparison player

```bash
veo quality measure --reference original.mp4 --distorted encoded.mp4 \
  --per-frame -o vmaf_data.json
veo compare --reference original.mp4 --encoded encoded.mp4 \
  --vmaf-data vmaf_data.json
```

### Other commands

```bash
veo inspect probe video.mp4              # Show video metadata
veo encode input.y4m -o out.mp4          # Encode a video
veo quality measure --reference a --distorted b  # Measure VMAF/PSNR/SSIM
```

## Documentation

| Document | Description |
|----------|-------------|
| [Per-Title Encoding](docs/per-title-encoding.md) | Convex hull analysis, R-D optimization, ladder selection |
| [Per-Shot Encoding](docs/per-shot-encoding.md) | Shot detection, Trellis optimization, constant-slope bit allocation |
| [Content-Adaptive Encoding](docs/content-adaptive-encoding.md) | Device profiles, multi-codec hulls, ML prediction concepts |
| [Segment-Level CRF Adaptation](docs/segment-level-adaptation.md) | Segment-level CRF tuning with complexity analysis |
| [Quality Metrics](docs/quality-metrics.md) | VMAF, PSNR, SSIM, SSIMULACRA2, BD-Rate |
| [Rate Control](docs/rate-control.md) | CRF vs QP vs VBR - which mode and when |
| [Shot Detection](docs/shot-detection.md) | FFmpeg scdet, PySceneDetect, TransNetV2 - comparison and guidelines |
| [Chunked Encoding](docs/chunked-encoding.md) | Parallel encoding with shot-aware chunking for production workflows |
| [Comparison Player](docs/comparison-player.md) | Side-by-side visual QA with VMAF timeline and quality dip markers |

## Supported Codecs

| Codec | Flag | Notes |
|-------|------|-------|
| H.264/AVC | `libx264` | Fastest encode, widest device support |
| H.265/HEVC | `libx265` | ~30-40% better compression than H.264 |
| AV1 | `libsvtav1` | ~50% better compression, royalty-free, SVT-AV1 4.0 |

## Project Structure

```
veo/
├── crates/
│   ├── veo-ffmpeg/         FFmpeg/FFprobe wrapper (encode, probe, path, cache)
│   ├── veo-quality/        VMAF/PSNR/SSIM measurement
│   ├── veo-encoding/       Shared config, preset mapping, temp cleanup
│   ├── veo-hull/           Convex hull + BD-Rate
│   ├── veo-ladder/         Ladder selection with crossover enforcement
│   ├── veo-shot/           Shot detection (scdet)
│   ├── veo-complexity/     Spatial/temporal/DCT complexity analysis
│   ├── veo-pertitle/       Per-title analysis pipeline
│   ├── veo-pershot/        Per-shot + Trellis optimization
│   ├── veo-persegment/     Segment-level CRF adaptation
│   ├── veo-contextaware/   Device-specific ladder generation
│   ├── veo-checkpoint/     Resume support for long analyses
│   ├── veo-compare/        Browser-based comparison player
│   ├── veo-chart/          Chart generation (plotters)
│   └── veo-cli/            CLI binary (clap)
├── docs/                   Principles and science docs
├── LICENSE
└── NOTICE
```

## Status

All four optimization methods ported from the Go implementation:

**Per-Title** - Convex hull, BD-Rate, resolution crossover enforcement,
Netflix fixed ladder comparison, CRF and QP trial modes, checkpointing.

**Per-Shot** - Shot detection (scdet), per-shot hulls, Trellis Lagrangian
bit allocation.

**Segment-Level CRF** - Complexity analysis (entropy + YDIF + DCT energy),
binary-search CRF per 1-second segment, closed-loop VMAF verification.

**Context-Aware** - Device profiles (mobile/desktop/TV/4K TV) with resolution
caps, codec preferences, and VMAF model selection.

**Infrastructure** - Async I/O via tokio, structured logging (tracing),
checkpointing, comparison player with VMAF dip detection.

### Backlog
- Chart generation (plotters integration)
- Chunked encoding ([documented](docs/chunked-encoding.md))
- ML feature extraction and prediction
- REST API
- Tests

## License

Apache License 2.0 - see [LICENSE](LICENSE) for details.

H.264/HEVC encoding may require patent licenses depending on use case.
AV1 is royalty-free. See [NOTICE](NOTICE) for third-party attributions.
