use std::sync::{Arc, OnceLock, RwLock};

use crate::{DynEngine, EncodeJob, EncodeResult, ProbeResult, Progress, VideoEngine};

static DEFAULT_ENGINE: OnceLock<RwLock<Option<DynEngine>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<DynEngine>> {
    DEFAULT_ENGINE.get_or_init(|| RwLock::new(None))
}

/// Registers the process-wide default [`VideoEngine`].
///
/// Call once at startup (the CLI does this with the FFmpeg engine). Subsequent
/// calls replace the previous default.
pub fn set_default_engine(engine: DynEngine) {
    *slot().write().expect("engine registry poisoned") = Some(engine);
}

/// Returns the process-wide default engine.
///
/// # Errors
///
/// Returns an error if no engine has been registered via [`set_default_engine`].
pub fn default_engine() -> anyhow::Result<DynEngine> {
    slot().read().expect("engine registry poisoned").clone().ok_or_else(|| {
        anyhow::anyhow!(
            "no default video engine registered; call viser_engine::set_default_engine() at startup"
        )
    })
}

/// Returns the default engine if one is registered.
pub fn try_default_engine() -> Option<DynEngine> {
    slot().read().expect("engine registry poisoned").clone()
}

/// Probe a media file using the default engine.
pub async fn probe(path: &str) -> anyhow::Result<ProbeResult> {
    default_engine()?.probe(path).await
}

/// Encode using the default engine.
pub async fn encode(
    job: EncodeJob,
    progress: Option<tokio::sync::mpsc::Sender<Progress>>,
) -> anyhow::Result<EncodeResult> {
    default_engine()?.encode(job, progress).await
}

/// Extract a time range using the default engine.
pub async fn extract(input: &str, output: &str, start: f64, duration: f64) -> anyhow::Result<()> {
    default_engine()?.extract(input, output, start, duration).await
}

/// Concatenate bitstreams using the default engine.
pub async fn concat(inputs: &[String], output: &str) -> anyhow::Result<()> {
    default_engine()?.concat(inputs, output).await
}

/// Chunked encode using the default engine.
pub async fn chunked_encode(
    job: EncodeJob,
    chunk_seconds: f64,
    parallel: usize,
) -> anyhow::Result<EncodeResult> {
    default_engine()?.chunked_encode(job, chunk_seconds, parallel).await
}

/// Build an [`Arc`] engine handle.
pub fn arc_engine<E: VideoEngine + 'static>(engine: E) -> DynEngine {
    Arc::new(engine)
}
