use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{ProbeResult, probe};

/// Thread-safe probe result cache to avoid redundant ffprobe calls.
#[derive(Debug, Clone)]
pub struct ProbeCache {
    cache: Arc<RwLock<HashMap<String, ProbeResult>>>,
    engine: ProbeEngine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeEngine {
    Ffprobe,
    #[cfg(feature = "revelo")]
    Revelo,
}

impl ProbeCache {
    pub fn new() -> Self {
        Self { cache: Arc::default(), engine: ProbeEngine::Ffprobe }
    }

    #[cfg(feature = "revelo")]
    pub fn with_revelo() -> Self {
        Self { cache: Arc::default(), engine: ProbeEngine::Revelo }
    }

    pub fn with_engine(engine: ProbeEngine) -> Self {
        Self { cache: Arc::default(), engine }
    }

    /// Returns cached result or calls ffprobe and caches the result.
    pub async fn probe(&self, path: &str) -> anyhow::Result<ProbeResult> {
        {
            let cache = self.cache.read().await;
            if let Some(result) = cache.get(path) {
                return Ok(result.clone());
            }
        }

        let result = match self.engine {
            ProbeEngine::Ffprobe => probe(path).await?,
            #[cfg(feature = "revelo")]
            ProbeEngine::Revelo => crate::probe_revelo(path).await?,
        };

        {
            let mut cache = self.cache.write().await;
            cache.insert(path.to_string(), result.clone());
        }

        Ok(result)
    }
}

impl Default for ProbeCache {
    fn default() -> Self {
        Self::new()
    }
}
