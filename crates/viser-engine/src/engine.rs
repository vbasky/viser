use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{Codec, EncodeJob, EncodeResult, ProbeResult, Progress, chunk_plan};

/// Boxed future returned by [`VideoEngine`] methods (dyn-compatible async).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Shared handle to a video engine implementation.
pub type DynEngine = Arc<dyn VideoEngine>;

/// Capabilities advertised by a [`VideoEngine`].
#[derive(Debug, Clone, Default)]
pub struct EngineCapabilities {
    /// Human-readable engine identifier (e.g. `"ffmpeg"`, `"mlvc"`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Codec ids this engine can encode (as returned by [`Codec::as_str`]).
    pub codecs: Vec<String>,
    /// Whether the engine can probe media containers/streams.
    pub can_probe: bool,
    /// Whether the engine can extract time ranges without re-encoding.
    pub can_extract: bool,
    /// Whether the engine can concatenate bitstreams without re-encoding.
    pub can_concat: bool,
    /// Whether the engine supports chunked parallel encode.
    pub can_chunked_encode: bool,
    /// Whether hardware-accelerated decode hints are honored.
    pub supports_hwaccel: bool,
}

/// Pluggable video encode / probe / edit backend.
///
/// Pipelines depend on this trait instead of a concrete FFmpeg (or other)
/// implementation. Register the active engine with [`crate::set_default_engine`].
pub trait VideoEngine: Send + Sync {
    /// Stable engine id (`"ffmpeg"`, `"external"`, `"mlvc"`, …).
    fn id(&self) -> &str;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Advertised capabilities.
    fn capabilities(&self) -> EngineCapabilities;

    /// Whether this engine can encode with the given codec.
    fn supports_codec(&self, codec: &Codec) -> bool {
        let id = codec.as_str();
        self.capabilities().codecs.iter().any(|c| c == id)
    }

    /// Probe a media file for format and stream metadata.
    fn probe<'a>(&'a self, path: &'a str) -> BoxFuture<'a, anyhow::Result<ProbeResult>>;

    /// Encode according to `job`, optionally streaming progress updates.
    fn encode<'a>(
        &'a self,
        job: EncodeJob,
        progress: Option<tokio::sync::mpsc::Sender<Progress>>,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>>;

    /// Copy a time range of a media file without re-encoding when possible.
    fn extract<'a>(
        &'a self,
        input: &'a str,
        output: &'a str,
        start: f64,
        duration: f64,
    ) -> BoxFuture<'a, anyhow::Result<()>>;

    /// Concatenate bitstreams without re-encoding when possible.
    fn concat<'a>(
        &'a self,
        inputs: &'a [String],
        output: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>>;

    /// Encode a source in chunks and concatenate the results.
    ///
    /// Default implementation encodes chunks **sequentially** using
    /// [`chunk_plan`], [`VideoEngine::encode`], and [`VideoEngine::concat`].
    /// Engines that support true parallel chunking (e.g. FFmpeg) should
    /// override this method.
    fn chunked_encode<'a>(
        &'a self,
        job: EncodeJob,
        chunk_seconds: f64,
        _parallel: usize,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        Box::pin(async move {
            if !self.capabilities().can_chunked_encode {
                anyhow::bail!("engine '{}' does not support chunked encode", self.id());
            }

            let probe_result = self.probe(&job.input).await?;
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
                return self.encode(job, None).await;
            }

            let tmp_dir = std::env::temp_dir().join(format!(
                "viser-chunked-{}-{}",
                self.id(),
                std::process::id()
            ));
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
                self.encode(chunk_job, None).await?;
                outputs.push(output_path);
            }

            let started = std::time::Instant::now();
            self.concat(&outputs, &job.output).await?;

            let meta = std::fs::metadata(&job.output)?;
            let output_probe = self.probe(&job.output).await?;
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
