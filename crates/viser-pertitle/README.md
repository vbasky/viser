# viser-pertitle

Per-title encoding pipeline — probes source, builds trial matrix, encodes in parallel with checkpointing, computes convex hull and optimal ladder.

## Key Types

- `Config` — search space definition (encoding config, ladder opts, checkpoint path, VMAF model)
- `Result` — complete output (all points, hull, per-codec hulls, crossovers, selected ladder)

## Key Functions

- `analyze(source, cfg, progress_tx)` — runs the full per-title analysis pipeline
