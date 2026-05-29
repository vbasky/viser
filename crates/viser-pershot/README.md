# viser-pershot

Per-shot encoding with Trellis bit allocation — detects shots, runs per-title analysis on each independently, then optimizes bit distribution via Lagrangian constant-slope search.

## Key Types

- `Config` — per-shot config (encoding parameters, shot detection, ladder options)
- `TrellisAssignment` — optimal encoding assignment per shot

## Key Functions

- `analyze(source, cfg, progress_tx)` — detects shots and runs per-shot analysis
- `trellis_optimize(shot_results, opts)` — finds optimal encoding per shot via binary search on lambda
