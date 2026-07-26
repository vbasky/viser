# viser-engine

Engine-agnostic video encode/probe layer for the [viser](https://github.com/vbasky/viser) workspace.

Pipelines (per-title, per-shot, quality, CLI) talk to a [`VideoEngine`] trait
instead of calling FFmpeg directly. Shared media types (`Codec`, `Resolution`,
`EncodeJob`, `ProbeResult`, …) live here so backends stay interchangeable.

## Architecture

```text
  CLI / pipelines
        │
        ▼
  viser-engine   ◄── types + VideoEngine + registry
        │
   ┌────┴─────┐
   ▼          ▼
 FFmpeg    External / MLVC / …
 (default) (shell-out templates)
```

## Backends

| Backend | Crate / type | Notes |
|---------|--------------|--------|
| **FFmpeg** | `viser_ffmpeg::FfmpegEngine` | Default. Full encode/probe/extract/concat/chunked |
| **External** | `viser_engine::ExternalEngine` | Shell-out via `VISER_EXTERNAL_ENCODE` / `PROBE` |

### Registering the default engine

```rust
// FFmpeg (what the CLI does at startup)
viser_ffmpeg::register_as_default();

// Or an external neural codec wrapper:
let engine = viser_engine::ExternalEngine::from_env();
viser_engine::set_default_engine(std::sync::Arc::new(engine));
```

Free functions `viser_engine::encode` / `probe` / `extract` / `concat` /
`chunked_encode` dispatch through the registered default.

### External / MLVC integration

```bash
export VISER_EXTERNAL_ENCODE='my-mlvc-wrapper --in {input} --out {output} --q {crf}'
# optional: dump ProbeResult JSON
export VISER_EXTERNAL_PROBE='my-probe --json {path}'
```

Placeholders: `{input}` `{output}` `{crf}` `{preset}` `{width}` `{height}`
`{codec}` `{bitrate}`.

Use `--codec external` (aliases: `mlvc`, `mlvc-s`) so trial matrices select the
external family. Container extract/concat still require FFmpeg (or a fuller
custom engine implementation).

## Implementing a backend

```rust
use viser_engine::{BoxFuture, EncodeJob, EncodeResult, EngineCapabilities, ProbeResult, Progress, VideoEngine};

struct MyEngine;

impl VideoEngine for MyEngine {
    fn id(&self) -> &str { "my-engine" }
    fn name(&self) -> &str { "My Engine" }
    fn capabilities(&self) -> EngineCapabilities { /* ... */ }
    fn probe<'a>(&'a self, path: &'a str) -> BoxFuture<'a, anyhow::Result<ProbeResult>> {
        Box::pin(async move { /* ... */ })
    }
    fn encode<'a>(
        &'a self,
        job: EncodeJob,
        progress: Option<tokio::sync::mpsc::Sender<Progress>>,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        Box::pin(async move { /* ... */ })
    }
    fn extract<'a>(&'a self, _: &'a str, _: &'a str, _: f64, _: f64) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { anyhow::bail!("unsupported") })
    }
    fn concat<'a>(&'a self, _: &'a [String], _: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { anyhow::bail!("unsupported") })
    }
}
```

## Related crates

- [`viser-ffmpeg`](../viser-ffmpeg) — FFmpeg implementation of `VideoEngine`
- [`viser-encoding`](../viser-encoding) — shared encoding config / presets
