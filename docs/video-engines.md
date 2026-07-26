# Video engines

Viser is **video-engine agnostic**. Analysis pipelines (per-title, per-shot,
per-segment, encode CLI) depend on the [`VideoEngine`] trait in `viser-engine`,
not on FFmpeg APIs directly.

## Why

Classical VOD work uses FFmpeg (`libx264`, `libx265`, `libsvtav1`, hardware
encoders). Research and edge deployment increasingly use **external / neural**
codecs (e.g. [Microsoft MLVC](https://github.com/microsoft/mlvc)) that are *not*
FFmpeg `-c:v` targets. A single trait boundary lets both coexist.

## Default: FFmpeg

At CLI startup:

```rust
viser_ffmpeg::register_as_default();
```

This registers `FfmpegEngine`, which implements encode, probe, extract, concat,
and parallel chunked encode via the `ffmpeg` / `ffprobe` binaries.

`use viser_ffmpeg::{Codec, encode, probe, …}` still works — types are defined in
`viser-engine` and re-exported from `viser-ffmpeg` for compatibility.

## CLI flags

```bash
# FFmpeg only (default)
viser per-title analyze -i src.mp4 ...

# Dual-engine: FFmpeg probe/extract + MLVC encode
# Point --mlvc-cmd at your own MLVC encode entrypoint (not shipped in this repo).
viser --engine mlvc \
  --mlvc-cmd 'mlvc-encode' \
  --mlvc-model psnr --mlvc-variant s \
  per-title analyze -i src.y4m --codecs external --resolutions 360p --crf-values 2,4,6

# Explicit roles
viser --probe-engine ffmpeg --encode-engine external \
  --external-encode 'my-enc -i {input} -o {output} -q {crf}' \
  encode -i in.mp4 -o out.bin --codec external --crf 5
```

| Flag | Env | Meaning |
|------|-----|---------|
| `--engine` | | `ffmpeg` \| `external` \| `mlvc` (sets encode; media stays FFmpeg for non-ffmpeg) |
| `--probe-engine` | | Override media engine |
| `--encode-engine` | | Override encode engine |
| `--external-encode` | `VISER_EXTERNAL_ENCODE` | Shell template |
| `--external-probe` | `VISER_EXTERNAL_PROBE` | Probe JSON template |
| `--mlvc-cmd` | `VISER_MLVC_CMD` | MLVC command / template |
| `--mlvc-model` | `VISER_MLVC_MODEL` | `psnr` \| `perceptual` |
| `--mlvc-variant` | `VISER_MLVC_VARIANT` | `full` \| `s` |
| `--mlvc-weights` | `VISER_MLVC_WEIGHTS` | Checkpoint path |

## Dual-engine composite

`CompositeEngine` routes:

| Op | Backend |
|----|---------|
| `probe` / `extract` / `concat` | media (usually FFmpeg) |
| `encode` / chunked encode | encode (FFmpeg, MLVC, external) |

Resolved by `viser_ffmpeg::resolve_engine` / `install_engine`. Pipelines take
an explicit `DynEngine` via `analyze_with` / `adapt_with`.

## External / MLVC

`ExternalEngine` shells out to command templates. Placeholders:

| Placeholder | Meaning |
|-------------|---------|
| `{input}` `{output}` | Paths |
| `{crf}` | Quality / lambda index (mapped from trial CRF) |
| `{preset}` | Speed preset string |
| `{width}` `{height}` | Target resolution (0 if unset) |
| `{codec}` | Codec id (`external`, …) |
| `{bitrate}` | Target bitrate kbps (VBR) |

First-class MLVC: `MlvcConfig` → `ExternalEngine` with id `mlvc`. Supply the
encode command via `--mlvc-cmd` / `VISER_MLVC_CMD` (your wrapper around
[microsoft/mlvc](https://github.com/microsoft/mlvc) or any other tool).

Codec CLI aliases: `external`, `mlvc`, `mlvc-s` → `Codec::External`.

Encode output should be decodable by FFmpeg so VMAF/PSNR measurement still works
(raw YUV, bitstream remuxed to a container, or reconstructed video).

## Library injection

```rust
let engine = viser_ffmpeg::resolve_engine(&opts)?;
viser_pertitle::analyze_with(source, cfg, progress, engine.clone()).await?;
viser_pershot::analyze_with(source, cfg, progress, engine.clone()).await?;
viser_persegment::adapt_with(source, cfg, engine).await?;
```

## Adding a native backend

1. Implement `VideoEngine`.
2. Advertise codecs in `EngineCapabilities`.
3. Register via `set_default_engine` or pass `DynEngine` into `*_with` APIs.
4. Prefer explicit engine handles over process-global free functions in new code.

See `crates/viser-engine/README.md` for the trait sketch.

## Type ownership

| Layer | Owns |
|-------|------|
| `viser-engine` | `Codec`, `Resolution`, `EncodeJob`, `ProbeResult`, `SourceFormat`, `VideoEngine`, registry |
| `viser-ffmpeg` | FFmpeg arg building, path resolution, HW detection, `FfmpegEngine`, HDR probe via ffprobe |
| pipelines | Analysis / ladder / metrics — talk to engine types |

## Related

- [System design](system-design.md)
- [Rate control](rate-control.md)
- [MLVC upstream](https://github.com/microsoft/mlvc)
