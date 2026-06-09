# viser — Video Encoding Optimizer

![viser - Video Encoding Optimizer](https://raw.githubusercontent.com/vbasky/viser/main/docs/banner.png)

**Name:** *Viser* blends *vision* + *optimizer* — it sees the optimal encoding for every video. It's also French *viser* ("to aim/see").

[![crates.io](https://img.shields.io/crates/v/viser-cli?logo=rust&color=orange)](https://crates.io/crates/viser-cli)
[![docs.rs](https://img.shields.io/docsrs/viser-cli?logo=docsdotrs)](https://docs.rs/viser-cli)
[![CI](https://img.shields.io/github/actions/workflow/status/vbasky/viser/ci.yml?branch=main&logo=github&label=CI)](https://github.com/vbasky/viser/actions)
[![License](https://img.shields.io/github/license/vbasky/viser)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)](https://www.rust-lang.org)

**Acknowledgment:** viser builds on decades of research in rate-distortion theory,
perceptual quality measurement, and content-adaptive streaming. Thank you to the
engineers and researchers at Netflix, Beamr, Fraunhofer, Mux, and the broader
video encoding community whose published work, open-source tools, and
foundational science inform every part of this project.

---

viser analyzes video content and computes optimal encoding parameters using
perceptual quality measurement (VMAF) and convex hull (Pareto frontier) analysis.
Instead of applying a one-size-fits-all bitrate ladder, viser tailors encoding
decisions to each video's content complexity, producing better quality at lower
bitrates.

| Content Type | Fixed Ladder @ 3 Mbps 1080p | viser Custom Ladder |
| --- | --- | --- |
| Talking head (news anchor) | Excellent — bits wasted | Same quality, half the bitrate |
| Animation (Pixar-style) | Very good — some waste | Same quality, ~30% less bitrate |
| Sports (football game) | Acceptable — needs more | Same bitrate, higher quality |
| Film grain (dark thriller) | Poor — severely underbit | Same bitrate, transparent quality |

## Optimization Methods

| Method | Granularity | Best For | Description |
| -------- | ------------- | ---------- | ------------- |
| [Per-Title](docs/per-title-encoding.md) | Whole video | VOD catalogs | Computes a custom bitrate ladder per video using convex hull analysis across resolutions, codecs, and quality levels |
| [Per-Shot](docs/per-shot-encoding.md) | Shot (2-30s) | Feature films, episodic | Detects scene boundaries and allocates bits across shots using Trellis optimization — complex scenes get more bits, simple get fewer |
| [Segment-Level CRF](docs/segment-level-adaptation.md) | 1-second segments | Variable complexity content | Adapts CRF per segment with closed-loop VMAF verification to maintain consistent quality |
| [Context-Aware](docs/content-adaptive-encoding.md) | Per device class | Multi-device streaming | Generates device-specific ladders (mobile/desktop/TV) with resolution caps, codecs, and VMAF models |

## Architecture

```bash
viser/
├── crates/
│   ├── viser-ffmpeg/         FFmpeg/FFprobe wrapper (encode, probe, path, cache)
│   ├── viser-quality/        VMAF/PSNR/SSIM measurement
│   ├── viser-hull/           Convex hull (Pareto frontier) + BD-Rate
│   ├── viser-ladder/         Ladder selection with crossover enforcement
│   ├── viser-shot/           Shot/scene detection (FFmpeg scdet)
│   ├── viser-complexity/     Spatial/temporal/DCT complexity analysis
│   ├── viser-encoding/       Shared config, preset mapping, temp cleanup
│   ├── viser-pertitle/       Per-title analysis pipeline
│   ├── viser-pershot/        Per-shot + Trellis optimization
│   ├── viser-persegment/     Segment-level CRF adaptation
│   ├── viser-contextaware/   Device-specific ladder generation
│   ├── viser-checkpoint/     Resume support for long analyses
│   ├── viser-compare/        Browser-based comparison player
│   ├── viser-chart/          Chart generation (plotters)
│   └── viser-cli/            CLI binary (clap)
├── docs/                     Principles and science docs
├── Cargo.toml
├── LICENSE
└── rustfmt.toml
```

## Installation

```bash
# Cargo (from source)
cargo install viser-cli

# With optional revelo probe engine (pure-Rust, no ffprobe needed)
cargo install viser-cli --features revelo

# Homebrew (once accepted into homebrew-core)
brew install viser

# Or build from source
git clone https://github.com/vbasky/viser.git
cd viser
cargo build --release
```

Pre-built binaries for Linux, macOS (ARM + Intel), and Windows are available on
the [releases page](https://github.com/vbasky/viser/releases).

## Quick Start

### Prerequisites

- **Rust 1.88+** (edition 2024) — install via [rustup](https://rustup.rs/)
- **FFmpeg with libvmaf** — build from source or use a package manager
- FFmpeg/FFprobe must be on `PATH`, or set `VISER_FFMPEG` / `VISER_FFPROBE` env vars

```bash
# Build viser
cargo build --release

# Run tests (170+ tests, ~0.5s)
cargo test --workspace

# Run your first per-title analysis
./target/release/viser per-title analyze -i video.y4m \
  --resolutions 240p --codecs libx264 --preset ultrafast
```

## Usage

### Per-title encoding (whole-video ladder)

```bash
# Run a full per-title analysis with H.264 + AV1, 3 resolutions, 7 CRF values each
viser per-title analyze -i video.mp4 \
  --codecs libx264,libsvtav1 \
  --resolutions 480p,720p,1080p \
  --preset veryfast \
  --parallel 4 \
  -o analysis.json

# Deliver the selected ladder rungs as final encodes
viser per-title deliver \
  --analysis analysis.json \
  --output-dir delivery \
  --mode capped-crf \
  --parallel 4 \
  --manifest delivery/manifest.json
```

`per-title analyze` now automatically detects audio bitrate from the source and
reserves it in the delivery budget. HDR sources are detected and gated behind
`--allow-hdr` (currently best-effort only). The analysis JSON includes an
`audio_bitrate_kbps` field that delivery can use for budget planning.

### Per-shot encoding (scene-level bit allocation)

```bash
# Detect scene boundaries
viser per-shot detect -i video.mp4 --threshold 10

# Run per-shot analysis with Trellis optimization
viser per-shot analyze -i video.mp4 --target-bitrate 2000
```

### Segment-level CRF adaptation

```bash
viser per-segment analyze -i video.mp4 --target-vmaf 93 --codec libx264
```

### Context-aware encoding (device-specific ladders)

```bash
viser context-aware analyze -i video.mp4 --devices mobile,desktop,tv
```

### Content type detection (screen vs natural video)

```bash
# Analyze spatial/temporal/DCT complexity and classify content type
viser complexity analyze -i video.mp4
```

Screen content (slides, code, UI screencasts) needs different encoding strategies
than natural video — viser detects it from complexity heuristics: static frames,
sharp edges, and DCT energy vs temporal motion tradeoffs.

### Visual QA with comparison player

```bash
# Measure per-frame VMAF between reference and encoded
viser quality measure --reference original.mp4 --distorted encoded.mp4 \
  --per-frame -o vmaf_data.json

# Launch side-by-side comparison player with VMAF timeline
viser compare --reference original.mp4 --encoded encoded.mp4 \
  --vmaf-data vmaf_data.json
```

### Inspection

```bash
# Probe with ffprobe (default)
viser inspect probe video.mp4

# Probe with revelo (pure-Rust, no ffprobe needed — build with --features revelo)
viser inspect probe video.mp4 --probe-engine revelo
```

### Direct encode

```bash
viser encode input.mp4 -o out.mp4
viser encode input.mp4 -o capped.mp4 --mode capped-crf --crf 20 --max-bitrate 3000
viser encode input.mp4 -o rung_3000k.mp4 --mode vbr --target-bitrate 3000
viser quality measure --reference a.mp4 --distorted b.mp4
```

`per-title deliver` reads a saved analysis JSON, encodes the selected ladder
rungs as final delivery outputs, and writes a manifest describing the emitted
files with their target and measured bitrates. Delivery supports both 2-pass
VBR and capped-CRF output, plus optional local chunked encoding with automatic
concatenation.

`per-title analyze` now detects HDR sources from probe metadata. By default it
refuses HDR inputs because libvmaf-based analysis is still SDR-centric; pass
`--allow-hdr` only for best-effort workflows. `viser inspect probe` surfaces the
detected dynamic range and color metadata to make that decision explicit.

## Supported Codecs

| Codec | Flag | Notes |
| ------- | ------ | ------- |
| H.264/AVC | `libx264` | Fastest encode, widest device support |
| H.265/HEVC | `libx265` | ~30-40% better compression than H.264 |
| AV1 | `libsvtav1` | ~50% better compression, royalty-free, SVT-AV1 4.0 |

## Design

| Principle | Description |
| ----------- | ------------- |
| **Content-aware** | Tailors encoding to each video's visual complexity, not one-size-fits-all |
| **VMAF-driven** | Uses perceptual quality scores that correlate with human eyes, not PSNR |
| **Pareto-optimal** | Finds the set of encoding points where no improvement is possible without tradeoff |
| **Four granularities** | Whole-video, per-scene, per-second, per-device — pick the right level |
| **Async + parallel** | tokio-based concurrent trial encodes, semaphore-controlled parallelism |
| **Resumable** | SHA-256 checkpointing means multi-hour analyses survive crashes |
| **BSD-2-Clause** | Permissive license, no patent grant implications |

## Project Scale

| Metric | Value |
| -------- | ------- |
| Workspace crates | 15 |
| Optimization methods | 4 (per-title, per-shot, per-segment, context-aware) |
| Codecs | 3 (H.264, H.265, AV1) |
| Quality metrics | 5 (VMAF, PSNR, SSIM, SSIMULACRA2, Butteraugli) |
| License | BSD-2-Clause |
| MSRV | 1.88 |

## Status

All four optimization methods ported from the prior Go implementation, plus
three additional features shipped since 0.3.0.

- **Per-Title** — Convex hull, BD-Rate, resolution crossover enforcement, Netflix/Apple fixed ladder comparison, CRF and QP trial modes, checkpointing, audio bitrate-aware ladder budgets.
- **Delivery Path** — 2-pass VBR delivery from saved analysis, capped-CRF delivery, manifest export, parallel rung generation, local chunked delivery with concat assembly.
- **Per-Shot** — Shot detection (scdet), per-shot hulls, Trellis Lagrangian bit allocation.
- **Segment-Level CRF** — Complexity analysis (entropy + YDIF + DCT energy), binary-search CRF per 1-second segment, closed-loop VMAF verification.
- **Context-Aware** — Device profiles (mobile/desktop/TV/4K TV) with resolution caps, codec preferences, VMAF model selection.
- **Screen Content Detection** — Classifies video as natural or screen content (slides/code/UI) from spatial/temporal/DCT heuristics, for encoding strategy selection.
- **Pure-Rust Probing** — Optional `revelo` probe engine replaces ffprobe for metadata extraction; build with `--features revelo`, use with `--probe-engine revelo`.
- **SSIMULACRA2 + Butteraugli** — Two extra quality metrics in `viser-quality`, run alongside VMAF/PSNR/SSIM.

### Test Suite

The workspace contains **170+ tests** covering:

```bash
# Run all tests
cargo test --workspace

# Run tests for specific algorithm crates
cargo test -p viser-hull        # convex hull, BD-rate (24 tests)
cargo test -p viser-ladder       # ladder selection, crossover, savings (19 tests)
cargo test -p viser-pershot      # Trellis optimization (12 tests)
cargo test -p viser-complexity   # complexity analysis + screen content detection (21 tests)
cargo test -p viser-ffmpeg       # probe, encode args, revelo adapter (26 tests)
cargo test -p viser-encoding     # config validation, preset mapping (13 tests)

# Run with revelo probe engine enabled
cargo test --features revelo -p viser-ffmpeg -p viser-cli
```

Tests cover: convex hull (empty, single, interior removal, unsorted input, per-codec),
BD-rate (minimum points, negative efficiency, overlap, singular matrices, cubic fit),
Trellis (empty, single shot, duration weighting, identical shots, empty hull fallback,
lambda search bounds), ladder (empty, zero rungs, bitrate/VMAF filters, max VMAF cap,
sorted output, Netflix/Apple reference ladders, savings), screen content (slides 90%,
natural 0%, code capture 70%, empty), and revelo probe (codec mapping, color transfer,
pixel format, frame rate formatting).

### Backlog

- Chart generation (plotters integration — not yet wired into CLI)
- Distributed chunked encoding — multi-machine orchestration is still out of scope; current chunking is local-only
- REST API
- Scene-transition smoothing — per-shot ladders switch abruptly between shots
- ABR switching optimization — ladder rungs tuned for client switching behavior, not just quality-spaced
- Cost-aware optimization — factor storage/CDN cost into ladder selection
- HW acceleration in quality measurement — VMAF runs on CPU via libvmaf; GPU-accelerated path is not viable (libvmaf has no GPU backend)

### Limitations (design scope)

viser is designed for content-adaptive VOD encoding and explicitly does not address:

- **No ML prediction** — 42+ trial encodes per analysis every time; Bitmovin/Mux predict ladders from source features in minutes, not hours. viser measures, not predicts.
- **HDR analysis is best-effort only** — HDR is detected and gated behind `--allow-hdr`, but quality scoring still depends on SDR-oriented libvmaf.
- **No streaming-aware optimization** — delivery can emit manifest metadata for files it wrote, but not HLS/DASH playlists or switching-aware ladder tuning.

## Documentation

| Document | Description |
| ---------- | ------------- |
| [Per-Title Encoding](docs/per-title-encoding.md) | Convex hull, R-D optimization, ladder selection |
| [Per-Shot Encoding](docs/per-shot-encoding.md) | Shot detection, Trellis, constant-slope bit allocation |
| [Content-Adaptive Encoding](docs/content-adaptive-encoding.md) | Device profiles, multi-codec hulls |
| [Segment-Level CRF](docs/segment-level-adaptation.md) | CRF tuning with complexity analysis |
| [Quality Metrics](docs/quality-metrics.md) | VMAF, PSNR, SSIM, BD-Rate |
| [Rate Control](docs/rate-control.md) | CRF vs QP vs VBR |
| [Shot Detection](docs/shot-detection.md) | scdet, PySceneDetect, TransNetV2 |
| [Chunked Encoding](docs/chunked-encoding.md) | Parallel encoding for production |
| [Comparison Player](docs/comparison-player.md) | Side-by-side QA with VMAF timeline |
| [Robustness Assessment](docs/robustness-assessment.md) | Project assessment: strengths, weaknesses, and feature gaps |

## Status

`0.6.x` — hardware encoder support (NVENC, QuickSync, VideoToolbox, VAAPI, AMF
for H.264/H.265) plus battle-tested algorithms ported from a production Go
implementation. Per-title analysis, per-shot Trellis optimization, and quality
measurement are covered by integration tests. The API may evolve before `1.0`.
See the [status & roadmap](STATUS.md) for what's covered today and what's
planned.

## License

BSD 2-Clause License — see [LICENSE](LICENSE) for details.

H.264/HEVC encoding may require patent licenses depending on use case.
AV1 is royalty-free. See [NOTICE](NOTICE) for third-party attributions.
