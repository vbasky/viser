# viser-persegment

Segment-level CRF adaptation with closed-loop VMAF verification — analyzes complexity, then runs a binary search per 1-second segment to hit a target VMAF.

## Key Types

- `Config` — segment adaptation config (target VMAF, tolerance, CRF range, codec, segment duration)
- `Result` — per-segment CRF results, overall averages, and complexity profile

## Key Functions

- `adapt(source, cfg)` — runs segment-level CRF adaptation
