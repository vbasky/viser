# viser-compare

Browser-based side-by-side comparison player with VMAF quality timeline — serves an interactive QA page for visual inspection of encoded videos.

## Key Types

- `Dip` — a detected quality dip with warning/critical severity thresholds
- `FrameVmaf` — per-frame VMAF score

## Key Functions

- `load_vmaf_data(path)` — loads per-frame VMAF from JSON (supports multiple formats)
- `find_dips(frames, warning_threshold, critical_threshold)` — identifies quality dips by severity
- `serve(opts)` — starts the comparison player HTTP server and opens the browser
