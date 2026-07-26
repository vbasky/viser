//! FFmpeg/FFprobe implementation of [`viser_engine::VideoEngine`].

use std::sync::Arc;

use viser_engine::{
    BoxFuture, Codec, DynEngine, EncodeJob, EncodeResult, EngineCapabilities, ProbeResult,
    Progress, VideoEngine,
};

use crate::probe::probe as ffmpeg_probe;
use crate::{
    chunked_encode as ffmpeg_chunked_encode, concat as ffmpeg_concat, encode as ffmpeg_encode,
    extract as ffmpeg_extract,
};

/// FFmpeg/FFprobe video engine.
///
/// Encode, probe, extract, and concat all go through the `ffmpeg` / `ffprobe`
/// binaries resolved by [`crate::ffmpeg_path`] / [`crate::ffprobe_path`].
#[derive(Debug, Default, Clone, Copy)]
pub struct FfmpegEngine;

impl FfmpegEngine {
    /// Creates a new FFmpeg engine handle.
    pub fn new() -> Self {
        Self
    }
}

impl VideoEngine for FfmpegEngine {
    fn id(&self) -> &str {
        "ffmpeg"
    }

    fn name(&self) -> &str {
        "FFmpeg / FFprobe"
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            id: "ffmpeg".into(),
            name: "FFmpeg / FFprobe".into(),
            codecs: classical_codec_ids(),
            can_probe: true,
            can_extract: true,
            can_concat: true,
            can_chunked_encode: true,
            supports_hwaccel: true,
        }
    }

    fn supports_codec(&self, codec: &Codec) -> bool {
        !codec.is_external()
    }

    fn probe<'a>(&'a self, path: &'a str) -> BoxFuture<'a, anyhow::Result<ProbeResult>> {
        Box::pin(async move { ffmpeg_probe(path).await })
    }

    fn encode<'a>(
        &'a self,
        job: EncodeJob,
        progress: Option<tokio::sync::mpsc::Sender<Progress>>,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        Box::pin(async move {
            if job.codec.is_external() {
                anyhow::bail!(
                    "codec '{}' requires a non-FFmpeg engine; set a different default engine",
                    job.codec
                );
            }
            ffmpeg_encode(job, progress).await
        })
    }

    fn extract<'a>(
        &'a self,
        input: &'a str,
        output: &'a str,
        start: f64,
        duration: f64,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { ffmpeg_extract(input, output, start, duration).await })
    }

    fn concat<'a>(
        &'a self,
        inputs: &'a [String],
        output: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { ffmpeg_concat(inputs, output).await })
    }

    fn chunked_encode<'a>(
        &'a self,
        job: EncodeJob,
        chunk_seconds: f64,
        parallel: usize,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        Box::pin(async move {
            if job.codec.is_external() {
                anyhow::bail!(
                    "codec '{}' requires a non-FFmpeg engine; set a different default engine",
                    job.codec
                );
            }
            ffmpeg_chunked_encode(job, chunk_seconds, parallel).await
        })
    }
}

fn classical_codec_ids() -> Vec<String> {
    [
        Codec::X264,
        Codec::X265,
        Codec::SvtAv1,
        Codec::Vp9,
        Codec::NvencH264,
        Codec::QsvH264,
        Codec::VideoToolboxH264,
        Codec::VaapiH264,
        Codec::AmfH264,
        Codec::NvencH265,
        Codec::QsvH265,
        Codec::VideoToolboxH265,
        Codec::VaapiH265,
        Codec::AmfH265,
        Codec::NvencAv1,
        Codec::QsvAv1,
        Codec::VaapiAv1,
        Codec::AmfAv1,
    ]
    .into_iter()
    .map(|c| c.as_str().to_string())
    .collect()
}

/// Returns an [`Arc`]-wrapped FFmpeg engine suitable for [`viser_engine::set_default_engine`].
pub fn ffmpeg_engine() -> DynEngine {
    Arc::new(FfmpegEngine::new())
}

/// Registers the FFmpeg engine as the process-wide default.
///
/// Safe to call multiple times; subsequent calls replace the previous default.
pub fn register_as_default() {
    viser_engine::set_default_engine(ffmpeg_engine());
}
