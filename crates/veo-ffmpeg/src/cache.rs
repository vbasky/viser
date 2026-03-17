use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{probe, ProbeResult};

/// Thread-safe probe result cache to avoid redundant ffprobe calls.
#[derive(Debug, Clone, Default)]
pub struct ProbeCache {
    cache: Arc<RwLock<HashMap<String, ProbeResult>>>,
}

impl ProbeCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns cached result or calls ffprobe and caches the result.
    pub async fn probe(&self, path: &str) -> anyhow::Result<ProbeResult> {
        // Check read lock first
        {
            let cache = self.cache.read().await;
            if let Some(result) = cache.get(path) {
                return Ok(result.clone());
            }
        }

        // Not cached — probe and insert
        let result = probe(path).await?;
        {
            let mut cache = self.cache.write().await;
            cache.insert(path.to_string(), result.clone());
        }

        Ok(result)
    }
}
