# viser-complexity

Spatial, temporal, and DCT energy complexity analysis — extracts per-frame metrics and aggregates into segments.

## Key Types

- `Profile` — full complexity profile (per-frame metrics, segment aggregates, overall score)
- `AnalyzeOpts` — analysis options (segment duration, subsample)

## Key Functions

- `analyze(path, opts)` — extracts complexity metrics (entropy, temporal diff, DCT energy) via FFmpeg
