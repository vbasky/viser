//! Resolve a process [`DynEngine`] from CLI / env options.
//!
//! Supports pure FFmpeg, pure external/MLVC, and dual-engine composites
//! (FFmpeg for media ops + external encode).

use std::sync::Arc;

use viser_engine::{
    CompositeEngine, DynEngine, ExternalEngine, ExternalEngineConfig, MlvcConfig, MlvcModel,
    MlvcVariant,
};

use crate::ffmpeg_engine;

/// Which backend to use for a role (media vs encode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineKind {
    /// FFmpeg / FFprobe.
    Ffmpeg,
    /// Generic shell-out (`VISER_EXTERNAL_ENCODE`).
    External,
    /// Microsoft MLVC neural codec.
    Mlvc,
}

impl EngineKind {
    /// Parse from CLI string.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ffmpeg" | "ff" | "default" => Ok(Self::Ffmpeg),
            "external" | "ext" | "cmd" => Ok(Self::External),
            "mlvc" | "mlvc-s" | "neural" => Ok(Self::Mlvc),
            other => anyhow::bail!("unknown engine '{other}' (expected ffmpeg|external|mlvc)"),
        }
    }

    /// Stable id string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::External => "external",
            Self::Mlvc => "mlvc",
        }
    }
}

/// Options controlling which engines are wired.
#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    /// Shorthand: sets both media and encode when specific fields are unset.
    pub engine: Option<EngineKind>,
    /// Probe / extract / concat backend.
    pub media: Option<EngineKind>,
    /// Encode backend.
    pub encode: Option<EngineKind>,
    /// Override for `VISER_EXTERNAL_ENCODE` template.
    pub external_encode: Option<String>,
    /// Override for `VISER_EXTERNAL_PROBE` template.
    pub external_probe: Option<String>,
    /// MLVC encode command / template (`VISER_MLVC_CMD`).
    pub mlvc_cmd: Option<String>,
    /// MLVC model: psnr | perceptual.
    pub mlvc_model: Option<String>,
    /// MLVC variant: full | s.
    pub mlvc_variant: Option<String>,
    /// MLVC weights path.
    pub mlvc_weights: Option<String>,
}

impl EngineOptions {
    /// Resolved media engine kind (probe/extract/concat).
    ///
    /// Non-FFmpeg encode backends default **media** to FFmpeg so probe/extract
    /// and quality measurement keep working (dual-engine composite).
    pub fn media_kind(&self) -> EngineKind {
        if let Some(m) = &self.media {
            return m.clone();
        }
        let encode = self.encode_kind();
        if !matches!(encode, EngineKind::Ffmpeg) {
            return EngineKind::Ffmpeg;
        }
        self.engine.clone().unwrap_or(EngineKind::Ffmpeg)
    }

    /// Resolved encode engine kind.
    pub fn encode_kind(&self) -> EngineKind {
        self.encode.clone().or_else(|| self.engine.clone()).unwrap_or(EngineKind::Ffmpeg)
    }
}

/// Builds a [`DynEngine`] from options and registers nothing (caller may
/// [`viser_engine::set_default_engine`]).
pub fn resolve_engine(opts: &EngineOptions) -> anyhow::Result<DynEngine> {
    let media_kind = opts.media_kind();
    let encode_kind = opts.encode_kind();

    let media = build_kind(media_kind.clone(), opts)?;
    let encode = if encode_kind == media_kind && !needs_distinct_external(opts, &encode_kind) {
        media.clone()
    } else {
        build_kind(encode_kind.clone(), opts)?
    };

    if media.id() == encode.id() && Arc::ptr_eq(&media, &encode) {
        return Ok(media);
    }
    // Even if ids match, prefer composite when roles differ by kind.
    if media_kind == encode_kind {
        return Ok(encode);
    }

    Ok(Arc::new(CompositeEngine::new(media, encode)))
}

fn needs_distinct_external(opts: &EngineOptions, kind: &EngineKind) -> bool {
    matches!(kind, EngineKind::External | EngineKind::Mlvc)
        && (opts.external_encode.is_some()
            || opts.mlvc_cmd.is_some()
            || opts.mlvc_model.is_some()
            || opts.mlvc_variant.is_some()
            || opts.mlvc_weights.is_some())
}

fn build_kind(kind: EngineKind, opts: &EngineOptions) -> anyhow::Result<DynEngine> {
    match kind {
        EngineKind::Ffmpeg => Ok(ffmpeg_engine()),
        EngineKind::External => {
            let mut cfg = ExternalEngineConfig::default();
            if let Some(enc) = &opts.external_encode {
                cfg.encode_template = enc.clone();
            }
            if let Some(probe) = &opts.external_probe {
                cfg.probe_template = Some(probe.clone());
            }
            if cfg.encode_template.is_empty() {
                anyhow::bail!(
                    "external encode engine requires --external-encode or VISER_EXTERNAL_ENCODE"
                );
            }
            Ok(Arc::new(ExternalEngine::new(cfg)))
        }
        EngineKind::Mlvc => {
            let mut cfg = MlvcConfig::default();
            if let Some(cmd) = &opts.mlvc_cmd {
                cfg.command = cmd.clone();
            }
            if let Some(m) = &opts.mlvc_model {
                cfg.model = MlvcModel::parse(m)?;
            }
            if let Some(v) = &opts.mlvc_variant {
                cfg.variant = MlvcVariant::parse(v)?;
            }
            if let Some(w) = &opts.mlvc_weights {
                cfg.weights = Some(w.clone());
            }
            Ok(cfg.into_dyn())
        }
    }
}

/// Convenience: resolve and install as the process-wide default.
pub fn install_engine(opts: &EngineOptions) -> anyhow::Result<DynEngine> {
    let engine = resolve_engine(opts)?;
    viser_engine::set_default_engine(engine.clone());
    Ok(engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_ffmpeg() {
        let eng = resolve_engine(&EngineOptions::default()).unwrap();
        assert_eq!(eng.id(), "ffmpeg");
    }

    #[test]
    fn dual_mlvc_is_composite() {
        let opts = EngineOptions {
            engine: Some(EngineKind::Mlvc),
            mlvc_cmd: Some("mlvc-encode".into()),
            ..Default::default()
        };
        assert_eq!(opts.media_kind(), EngineKind::Ffmpeg);
        assert_eq!(opts.encode_kind(), EngineKind::Mlvc);
        let eng = resolve_engine(&opts).unwrap();
        assert_eq!(eng.id(), "composite");
        let caps = eng.capabilities();
        assert!(caps.id.contains("ffmpeg"));
        assert!(caps.id.contains("mlvc"));
    }

    #[test]
    fn external_requires_template() {
        let opts = EngineOptions {
            encode: Some(EngineKind::External),
            external_encode: None,
            ..Default::default()
        };
        // May succeed if VISER_EXTERNAL_ENCODE is set in the environment.
        match resolve_engine(&opts) {
            Ok(e) => assert!(e.id() == "composite" || e.id() == "external"),
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("external encode") || msg.contains("VISER_EXTERNAL"));
            }
        }
    }
}
