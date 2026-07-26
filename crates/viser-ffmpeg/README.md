# viser-ffmpeg

FFmpeg/FFprobe **engine backend** for viser — encode, probe, path resolution,
hardware detection, and probe caching.

Shared types (`Codec`, `Resolution`, `EncodeJob`, …) and the `VideoEngine` trait
live in [`viser-engine`](../viser-engine). This crate re-exports those types for
backward compatibility and provides:

- `FfmpegEngine` — `VideoEngine` implementation
- `register_as_default()` / `ffmpeg_engine()` — process-wide registration
- FFmpeg-specific helpers (`encode_color_args`, `enrich_hdr10`, HW init, …)

## Key Types (re-exported from `viser-engine`)

- `Codec` — classical + hardware + `External` codec ids
- `Resolution` — with `RES_2160P`, `RES_1080P`, `RES_720P`, etc.
- `EncodeJob` / `EncodeResult` / `Progress`
- `ProbeResult` / `StreamInfo` / `SourceFormat`

## Key Functions

- `encode(job, progress_tx)` — FFmpeg encode with progress
- `probe(path)` — ffprobe parse
- `ffmpeg_path()` / `ffprobe_path()` — binary resolution
- `register_as_default()` — install this backend as the process default

## Engine registration

```rust
viser_ffmpeg::register_as_default();
// pipelines may then use viser_engine::encode / probe free functions
```

See [docs/video-engines.md](../../docs/video-engines.md) for multi-engine use
(including external / MLVC shell-out).
