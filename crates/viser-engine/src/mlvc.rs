//! First-class [MLVC](https://github.com/microsoft/mlvc) engine configuration.
//!
//! MLVC is a neural video codec that does not ship as an FFmpeg encoder. Viser
//! drives it through [`ExternalEngine`] command templates that you supply
//! (e.g. `VISER_MLVC_CMD` or `--mlvc-cmd`).
//!
//! Dual-engine (recommended):
//!
//! ```text
//! probe/extract = FFmpeg
//! encode        = MlvcEngine (shell-out)
//! ```
//!
//! Environment:
//!
//! | Variable | Purpose |
//! |----------|---------|
//! | `VISER_MLVC_CMD` | Encode command template or executable prefix |
//! | `VISER_MLVC_MODEL` | `psnr` or `perceptual` (default `psnr`) |
//! | `VISER_MLVC_VARIANT` | `full` or `s` (default `full`) |
//! | `VISER_MLVC_WEIGHTS` | Optional checkpoint path |

use crate::{Codec, DynEngine, ExternalEngine, ExternalEngineConfig, arc_engine};

/// MLVC model objective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MlvcModel {
    /// PSNR-trained checkpoint.
    #[default]
    Psnr,
    /// Perceptually fine-tuned checkpoint.
    Perceptual,
}

impl MlvcModel {
    /// Parse from CLI / env string.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "psnr" => Ok(Self::Psnr),
            "perceptual" | "perc" | "lpips" => Ok(Self::Perceptual),
            other => anyhow::bail!("unknown MLVC model '{other}' (expected psnr|perceptual)"),
        }
    }

    /// Env / flag string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Psnr => "psnr",
            Self::Perceptual => "perceptual",
        }
    }
}

/// MLVC network size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MlvcVariant {
    /// Full MLVC (~18M params).
    #[default]
    Full,
    /// MLVC-S small model (~5.4M params).
    Small,
}

impl MlvcVariant {
    /// Parse from CLI / env string.
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "full" | "mlvc" | "default" => Ok(Self::Full),
            "s" | "small" | "mlvc-s" | "mlvcs" => Ok(Self::Small),
            other => anyhow::bail!("unknown MLVC variant '{other}' (expected full|s)"),
        }
    }

    /// Env / flag string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Small => "s",
        }
    }
}

/// Configuration for the MLVC encode backend.
#[derive(Debug, Clone)]
pub struct MlvcConfig {
    /// Command template. May be a full template with placeholders or an
    /// executable on `PATH`.
    ///
    /// Default: `$VISER_MLVC_CMD` or `mlvc-encode`.
    pub command: String,
    /// Training objective / checkpoint family.
    pub model: MlvcModel,
    /// Network size.
    pub variant: MlvcVariant,
    /// Optional path to weights checkpoint.
    pub weights: Option<String>,
}

impl Default for MlvcConfig {
    fn default() -> Self {
        Self {
            command: std::env::var("VISER_MLVC_CMD").unwrap_or_else(|_| "mlvc-encode".into()),
            model: std::env::var("VISER_MLVC_MODEL")
                .ok()
                .and_then(|s| MlvcModel::parse(&s).ok())
                .unwrap_or_default(),
            variant: std::env::var("VISER_MLVC_VARIANT")
                .ok()
                .and_then(|s| MlvcVariant::parse(&s).ok())
                .unwrap_or_default(),
            weights: std::env::var("VISER_MLVC_WEIGHTS").ok().filter(|s| !s.is_empty()),
        }
    }
}

impl MlvcConfig {
    /// Builds the shell encode template consumed by [`ExternalEngine`].
    ///
    /// If `command` already contains `{input}`, it is used as a full template
    /// (with model/variant/weights substituted). Otherwise a default flag layout
    /// is appended:
    ///
    /// ```text
    /// <command> --input {input} --output {output} --quality {crf} \
    ///   --model <model> --variant <variant> [--weights <path>] \
    ///   --width {width} --height {height}
    /// ```
    pub fn encode_template(&self) -> String {
        let mut tmpl = if self.command.contains("{input}") {
            self.command.clone()
        } else {
            format!(
                "{} --input {{input}} --output {{output}} --quality {{crf}} \
                 --model {} --variant {} --width {{width}} --height {{height}}",
                self.command,
                self.model.as_str(),
                self.variant.as_str(),
            )
        };
        tmpl = tmpl
            .replace("{model}", self.model.as_str())
            .replace("{variant}", self.variant.as_str());
        if let Some(w) = &self.weights {
            if !tmpl.contains(w) && !tmpl.contains("{weights}") {
                tmpl.push_str(" --weights ");
                tmpl.push_str(w);
            }
            tmpl = tmpl.replace("{weights}", w);
        }
        tmpl
    }

    /// Wraps this config as an [`ExternalEngine`] with id `mlvc`.
    pub fn into_engine(self) -> ExternalEngine {
        ExternalEngine::new(ExternalEngineConfig {
            id: "mlvc".into(),
            name: format!("MLVC ({}/{})", self.variant.as_str(), self.model.as_str()),
            encode_template: self.encode_template(),
            probe_template: None,
        })
    }

    /// Arc-wrapped engine handle.
    pub fn into_dyn(self) -> DynEngine {
        arc_engine(self.into_engine())
    }

    /// Codecs this engine accepts.
    pub fn codecs() -> &'static [Codec] {
        &[Codec::External]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_shape() {
        let cfg = MlvcConfig {
            command: "mlvc-encode".into(),
            model: MlvcModel::Psnr,
            variant: MlvcVariant::Small,
            weights: Some("/w.ckpt".into()),
        };
        let t = cfg.encode_template();
        assert!(t.contains("--input {input}"));
        assert!(t.contains("--quality {crf}"));
        assert!(t.contains("--variant s"));
        assert!(t.contains("--weights /w.ckpt"));
    }

    #[test]
    fn full_template_passthrough() {
        let cfg = MlvcConfig {
            command: "python wrap.py -i {input} -o {output} -q {crf} -m {model}".into(),
            model: MlvcModel::Perceptual,
            variant: MlvcVariant::Full,
            weights: None,
        };
        let t = cfg.encode_template();
        assert!(t.contains("-m perceptual"));
        assert!(t.contains("{input}"));
    }
}
