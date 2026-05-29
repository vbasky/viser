# viser-chart

Chart generation for viser — renders R-D curves, convex hull visualizations, and ladder charts as PNG images using plotters.

## Key Types

- `Opts` — chart configuration (title, dimensions, format, max bitrate)

## Key Functions

- `rd_curve(points, hull, opts)` — R-D curve chart as PNG bytes
- `per_codec_rd_curve(per_codec, bd_rate, opts)` — per-codec R-D curve comparison
- `ladder_chart(ladder, opts)` — ladder visualization (horizontal bar chart)
- `save_chart(data, path)` — writes chart bytes to file
