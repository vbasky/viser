# viser-quality

VMAF, PSNR, and SSIM quality measurement via FFmpeg's libvmaf filter.

## Key Types

- `Metric` — quality metric variant (`Vmaf`, `Psnr`, `Ssim`)
- `MeasureOpts` — measurement options (metrics, subsample, VMAF model, per-frame flag)
- `Result` — aggregate scores and optional per-frame data

## Key Functions

- `measure(reference, distorted, opts)` — computes quality between reference and distorted video
