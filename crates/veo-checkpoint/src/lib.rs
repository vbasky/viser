use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use veo_hull::Point;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub config_hash: String,
    pub source: String,
    pub completed: HashMap<String, Point>,
}

/// Manages incremental trial result persistence.
pub struct Checkpoint {
    mu: Mutex<CheckpointInner>,
}

struct CheckpointInner {
    path: PathBuf,
    state: State,
    dirty: bool,
}

impl Checkpoint {
    /// Creates a checkpoint manager. If a checkpoint file exists with a matching
    /// config hash, completed trials are loaded.
    pub fn new(path: impl AsRef<Path>, config_hash: &str, source: &str) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut state = State {
            config_hash: config_hash.to_string(),
            source: source.to_string(),
            completed: HashMap::new(),
        };

        if let Ok(data) = fs::read(&path) {
            if let Ok(existing) = serde_json::from_slice::<State>(&data) {
                if existing.config_hash == config_hash && existing.source == source {
                    state = existing;
                }
            }
        }

        Ok(Self {
            mu: Mutex::new(CheckpointInner {
                path,
                state,
                dirty: false,
            }),
        })
    }

    pub fn is_completed(&self, resolution: &str, codec: &str, crf: i32) -> bool {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.contains_key(&make_key(resolution, codec, crf))
    }

    pub fn get(&self, resolution: &str, codec: &str, crf: i32) -> Option<Point> {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.get(&make_key(resolution, codec, crf)).cloned()
    }

    pub fn save(&self, resolution: &str, codec: &str, crf: i32, point: Point) -> anyhow::Result<()> {
        let mut inner = self.mu.lock().unwrap();
        inner.state.completed.insert(make_key(resolution, codec, crf), point);
        inner.dirty = true;
        flush_locked(&mut inner)
    }

    pub fn completed_count(&self) -> usize {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.len()
    }

    pub fn all_completed(&self) -> Vec<Point> {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.values().cloned().collect()
    }

    pub fn remove(&self) -> anyhow::Result<()> {
        let inner = self.mu.lock().unwrap();
        fs::remove_file(&inner.path)?;
        Ok(())
    }
}

fn flush_locked(inner: &mut CheckpointInner) -> anyhow::Result<()> {
    if !inner.dirty {
        return Ok(());
    }

    let data = serde_json::to_string_pretty(&inner.state)?;

    // Write atomically via temp file + rename
    let dir = inner.path.parent().unwrap_or(Path::new("."));
    let tmp = tempfile::NamedTempFile::new_in(dir)?;
    fs::write(tmp.path(), &data)?;
    tmp.persist(&inner.path)?;

    inner.dirty = false;
    Ok(())
}

/// Computes a deterministic hash of encoding configuration parameters.
pub fn config_hash(
    source: &str,
    resolutions: &[String],
    codecs: &[String],
    crf_values: &[i32],
    preset: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("source={source}\n"));
    for r in resolutions {
        hasher.update(format!("res={r}\n"));
    }
    for c in codecs {
        hasher.update(format!("codec={c}\n"));
    }
    for crf in crf_values {
        hasher.update(format!("crf={crf}\n"));
    }
    hasher.update(format!("preset={preset}\n"));
    let hash = hasher.finalize();
    hex::encode(&hash[..8])
}

/// Returns the default checkpoint file path for a given source.
pub fn default_path(source: &str) -> PathBuf {
    let path = Path::new(source);
    let dir = path.parent().unwrap_or(Path::new("."));
    let base = path.file_name().unwrap_or_default().to_string_lossy();
    dir.join(format!(".veo-checkpoint-{base}.json"))
}

fn make_key(resolution: &str, codec: &str, crf: i32) -> String {
    format!("{resolution}_{codec}_{crf}")
}
