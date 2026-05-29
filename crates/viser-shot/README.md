# viser-shot

Shot/scene boundary detection using FFmpeg's `scdet` filter.

## Key Types

- `Shot` — detected scene with index, start/end/duration, and change score
- `DetectOpts` — detection parameters (threshold 0–100, minimum shot duration)

## Key Functions

- `detect(path, opts)` — finds shot boundaries, merges short shots, returns timestamps
