# viser-cli

![viser - Video Encoding Optimizer](https://raw.githubusercontent.com/vbasky/viser/main/docs/banner.png)

CLI binary for viser — the main entry point for all optimization methods, encode commands, quality measurement, and comparison.

## Commands

- `viser per-title analyze` — per-title (whole-video) convex hull analysis
- `viser per-shot detect/analyze` — shot detection and per-shot analysis
- `viser per-segment analyze` — segment-level CRF adaptation
- `viser context-aware analyze` — device-specific ladder generation
- `viser quality measure` — VMAF/PSNR/SSIM measurement
- `viser compare` — browser-based comparison player
- `viser encode` — single encode job
- `viser inspect probe` — ffprobe wrapper

See `viser --help` or the [top-level README](../README.md) for usage examples.
