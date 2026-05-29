# viser-ffmpeg

FFmpeg/FFprobe wrapper — encode, probe, path resolution, and cache management.

## Key Types

- `Codec` — supported codecs (`X264`, `X265`, `SvtAv1`)
- `Resolution` — video resolution with `new()` and `label()` helpers; constants `RES_2160P`, `RES_1080P`, `RES_720P`, etc.
- `EncodeResult` — result of a single encode trial

## Key Functions

- `encode(job, progress_tx)` — runs an FFmpeg encode job with real-time progress reporting
- `probe(path)` — runs ffprobe and returns parsed format/stream/fps info
- `ffmpeg_path()` / `ffprobe_path()` — resolved binary paths (from env or PATH)
