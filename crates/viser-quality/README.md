# viser-quality

VMAF, PSNR, SSIM, SSIMULACRA2, and butteraugli quality measurement.
VMAF/PSNR/SSIM use FFmpeg's libvmaf filter; SSIMULACRA2 and butteraugli
run their respective CLI tools on extracted PNG frames.

## Key Types

- `Metric` — quality metric variant (`Vmaf`, `Psnr`, `Ssim`, `Ssimulacra2`, `Butteraugli`)
- `MeasureOpts` — measurement options (metrics, subsample, VMAF model, per-frame flag)
- `Result` — aggregate scores with optional per-frame data

## Key Functions

- `measure(reference, distorted, opts)` — computes quality between reference and distorted video

## Prerequisites

- **VMAF/PSNR/SSIM**: `ffmpeg` compiled with `--enable-libvmaf`
- **SSIMULACRA2**: `ssimulacra2` binary on `$PATH`
- **Butteraugli**: `butteraugli` binary on `$PATH` (missing binary degrades gracefully to 0.0)
