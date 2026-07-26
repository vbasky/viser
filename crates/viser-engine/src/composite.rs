//! Compose separate media (probe/extract/concat) and encode engines.
//!
//! Typical dual-engine setup for neural codecs:
//!
//! ```text
//! media  = FFmpeg   (probe, extract, concat, quality inputs)
//! encode = MLVC     (trial encodes)
//! ```

use crate::{
    BoxFuture, Codec, DynEngine, EncodeJob, EncodeResult, EngineCapabilities, ProbeResult,
    Progress, VideoEngine, chunk_plan,
};

/// Routes media operations and encode operations to different backends.
#[derive(Clone)]
pub struct CompositeEngine {
    /// Probe / extract / concat backend (usually FFmpeg).
    media: DynEngine,
    /// Encode backend (FFmpeg, MLVC, external, …).
    encode: DynEngine,
}

impl CompositeEngine {
    /// Builds a composite from a media engine and an encode engine.
    pub fn new(media: DynEngine, encode: DynEngine) -> Self {
        Self { media, encode }
    }

    /// Media (probe/extract/concat) engine.
    pub fn media(&self) -> &DynEngine {
        &self.media
    }

    /// Encode engine.
    pub fn encode_engine(&self) -> &DynEngine {
        &self.encode
    }
}

impl VideoEngine for CompositeEngine {
    fn id(&self) -> &str {
        "composite"
    }

    fn name(&self) -> &str {
        "Composite (media + encode)"
    }

    fn capabilities(&self) -> EngineCapabilities {
        let media = self.media.capabilities();
        let encode = self.encode.capabilities();
        EngineCapabilities {
            id: format!("composite:{}+{}", media.id, encode.id),
            name: format!("{} + {}", media.name, encode.name),
            codecs: encode.codecs,
            can_probe: media.can_probe,
            can_extract: media.can_extract,
            can_concat: media.can_concat,
            can_chunked_encode: encode.can_chunked_encode && media.can_concat,
            supports_hwaccel: encode.supports_hwaccel || media.supports_hwaccel,
        }
    }

    fn supports_codec(&self, codec: &Codec) -> bool {
        self.encode.supports_codec(codec)
    }

    fn probe<'a>(&'a self, path: &'a str) -> BoxFuture<'a, anyhow::Result<ProbeResult>> {
        self.media.probe(path)
    }

    fn encode<'a>(
        &'a self,
        job: EncodeJob,
        progress: Option<tokio::sync::mpsc::Sender<Progress>>,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        self.encode.encode(job, progress)
    }

    fn extract<'a>(
        &'a self,
        input: &'a str,
        output: &'a str,
        start: f64,
        duration: f64,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        self.media.extract(input, output, start, duration)
    }

    fn concat<'a>(
        &'a self,
        inputs: &'a [String],
        output: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        self.media.concat(inputs, output)
    }

    fn chunked_encode<'a>(
        &'a self,
        job: EncodeJob,
        chunk_seconds: f64,
        parallel: usize,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        Box::pin(async move {
            // Same backend: use its optimized (possibly parallel) path.
            if self.media.id() == self.encode.id() {
                return self.encode.chunked_encode(job, chunk_seconds, parallel).await;
            }

            if !self.capabilities().can_chunked_encode {
                anyhow::bail!("composite engine does not support chunked encode");
            }

            let probe_result = self.media.probe(&job.input).await?;
            let duration = probe_result.format.duration;
            if duration <= 0.0 {
                anyhow::bail!("could not determine source duration for chunked encoding");
            }
            let chunks = chunk_plan(duration, chunk_seconds);
            if chunks.is_empty() {
                anyhow::bail!(
                    "no chunks were planned (duration={duration}, chunk_seconds={chunk_seconds})"
                );
            }
            if chunks.len() == 1 {
                return self.encode.encode(job, None).await;
            }

            let tmp_dir = std::env::temp_dir()
                .join(format!("viser-chunked-composite-{}", std::process::id()));
            std::fs::create_dir_all(&tmp_dir)?;
            let _cleanup = TmpDirGuard(tmp_dir.clone());

            let mut outputs = Vec::with_capacity(chunks.len());
            for (index, (start, dur)) in chunks.iter().copied().enumerate() {
                let chunk_output = tmp_dir.join(format!("chunk_{index:04}.mp4"));
                let output_path = chunk_output.to_string_lossy().into_owned();
                let mut extra_args = job.extra_args.clone();
                extra_args.extend([
                    "-ss".into(),
                    format!("{start:.6}"),
                    "-t".into(),
                    format!("{dur:.6}"),
                ]);
                let chunk_job = EncodeJob {
                    input: job.input.clone(),
                    output: output_path.clone(),
                    extra_args,
                    ..job.clone()
                };
                self.encode.encode(chunk_job, None).await?;
                outputs.push(output_path);
            }

            let started = std::time::Instant::now();
            self.media.concat(&outputs, &job.output).await?;

            let meta = std::fs::metadata(&job.output)?;
            let output_probe = self.media.probe(&job.output).await?;
            let bitrate = output_probe.format.bit_rate as f64 / 1000.0;

            Ok(EncodeResult { job, bitrate, file_size: meta.len(), duration: started.elapsed() })
        })
    }
}

struct TmpDirGuard(std::path::PathBuf);
impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FormatInfo, RateControlMode, Resolution};
    use std::sync::Arc;

    struct StubEngine {
        id: &'static str,
        probe_calls: std::sync::atomic::AtomicUsize,
        encode_calls: std::sync::atomic::AtomicUsize,
    }

    impl StubEngine {
        fn new(id: &'static str) -> Self {
            Self {
                id,
                probe_calls: std::sync::atomic::AtomicUsize::new(0),
                encode_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl VideoEngine for StubEngine {
        fn id(&self) -> &str {
            self.id
        }
        fn name(&self) -> &str {
            self.id
        }
        fn capabilities(&self) -> EngineCapabilities {
            EngineCapabilities {
                id: self.id.into(),
                name: self.id.into(),
                codecs: vec!["libx264".into(), "external".into()],
                can_probe: true,
                can_extract: true,
                can_concat: true,
                can_chunked_encode: true,
                supports_hwaccel: false,
            }
        }
        fn probe<'a>(&'a self, path: &'a str) -> BoxFuture<'a, anyhow::Result<ProbeResult>> {
            Box::pin(async move {
                self.probe_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(ProbeResult {
                    format: FormatInfo {
                        filename: path.into(),
                        format_name: "test".into(),
                        format_long_name: String::new(),
                        duration: 1.0,
                        size: 0,
                        bit_rate: 1_000_000,
                        probe_score: 100,
                    },
                    streams: vec![],
                })
            })
        }
        fn encode<'a>(
            &'a self,
            job: EncodeJob,
            _: Option<tokio::sync::mpsc::Sender<Progress>>,
        ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
            Box::pin(async move {
                self.encode_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = std::fs::write(&job.output, b"fake");
                Ok(EncodeResult {
                    job,
                    bitrate: 100.0,
                    file_size: 4,
                    duration: std::time::Duration::from_millis(1),
                })
            })
        }
        fn extract<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: f64,
            _: f64,
        ) -> BoxFuture<'a, anyhow::Result<()>> {
            Box::pin(async move { Ok(()) })
        }
        fn concat<'a>(&'a self, _: &'a [String], _: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
            Box::pin(async move { Ok(()) })
        }
    }

    #[tokio::test]
    async fn composite_routes_probe_and_encode() {
        let media = Arc::new(StubEngine::new("media"));
        let encode = Arc::new(StubEngine::new("encode"));
        let composite = CompositeEngine::new(media.clone(), encode.clone());

        composite.probe("/tmp/in").await.unwrap();
        assert_eq!(media.probe_calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(encode.probe_calls.load(std::sync::atomic::Ordering::Relaxed), 0);

        let job = EncodeJob {
            input: "/tmp/in".into(),
            output: std::env::temp_dir().join("viser-composite-test.out").to_string_lossy().into(),
            resolution: Some(Resolution::new(640, 360)),
            codec: Codec::External,
            crf: 23,
            rate_control: RateControlMode::Crf,
            target_bitrate: 0.0,
            max_bitrate: 0.0,
            bufsize: 0.0,
            preset: "medium".into(),
            hwaccel: None,
            extra_args: vec![],
            source_format: None,
        };
        composite.encode(job, None).await.unwrap();
        assert_eq!(encode.encode_calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(media.encode_calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
}
