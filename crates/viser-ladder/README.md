# viser-ladder

Bitrate ladder selection with crossover enforcement — picks the best N rungs from a convex hull.

## Key Types

- `Ladder` — ordered rungs with `bitrate_range()`, `quality_range()`, `savings()` methods
- `Rung` — a single ladder step (point + index)
- `FixedLadder` — non-optimized ladder for baseline comparison

## Key Functions

- `select(hull, opts)` — picks optimal rungs using greedy VMAF-target selection
- `netflix_old()` / `apple_hls()` — pre-built standard ladder presets
