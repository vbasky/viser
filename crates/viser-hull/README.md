# viser-hull

Convex hull (Pareto frontier) and Bjontegaard Delta Rate (BD-Rate) computation.

## Key Types

- `Point` — a single encoding trial result (resolution, codec, CRF, bitrate, VMAF)
- `Hull` — convex hull of R-D points with `crossovers()` method

## Key Functions

- `compute_upper(points)` — upper convex hull via Andrew's monotone chain (O(n log n))
- `compute_per_codec(points)` — separate upper hull per codec
- `bd_rate(curve_a, curve_b)` — BD-Rate with cubic interpolation (4+ points per curve)
