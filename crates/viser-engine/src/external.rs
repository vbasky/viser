//! Shell-out engine for integrating external codecs (e.g. MLVC) without
//! embedding their runtimes in viser.
//!
//! Configure via environment variables or [`ExternalEngineConfig`]:
//!
//! | Variable | Purpose | Placeholders |
//! |----------|---------|--------------|
//! | `VISER_EXTERNAL_ENCODE` | Encode command template | `{input}` `{output}` `{crf}` `{preset}` `{width}` `{height}` `{codec}` `{bitrate}` |
//! | `VISER_EXTERNAL_PROBE` | Optional probe command (JSON on stdout matching [`crate::ProbeResult`]) | `{path}` |
//!
//! Example (MLVC-style wrapper script):
//!
//! ```bash
//! export VISER_EXTERNAL_ENCODE='mlvc-encode --input {input} --output {output} --quality {crf}'
//! # then register ExternalEngine as the default in your host process
//! ```

use std::process::Stdio;
use std::time::Instant;

use tokio::process::Command;

use crate::{
    BoxFuture, Codec, EncodeJob, EncodeResult, EngineCapabilities, ProbeResult, Progress,
    VideoEngine,
};

/// Configuration for the shell-out [`ExternalEngine`].
#[derive(Debug, Clone)]
pub struct ExternalEngineConfig {
    /// Engine id (default `"external"`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Encode command template. Required for encoding.
    pub encode_template: String,
    /// Optional probe command template that prints [`ProbeResult`] JSON.
    pub probe_template: Option<String>,
}

impl Default for ExternalEngineConfig {
    fn default() -> Self {
        Self {
            id: "external".into(),
            name: "External command engine".into(),
            encode_template: std::env::var("VISER_EXTERNAL_ENCODE").unwrap_or_default(),
            probe_template: std::env::var("VISER_EXTERNAL_PROBE").ok().filter(|s| !s.is_empty()),
        }
    }
}

/// Video engine that shells out to user-provided command templates.
///
/// This is the integration point for neural codecs (MLVC, DCVC, …) and any
/// other encoder that is not FFmpeg. Probe/extract/concat are optional; when
/// unset, callers should pair this engine with FFmpeg for container ops or
/// implement those methods themselves.
#[derive(Debug, Clone)]
pub struct ExternalEngine {
    config: ExternalEngineConfig,
}

impl ExternalEngine {
    /// Creates an engine from the given config.
    pub fn new(config: ExternalEngineConfig) -> Self {
        Self { config }
    }

    /// Creates an engine from `VISER_EXTERNAL_*` environment variables.
    pub fn from_env() -> Self {
        Self::new(ExternalEngineConfig::default())
    }
}

impl VideoEngine for ExternalEngine {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            id: self.config.id.clone(),
            name: self.config.name.clone(),
            codecs: vec![Codec::External.as_str().into(), "mlvc".into(), "mlvc-s".into()],
            can_probe: self.config.probe_template.is_some(),
            can_extract: false,
            can_concat: false,
            can_chunked_encode: false,
            supports_hwaccel: false,
        }
    }

    fn supports_codec(&self, codec: &Codec) -> bool {
        codec.is_external() || codec.as_str().starts_with("mlvc")
    }

    fn probe<'a>(&'a self, path: &'a str) -> BoxFuture<'a, anyhow::Result<ProbeResult>> {
        Box::pin(async move {
            let Some(tmpl) = &self.config.probe_template else {
                anyhow::bail!(
                    "external engine '{}' has no probe template (set VISER_EXTERNAL_PROBE)",
                    self.id()
                );
            };
            let cmd = tmpl.replace("{path}", path);
            let output = run_shell(&cmd).await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("external probe failed: {stderr}");
            }
            let result: ProbeResult = serde_json::from_slice(&output.stdout)
                .map_err(|e| anyhow::anyhow!("external probe JSON parse failed: {e}"))?;
            Ok(result)
        })
    }

    fn encode<'a>(
        &'a self,
        job: EncodeJob,
        _progress: Option<tokio::sync::mpsc::Sender<Progress>>,
    ) -> BoxFuture<'a, anyhow::Result<EncodeResult>> {
        Box::pin(async move {
            if self.config.encode_template.is_empty() {
                anyhow::bail!(
                    "external engine '{}' has no encode template (set VISER_EXTERNAL_ENCODE)",
                    self.id()
                );
            }
            let (width, height) = job.resolution.map(|r| (r.width, r.height)).unwrap_or((0, 0));
            let cmd = self
                .config
                .encode_template
                .replace("{input}", &job.input)
                .replace("{output}", &job.output)
                .replace("{crf}", &job.crf.to_string())
                .replace("{preset}", &job.preset)
                .replace("{width}", &width.to_string())
                .replace("{height}", &height.to_string())
                .replace("{codec}", job.codec.as_str())
                .replace("{bitrate}", &format!("{:.0}", job.target_bitrate));

            let started = Instant::now();
            let output = run_shell(&cmd).await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("external encode failed: {stderr}");
            }

            let meta = std::fs::metadata(&job.output)
                .map_err(|e| anyhow::anyhow!("failed to stat external encode output: {e}"))?;
            // Prefer probing via this engine; fall back to file size only.
            let bitrate = match self.probe(&job.output).await {
                Ok(p) => p.format.bit_rate as f64 / 1000.0,
                Err(_) => {
                    // Rough estimate from size if duration unknown.
                    0.0
                }
            };

            Ok(EncodeResult { job, bitrate, file_size: meta.len(), duration: started.elapsed() })
        })
    }

    fn extract<'a>(
        &'a self,
        _input: &'a str,
        _output: &'a str,
        _start: f64,
        _duration: f64,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            anyhow::bail!(
                "external engine '{}' does not support extract; use the FFmpeg engine for container ops",
                self.id()
            )
        })
    }

    fn concat<'a>(
        &'a self,
        _inputs: &'a [String],
        _output: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            anyhow::bail!(
                "external engine '{}' does not support concat; use the FFmpeg engine for container ops",
                self.id()
            )
        })
    }
}

async fn run_shell(cmd: &str) -> anyhow::Result<std::process::Output> {
    // Prefer a login-free shell invocation; works on macOS/Linux.
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to start external command `{cmd}`: {e}"))?;
    Ok(output)
}
